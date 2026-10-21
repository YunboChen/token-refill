use std::path::{Path, PathBuf};

/// tauri-build 只对 tauri.conf.json 发变更声明，图标与分平台配置都要我们自己声明，
/// 否则换图标、改 tauri.{平台}.conf.json 后 `tauri dev` 会继续跑旧二进制里烙进去的图标。
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.file_name().is_some_and(|name| name != ".DS_Store") {
            out.push(path);
        }
    }
}

fn main() {
    let mut watched = Vec::new();
    collect(Path::new("icons"), &mut watched);

    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("tauri.") && name.ends_with(".conf.json") {
                watched.push(path);
            }
        }
    }

    for path in watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    tauri_build::build();
}
