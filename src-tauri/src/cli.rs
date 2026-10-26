//! 外部 CLI 子进程的运行环境。
//!
//! 从 Finder / 资源管理器启动的 GUI 应用只拿到系统 PATH（macOS 通常是
//! `/usr/bin:/bin:/usr/sbin:/sbin`）。npm / nvm 安装的 `codex`、`opencode`
//! 往往是 `#!/usr/bin/env node` 脚本：脚本本身能找到，但解释器不在 PATH 里，
//! `env` 会以退出码 127 结束。这里把 CLI 自己的目录与常见安装目录补到子进程
//! 的 PATH 前面，只影响子进程，不改动本进程环境。

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 给子进程设置 PATH：CLI 所在目录优先，其次是常见安装目录，最后是继承来的 PATH。
pub fn apply_child_path(command: &mut Command, binary: &Path) {
    if let Some(path) = child_path(binary, env::var_os("PATH")) {
        command.env("PATH", path);
    }
}

/// 打包后的应用是 GUI 子系统进程，自己没有控制台；此时再启动控制台程序
/// （`codex.exe`，或 `.cmd` 包装脚本背后的 `cmd.exe`），Windows 会为子进程
/// 新建一个控制台窗口，额度刷新与授权等待期间一直挂在桌面上。
/// `CREATE_NO_WINDOW` 让子进程在没有窗口的控制台里运行，stdin/stdout 管道照常可用。
#[cfg(windows)]
pub fn apply_no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    /// Win32 `CREATE_NO_WINDOW`，仅在新建控制台时有效。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn apply_no_window(_command: &mut Command) {}

fn child_path(binary: &Path, inherited: Option<OsString>) -> Option<OsString> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = binary.parent() {
        push_unique(&mut dirs, parent.to_path_buf());
    }
    for dir in tool_dirs() {
        push_unique(&mut dirs, dir);
    }
    if let Some(inherited) = inherited.as_deref() {
        for dir in env::split_paths(inherited) {
            push_unique(&mut dirs, dir);
        }
    }
    // 拼接失败时（例如条目里含引号）保持继承值不动。
    env::join_paths(dirs).ok()
}

fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.as_os_str().is_empty() || dirs.contains(&dir) {
        return;
    }
    dirs.push(dir);
}

/// 解释器可能所在的目录。CLI 与 `node` 装在不同管理器里时（例如 pnpm 全局
/// 安装的 CLI 配 Homebrew 的 node），只补 CLI 自己的目录还不够。
fn tool_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            // Volta / Bun / pnpm 的全局 bin
            dirs.push(home.join(".volta").join("bin"));
            dirs.push(home.join(".bun").join("bin"));
            dirs.push(home.join(".local").join("share").join("pnpm"));
            dirs.push(home.join("Library").join("pnpm"));
            // nvm / fnm 的 Node 版本目录，新版本排在前面
            push_version_bins(
                &mut dirs,
                &home.join(".nvm").join("versions").join("node"),
                &["bin"],
            );
            push_version_bins(
                &mut dirs,
                &home
                    .join(".local")
                    .join("share")
                    .join("fnm")
                    .join("node-versions"),
                &["installation", "bin"],
            );
            push_version_bins(
                &mut dirs,
                &home
                    .join("Library")
                    .join("Application Support")
                    .join("fnm")
                    .join("node-versions"),
                &["installation", "bin"],
            );
        }
    }
    #[cfg(windows)]
    {
        // 资源管理器启动的进程本来就继承用户 PATH，这里只兜底 PATH 被裁剪的情况。
        if let Some(base) = env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(base.join("npm"));
        }
        if let Some(base) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(base.join("pnpm"));
            dirs.push(base.join("Volta").join("bin"));
        }
        if let Some(home) = home_dir() {
            dirs.push(home.join(".bun").join("bin"));
        }
    }
    dirs
}

