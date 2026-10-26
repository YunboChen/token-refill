//! OpenCode Go 订阅额度。
//!
//! 官方 usage API：`GET https://opencode.ai/zen/go/v1/usage`，用 Go API Key
//! 作为 Bearer 令牌。返回 rolling（5 小时）/ weekly / monthly 三个窗口的
//! 已用百分比与重置时间；百分比已经是账户维度的汇总值，不需要再按模型累加。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::quota::{now_epoch, parse_epoch};
use crate::usage_api::{get_json, ProviderError};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CLI_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

/// 窗口定义：API 字段、展示名与固定周期。订阅月按购买日滚动，不写死天数。
const WINDOWS: [(&str, &str, Option<i64>); 3] = [
    ("rolling", "5 小时窗口", Some(5 * 60)),
    ("weekly", "1 周窗口", Some(7 * 24 * 60)),
    ("monthly", "订阅月", None),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpencodeUsage {
    pub checked_at_epoch: i64,
    pub windows: Vec<OpencodeWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpencodeWindow {
    pub source_id: &'static str,
    pub name: &'static str,
    pub window_duration_mins: Option<i64>,
    pub used_percent: i64,
    pub resets_at_epoch: Option<i64>,
}

pub fn read_usage(api_key: &str) -> Result<OpencodeUsage, ProviderError> {
    let payload =
        get_json(USAGE_URL, api_key, REQUEST_TIMEOUT).map_err(|error| match error.status {
            Some(401) => ProviderError::new(
                "auth-expired",
                "OpenCode Go API Key 无效或已失效，请重新填写。",
            ),
            Some(403) => {
                ProviderError::new("not-subscribed", "该 API Key 没有开通 OpenCode Go 订阅。")
            }
            Some(429) => ProviderError::new("rate-limited", "OpenCode 查询过于频繁，请稍后重试。"),
            Some(status) if status >= 500 => {
                ProviderError::new("upstream", "OpenCode 服务暂时不可用，请稍后重试。")
            }
            Some(_) => ProviderError::new("http", error.message),
            None => ProviderError::new("network", error.message),
        })?;

    parse_usage(&payload, now_epoch())
}

fn parse_usage(payload: &Value, checked_at_epoch: i64) -> Result<OpencodeUsage, ProviderError> {
    let usage = payload
        .get("usage")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::new("decode", "OpenCode 未返回额度数据。"))?;

    let mut windows = Vec::new();
    for (source_id, name, duration) in WINDOWS {
        let Some(window) = usage.get(source_id) else {
            continue;
        };
        // `rate-limited` 表示窗口已打满；百分比字段本身可能仍停在旧值。
        let rate_limited = window
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("rate-limited"));
        let percent = window
            .get("percent")
            .and_then(Value::as_f64)
            .map(|percent| percent.round().clamp(0.0, 100.0) as i64)
            .unwrap_or(0);
        windows.push(OpencodeWindow {
            source_id,
            name,
            window_duration_mins: duration,
            used_percent: if rate_limited { 100 } else { percent },
            resets_at_epoch: window.get("resetsAt").and_then(parse_epoch),
        });
    }

    if windows.is_empty() {
        return Err(ProviderError::new(
            "decode",
            "OpenCode 未返回可识别的额度窗口。",
        ));
    }

    Ok(OpencodeUsage {
        checked_at_epoch,
        windows,
    })
}

// ---------- 本机 CLI 凭据 ----------
//
// OpenCode 2 把凭据保存在数据库里，但官方 CLI 提供 `auth export` 导出接口；
// 应用只调用该命令读取当前活动凭据，不直接解析 SQLite、钥匙串或浏览器 Cookie。

#[cfg(windows)]
const BINARY_NAMES: &[&str] = &[
    "opencode.exe",
    "opencode.cmd",
    "opencode.bat",
    "opencode.com",
];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["opencode"];

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    let is_file = std::fs::metadata(path)
        .map(|meta| meta.is_file())
        .unwrap_or(false);
    is_file
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "exe" | "cmd" | "bat" | "com"
                )
            })
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const KEYS: [&str; 2] = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const KEYS: [&str; 2] = ["HOME", "USERPROFILE"];
    KEYS.iter()
        .find_map(|key| std::env::var_os(key).map(PathBuf::from))
}

fn find_in_dir(dir: &Path) -> Option<PathBuf> {
    BINARY_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

pub fn find_opencode_binary() -> Result<PathBuf, ProviderError> {
    if let Ok(explicit) = std::env::var("OPENCODE_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable(&path) {
            return Ok(path);
        }
        return Err(ProviderError::new(
            "cli-not-found",
            format!("OPENCODE_BIN 指向的文件不可执行：{}", path.display()),
        ));
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            if let Some(candidate) = find_in_dir(&dir) {
                return Ok(candidate);
            }
        }
    }

    let mut dirs = Vec::new();
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
        }
    }
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(base.join("npm"));
        }
        if let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(base.join("pnpm"));
        }
        if let Some(home) = home_dir() {
            dirs.push(home.join(".bun").join("bin"));
            dirs.push(home.join("scoop").join("shims"));
        }
    }
    dirs.iter().find_map(|dir| find_in_dir(dir)).ok_or_else(|| {
        ProviderError::new(
            "cli-not-found",
            "未找到 opencode 可执行文件。请安装 OpenCode CLI 或填写 API Key。",
        )
    })
}

struct ProcessOutput {
    success: bool,
    timed_out: bool,
    stdout: Vec<u8>,
}

