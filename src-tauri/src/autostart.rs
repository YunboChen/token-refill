//! 开机自启的静默启动标记。
//!
//! 登录项注册时带上 [`START_ARG`]：由系统登录项拉起的进程只驻留菜单栏，
//! 不显示主窗口；用户从 Finder、开始菜单或命令行手动启动时没有该参数，
//! 启动行为保持不变。

/// 系统登录项传给应用的启动参数。
pub const START_ARG: &str = "--autostart";

/// 本次进程是否由系统登录项启动。
pub fn started_from_login() -> bool {
    args_contain_start_arg(std::env::args())
}

/// 已注册的登录项是否带上了静默参数。
///
/// 旧版本只注册可执行文件、没有参数，升级后第一次登录仍会弹出主窗口。
/// 启动时只在缺参数的情况下改写登录项，避免每次启动都重写系统注册
/// （Windows 上会顺带重置任务管理器里对该启动项的禁用状态）。
#[cfg(target_os = "macos")]
pub fn registration_is_silent(app_name: &str) -> bool {
    macos::registration_has_start_arg(app_name)
}

#[cfg(windows)]
pub fn registration_is_silent(app_name: &str) -> bool {
    windows::registration_has_start_arg(app_name)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn registration_is_silent(_app_name: &str) -> bool {
    false
}

fn args_contain_start_arg<I>(args: I) -> bool
where
    I: IntoIterator<Item = String>,
{
    // 第一个参数是可执行文件路径，不参与匹配。
    args.into_iter().skip(1).any(|arg| arg == START_ARG)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use super::START_ARG;

    /// `tauri-plugin-autostart` 的 LaunchAgent 定义：
    /// `~/Library/LaunchAgents/<app_name>.plist`，参数写在 `ProgramArguments` 里。
    pub fn registration_has_start_arg(app_name: &str) -> bool {
        let Some(home) = std::env::var_os("HOME") else {
            return false;
        };
        let path = PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{app_name}.plist"));
        std::fs::read_to_string(path).is_ok_and(|plist| plist.contains(START_ARG))
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };

    use super::START_ARG;

    const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";

    /// 登录项由 `tauri-plugin-autostart` 写到 `Run` 键，值名即应用名。
    /// 管理员运行与非管理员运行可能分别落在 HKLM / HKCU，两处都要看。
    pub fn registration_has_start_arg(app_name: &str) -> bool {
        read_run_value(HKEY_CURRENT_USER, app_name)
            .or_else(|| read_run_value(HKEY_LOCAL_MACHINE, app_name))
            .is_some_and(|command| command.contains(START_ARG))
    }

    fn read_run_value(root: HKEY, app_name: &str) -> Option<String> {
        let key = wide(RUN_KEY);
        let value = wide(app_name);
        let mut kind = 0u32;
        let mut size = 0u32;

        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != 0 || size < 2 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }

        let units: Vec<u16> = buffer[..size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn matches_start_arg_from_login_item() {
        assert!(args_contain_start_arg(args(&[
            "/Applications/Token Refill.app/Contents/MacOS/token-refill",
            START_ARG
        ])));
        assert!(args_contain_start_arg(args(&[
            "app.exe", "--other", START_ARG
        ])));
    }

    #[test]
    fn ignores_executable_and_other_args() {
        assert!(!args_contain_start_arg(args(&["token-refill"])));
        assert!(!args_contain_start_arg(args(&["token-refill", "--other"])));
        // 只有可执行文件名恰好相同也不应误判为登录项启动。
        assert!(!args_contain_start_arg(args(&[START_ARG])));
        // 前缀相同的其他参数不算。
        assert!(!args_contain_start_arg(args(&[
            "token-refill",
            "--autostart=1"
        ])));
    }
}