/// 版本管理器的版本目录按字符串降序（v26.12.0 > v20.0.0），只收真实存在的 bin。
fn push_version_bins(dirs: &mut Vec<PathBuf>, root: &Path, suffix: &[&str]) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut versions: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort_by(|a, b| b.cmp(a));
    for version in versions {
        let mut dir = version;
        for part in suffix {
            dir.push(part);
        }
        if dir.is_dir() {
            push_unique(dirs, dir);
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const KEYS: [&str; 2] = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const KEYS: [&str; 2] = ["HOME", "USERPROFILE"];
    KEYS.iter()
        .find_map(|key| env::var_os(key).map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(path: &OsString) -> Vec<PathBuf> {
        env::split_paths(path).collect()
    }

    #[test]
    fn binary_dir_comes_before_inherited_path() {
        let binary = PathBuf::from("/opt/tools/bin/codex");
        // 用 join_paths 生成，Windows 的条目分隔符是 `;`，不能直接写字面量。
        let inherited = env::join_paths(["/usr/bin", "/bin"]).expect("inherited path");
        let path = child_path(&binary, Some(inherited)).expect("joined");

        let dirs = entries(&path);
        assert_eq!(dirs.first(), Some(&PathBuf::from("/opt/tools/bin")));
        let inherited_at = dirs
            .iter()
            .position(|dir| dir.as_os_str() == "/usr/bin")
            .expect("inherited entry kept");
        assert!(inherited_at > 0);
        assert_eq!(dirs.last(), Some(&PathBuf::from("/bin")));
    }

    #[test]
    #[cfg(unix)]
    fn inherited_path_kept_when_it_matches_tool_dirs() {
        let binary = PathBuf::from("/opt/homebrew/bin/codex");
        let inherited = OsString::from("/opt/homebrew/bin:/usr/bin");
        let path = child_path(&binary, Some(inherited)).expect("joined");

        let dirs = entries(&path);
        let count = dirs
            .iter()
            .filter(|dir| dir.as_os_str() == "/opt/homebrew/bin")
            .count();
        assert_eq!(count, 1);
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")));
    }

    #[test]
    fn empty_inherited_path_still_yields_binary_dir() {
        let binary = PathBuf::from("/opt/tools/bin/codex");
        for inherited in [None, Some(OsString::new())] {
            let path = child_path(&binary, inherited).expect("joined");
            assert_eq!(
                entries(&path).first().map(PathBuf::as_path),
                Some(Path::new("/opt/tools/bin"))
            );
        }
    }

    #[test]
    fn parentless_binary_does_not_add_current_directory() {
        let path =
            child_path(Path::new("codex"), Some(OsString::from("/usr/bin"))).expect("joined");

        let dirs = entries(&path);
        assert!(!dirs.iter().any(|dir| dir.as_os_str().is_empty()));
        assert!(dirs.iter().any(|dir| dir.as_os_str() == "/usr/bin"));
    }

    /// 复现打包应用的现象：`#!/usr/bin/env node` 脚本在只有系统目录的 PATH 下
    /// 以 127 退出，补上 CLI 所在目录后才能找到同目录的 node。
    #[test]
    #[cfg(unix)]
    fn node_script_runs_after_path_is_prepared() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("token-refill-cli-{}", std::process::id()));
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("temp dir");
        let write_exec = |name: &str, body: &str| {
            let path = bin.join(name);
            fs::write(&path, body).expect("script");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("mode");
        };
        write_exec("node", "#!/bin/sh\necho fake-node\n");
        write_exec("codex", "#!/usr/bin/env node\n");
        let binary = bin.join("codex");

        let stripped = Command::new(&binary)
            .env("PATH", "/token-refill-missing")
            .output()
            .expect("spawn");
        assert_eq!(stripped.status.code(), Some(127));

        let prepared =
            child_path(&binary, Some(OsString::from("/token-refill-missing"))).expect("joined");
        let output = Command::new(&binary)
            .env("PATH", &prepared)
            .output()
            .expect("spawn");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "fake-node\n");

        fs::remove_dir_all(&root).ok();
    }
}