fn read_capped(reader: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = reader.take(MAX_OUTPUT_BYTES).read_to_end(&mut buffer);
    buffer
}

fn run_cli(binary: &Path, args: &[&str]) -> Result<ProcessOutput, ProviderError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::cli::apply_child_path(&mut command, binary);
    crate::cli::apply_no_window(&mut command);
    crate::proxy::apply_to_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|_| ProviderError::new("cli-not-found", "无法启动 opencode，请确认 CLI 可用。"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法读取 opencode 输出。"))?;
    let stdout_handle = thread::spawn(move || read_capped(stdout));

    let deadline = Instant::now() + CLI_TIMEOUT;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProviderError::new("io", "等待 opencode 退出失败。"));
            }
        }
    };

    Ok(ProcessOutput {
        success: status.map(|status| status.success()).unwrap_or(false),
        timed_out,
        stdout: stdout_handle.join().unwrap_or_default(),
    })
}

/// 读取本机 OpenCode 当前可用的 Go API Key：优先官方 `auth export`，
/// 其次环境变量 `OPENCODE_GO_API_KEY`。密钥只留在内存。
pub fn read_cli_api_key() -> Result<String, ProviderError> {
    let cli_result = find_opencode_binary().and_then(|binary| {
        let output = run_cli(&binary, &["auth", "export"])?;
        if output.timed_out {
            return Err(ProviderError::new("timeout", "读取 OpenCode 凭据超时。"));
        }
        if !output.success {
            return Err(ProviderError::new(
                "auth-expired",
                "无法读取本机 OpenCode 凭据，请先在 CLI 中登录。",
            ));
        }
        let payload: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| ProviderError::new("decode", "OpenCode auth export 返回格式无法识别。"))?;
        parse_cli_export(&payload).ok_or_else(|| {
            ProviderError::new(
                "auth-expired",
                "本机 OpenCode 未找到可用的 Go API Key，请先登录或填写 API Key。",
            )
        })
    });

    match cli_result {
        Ok(key) => Ok(key),
        Err(error) => std::env::var("OPENCODE_GO_API_KEY")
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .ok_or(error),
    }
}

/// 从 `opencode auth export` 的数组中选活动凭据。
/// `opencode-go` 优先于通用的 `opencode`；只接受 `type = key`。
fn parse_cli_export(payload: &Value) -> Option<String> {
    let entries = payload.as_array()?;
    let mut best: Option<(u8, String)> = None;
    for entry in entries {
        if entry.get("active").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let integration = entry
            .get("integrationID")
            .or_else(|| entry.get("integrationId"))
            .and_then(Value::as_str)?;
        let rank = match integration {
            "opencode-go" => 0,
            "opencode" => 1,
            _ => continue,
        };
        let value = entry.get("value")?;
        if value.get("type").and_then(Value::as_str) != Some("key") {
            continue;
        }
        let key = value.get("key").and_then(Value::as_str)?.trim();
        if key.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
            best = Some((rank, key.to_string()));
        }
    }
    best.map(|(_, key)| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_windows_and_reset_times() {
        let payload = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok", "percent": 12, "resetsAt": "2026-10-11T13:01:09.000Z" },
                "weekly": { "status": "ok", "percent": 46, "resetsAt": "2026-10-12T00:00:00.000Z" },
                "monthly": { "status": "ok", "percent": 82, "resetsAt": "2026-10-14T21:34:50.000Z" }
            }
        });

        let usage = parse_usage(&payload, 1_790_000_000).unwrap();
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].used_percent, 12);
        assert_eq!(usage.windows[0].window_duration_mins, Some(300));
        assert_eq!(usage.windows[1].used_percent, 46);
        assert_eq!(usage.windows[2].window_duration_mins, None);
        assert_eq!(usage.windows[2].resets_at_epoch, Some(1_792_013_690));
    }

    #[test]
    fn rate_limited_pins_window_to_full() {
        let payload = serde_json::json!({
            "usage": {
                "rolling": { "status": "rate-limited", "percent": 37, "resetsAt": "2026-10-11T13:01:09.000Z" }
            }
        });

        let usage = parse_usage(&payload, 1).unwrap();
        assert_eq!(usage.windows[0].used_percent, 100);
    }

    #[test]
    fn empty_usage_is_a_decode_error() {
        let error = parse_usage(&serde_json::json!({ "usage": {} }), 1).unwrap_err();
        assert_eq!(error.code, "decode");
    }

    #[test]
    fn cli_export_prefers_the_active_go_integration() {
        let payload = serde_json::json!([
            {
                "integrationID": "opencode",
                "active": true,
                "value": { "type": "key", "key": "oc_sk_console" }
            },
            {
                "integrationID": "opencode-go",
                "active": true,
                "value": { "type": "key", "key": "sk_go" }
            },
            {
                "integrationID": "opencode-go",
                "active": false,
                "value": { "type": "key", "key": "sk_inactive" }
            }
        ]);

        assert_eq!(parse_cli_export(&payload).as_deref(), Some("sk_go"));
    }

    #[test]
    fn cli_export_ignores_non_key_entries_and_missing_payloads() {
        let payload = serde_json::json!([
            {
                "integrationID": "opencode",
                "active": true,
                "value": { "type": "oauth", "key": "not-a-key" }
            }
        ]);
        assert!(parse_cli_export(&payload).is_none());
        assert!(parse_cli_export(&serde_json::json!({})).is_none());
    }
}
