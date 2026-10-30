//! 最小的本地生命周期日志。
//!
//! 只记录窗口与托盘的创建、加载、显示、销毁与失败，用于排查只在 Windows
//! 实机出现的问题：发布包是 GUI 子系统，没有控制台，`eprintln!` 的输出会被
//! 系统直接丢弃，事后无法确认“点了托盘有没有触发事件”。
//!
//! 文件写在 `<app_data_dir>/logs/window.log`，超过上限时轮转为 `window.log.1`。
//! 这里只写固定文案、窗口标签与耗时，不写令牌、Cookie、邮箱、密钥或请求内容。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use time::OffsetDateTime;

/// 单个日志文件的大小上限；超过后重命名保留一份，避免长期驻留写满磁盘。
const MAX_LOG_BYTES: u64 = 256 * 1024;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 初始化日志目录；目录不可写时只保留 stderr 输出，不影响应用启动。
pub fn init(data_dir: &Path) {
    let dir = data_dir.join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = LOG_PATH.set(dir.join("window.log"));
}

/// 写一行生命周期日志：同时输出到 stderr（开发时可见）与日志文件。
pub fn write(message: &str) {
    eprintln!("{message}");

    let Some(path) = LOG_PATH.get() else {
        return;
    };
    rotate_if_needed(path);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "{} {message}", timestamp());
}

fn rotate_if_needed(path: &Path) {
    let too_big = fs::metadata(path)
        .map(|meta| meta.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if too_big {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
}

/// UTC 时间戳；不依赖本地时区，避免驻留期间时区设置变化带来的歧义。
fn timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

macro_rules! lifecycle {
    ($($arg:tt)*) => {
        $crate::logging::write(&format!($($arg)*))
    };
}
pub(crate) use lifecycle;

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn timestamp_is_a_compact_utc_marker() {
        let value = timestamp();
        assert_eq!(value.len(), 20, "时间戳应是 20 个字符：{value}");
        assert!(value.ends_with('Z'), "时间戳应带 UTC 后缀：{value}");
        assert_eq!(
            value.as_bytes()[10],
            b'T',
            "日期与时间之间用 T 分隔：{value}"
        );
    }
}
