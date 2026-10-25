//! Google Antigravity 额度。
//!
//! 通过官方 CLI 的打印模式读取 `/usage`，不读取其钥匙串、令牌或浏览器会话：
//! `agy -p /usage --output-format json`。版本低于 1.1.11 时旧 CLI 会把
//! `/usage` 当成普通提示词并消耗额度，因此查询前先校验版本。
//!
//! 账户来自用户本机的 `agy` 登录态，与应用内 Codex 本机账户的定位一致。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use semver::Version;
use serde_json::Value;

use crate::quota::{now_epoch, parse_epoch};
use crate::usage_api::ProviderError;

const MIN_CLI_VERSION: (u64, u64, u64) = (1, 1, 11);
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityUsage {
    pub checked_at_epoch: i64,
    pub account: Option<String>,
    pub plan_tier: Option<String>,
    pub groups: Vec<AntigravityGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityGroup {
    pub name: String,
    pub windows: Vec<AntigravityWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityWindow {
    pub source_id: String,
    pub name: String,
    pub window_duration_mins: Option<i64>,
    pub remaining_percent: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

pub fn read_usage() -> Result<AntigravityUsage, ProviderError> {
    let binary = find_agy_binary()?;
    ensure_supported_version(&binary)?;
    let output = run(
        &binary,
        &["-p", "/usage", "--output-format", "json"],
        QUERY_TIMEOUT,
    )?;
    if output.auth_required {
        return Err(auth_error());
    }
    if output.timed_out {
        return Err(ProviderError::new(
            "timeout",
            "Antigravity 查询超时，请确认 agy 已登录且网络可用。",
        ));
    }
    if !output.success {
        return Err(classify_failure(&output.stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_usage(&text, now_epoch())
}

// ---------- CLI 查找与执行 ----------

#[cfg(windows)]
const BINARY_NAMES: &[&str] = &["agy.exe", "agy.cmd", "agy.bat", "agy.com"];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["agy"];

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

/// 查找顺序：AGY_BIN → PATH → 常见安装目录。
pub fn find_agy_binary() -> Result<PathBuf, ProviderError> {
    if let Ok(explicit) = std::env::var("AGY_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable(&path) {
            return Ok(path);
        }
        return Err(ProviderError::new(
            "cli-not-found",
            format!("AGY_BIN 指向的文件不可执行：{}", path.display()),
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
        dirs.push(PathBuf::from("/opt/agy/bin"));
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
        }
    }
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(base.join("agy").join("bin"));
        }
        if let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(base.join("npm"));
        }
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(home.join("scoop").join("shims"));
        }
    }
    dirs.iter().find_map(|dir| find_in_dir(dir)).ok_or_else(|| {
        ProviderError::new(
            "cli-not-found",
            "未找到 agy（Antigravity CLI）。请安装并登录后重试，或设置 AGY_BIN 指向其路径。",
        )
    })
}

struct ProcessOutput {
    success: bool,
    timed_out: bool,
    auth_required: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn contains_auth_marker(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    [
        "authentication required",
        "authentication failed",
        "not logged in",
        "not signed in",
        "please visit the url to log in",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn read_capped(reader: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = reader.take(MAX_OUTPUT_BYTES).read_to_end(&mut buffer);
    buffer
}

/// 在超时内运行 CLI。`agy` 未登录时会打印登录链接并等待 60 秒，
/// 因此边读 stderr 边检测认证提示，命中后立即结束进程，避免每轮刷新都空等。
fn run(binary: &Path, args: &[&str], timeout: Duration) -> Result<ProcessOutput, ProviderError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // 查询额度不应触发 CLI 自更新。
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    crate::cli::apply_child_path(&mut command, binary);
    crate::proxy::apply_to_command(&mut command);

    let mut child = command.spawn().map_err(|_| {
        ProviderError::new(
            "cli-not-found",
            "无法启动 agy，请确认 Antigravity CLI 可用。",
        )
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法读取 agy 输出。"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法读取 agy 错误输出。"))?;
    let stdout_handle = thread::spawn(move || read_capped(stdout));
    let (auth_tx, auth_rx) = std::sync::mpsc::channel();
    let stderr_handle = thread::spawn(move || {
        let mut reader = stderr;
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    buffer.extend_from_slice(&chunk[..read]);
                    if buffer.len() as u64 >= MAX_OUTPUT_BYTES {
                        break;
                    }
                    if contains_auth_marker(&buffer) {
                        let _ = auth_tx.send(());
                    }
                }
                Err(_) => break,
            }
        }
        buffer
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut auth_required = false;
    let status = loop {
        if !auth_required && auth_rx.try_recv().is_ok() {
            auth_required = true;
            let _ = child.kill();
            break child.wait().ok();
        }
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
                return Err(ProviderError::new("io", "等待 agy 退出失败。"));
            }
        }
    };

    Ok(ProcessOutput {
        success: status.map(|status| status.success()).unwrap_or(false),
        timed_out,
        auth_required,
        stdout: stdout_handle.join().unwrap_or_default(),
        stderr: stderr_handle.join().unwrap_or_default(),
    })
}

fn parse_version(text: &str) -> Option<Version> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .map(|token| {
            token
                .trim_matches(|c: char| {
                    !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+'
                })
                .trim_start_matches('v')
        })
        .find_map(|token| Version::parse(token).ok())
}

fn ensure_supported_version(binary: &Path) -> Result<(), ProviderError> {
    let output = run(binary, &["--version"], VERSION_TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = parse_version(&stdout).or_else(|| parse_version(&stderr));

    let minimum = Version::new(MIN_CLI_VERSION.0, MIN_CLI_VERSION.1, MIN_CLI_VERSION.2);
    match version {
        Some(version) if version >= minimum => Ok(()),
        Some(version) => Err(ProviderError::new(
            "cli-outdated",
            format!("Antigravity CLI 版本过低（{version}），请升级到 1.1.11 或更高版本后再查询。"),
        )),
        None => Err(ProviderError::new(
            "cli-outdated",
            "无法确认 Antigravity CLI 版本，请升级到 1.1.11 或更高版本后重试。",
        )),
    }
}

fn auth_error() -> ProviderError {
    ProviderError::new(
        "auth-expired",
        "Antigravity 尚未登录，请先在终端运行 agy 完成登录后重试。",
    )
}

fn classify_failure(stderr: &[u8]) -> ProviderError {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if contains_auth_marker(stderr) {
        return auth_error();
    }
    if text.contains("unknown option") || text.contains("unrecognized") {
        return ProviderError::new(
            "cli-outdated",
            "当前 agy 不支持打印模式查询额度，请升级到 1.1.11 或更高版本。",
        );
    }
    ProviderError::new(
        "cli-failed",
        "Antigravity CLI 查询失败，请确认 agy 已登录且网络可用。",
    )
}

// ---------- JSON 解析 ----------

fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

fn parse_usage(text: &str, checked_at_epoch: i64) -> Result<AntigravityUsage, ProviderError> {
    let root: Value = serde_json::from_str(extract_json(text))
        .map_err(|_| ProviderError::new("decode", "Antigravity 返回的额度数据无法识别。"))?;

    if let Some(status) = root.get("status").and_then(Value::as_str) {
        if !status.eq_ignore_ascii_case("success") && !status.eq_ignore_ascii_case("ok") {
            if root
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|error| contains_auth_marker(error.as_bytes()))
            {
                return Err(auth_error());
            }
            return Err(ProviderError::new(
                "cli-failed",
                "Antigravity 查询未成功，请确认 agy 已登录。",
            ));
        }
    }

    let data = root
        .pointer("/command/data")
        .or_else(|| root.get("data"))
        .unwrap_or(&root);
    let groups_value = data
        .get("groups")
        .or_else(|| root.get("groups"))
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::new("decode", "Antigravity 未返回额度分组。"))?;

    let mut groups = Vec::new();
    for (group_index, group) in groups_value.iter().enumerate() {
        let name = text_field(group, "display_name")
            .or_else(|| text_field(group, "displayName"))
            .or_else(|| text_field(group, "name"))
            .unwrap_or_else(|| format!("额度分组 {}", group_index + 1));
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };

        let mut windows = Vec::new();
        for (index, bucket) in buckets.iter().enumerate() {
            if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let window = text_field(bucket, "window");
            let source_id = text_field(bucket, "bucket_id")
                .or_else(|| text_field(bucket, "bucketId"))
                .or_else(|| text_field(bucket, "id"))
                .unwrap_or_else(|| format!("{group_index}-{index}"));
            let window_name = text_field(bucket, "display_name")
                .or_else(|| text_field(bucket, "displayName"))
                .or_else(|| text_field(bucket, "name"))
                .unwrap_or_else(|| window_label(window.as_deref()));
            let fraction = fraction(bucket.get("remaining_fraction"))
                .or_else(|| fraction(bucket.get("remainingFraction")))
                .or_else(|| {
                    bucket
                        .get("remaining")
                        .and_then(|remaining| fraction(remaining.get("remaining_fraction")))
                });
            let remaining_percent =
                fraction.map(|value| (value * 100.0).round().clamp(0.0, 100.0) as i64);
            let resets_at_epoch = bucket
                .get("reset_time")
                .or_else(|| bucket.get("resetTime"))
                .and_then(parse_epoch);

            windows.push(AntigravityWindow {
                source_id,
                name: window_name,
                window_duration_mins: window_duration(window.as_deref()),
                remaining_percent,
                resets_at_epoch,
            });
        }

        if !windows.is_empty() {
            groups.push(AntigravityGroup { name, windows });
        }
    }

    if groups.is_empty() {
        return Err(ProviderError::new(
            "decode",
            "Antigravity 未返回可识别的额度窗口。",
        ));
    }

    Ok(AntigravityUsage {
        checked_at_epoch,
        account: text_field(data, "account").or_else(|| text_field(&root, "account")),
        plan_tier: text_field(data, "plan")
            .or_else(|| text_field(data, "tier"))
            .or_else(|| text_field(&root, "plan")),
        groups,
    })
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn fraction(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn window_duration(window: Option<&str>) -> Option<i64> {
    match window.map(|value| value.to_ascii_lowercase()) {
        Some(value) if value.contains("five") || value == "5h" => Some(5 * 60),
        Some(value) if value.contains("week") => Some(7 * 24 * 60),
        _ => None,
    }
}

fn window_label(window: Option<&str>) -> String {
    match window_duration(window) {
        Some(300) => "5 小时窗口".to_string(),
        Some(10080) => "1 周窗口".to_string(),
        _ => "额度窗口".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_print_mode_payload() {
        let payload = r#"{
            "status": "SUCCESS",
            "command": {
                "name": "usage",
                "data": {
                    "account": "user@example.com",
                    "tier": "Pro",
                    "groups": [
                        {
                            "display_name": "Gemini Models",
                            "buckets": [
                                { "bucket_id": "five_hour", "display_name": "Five Hour Limit", "window": "five_hour", "remaining_fraction": 0.75, "reset_time": "2026-10-23T12:00:00Z" },
                                { "bucket_id": "weekly", "display_name": "Weekly Limit", "window": "weekly", "remaining_fraction": 0.5, "reset_time": "2026-10-29T00:00:00Z" }
                            ]
                        }
                    ]
                }
            }
        }"#;

        let usage = parse_usage(payload, 1).unwrap();
        assert_eq!(usage.account.as_deref(), Some("user@example.com"));
        assert_eq!(usage.plan_tier.as_deref(), Some("Pro"));
        assert_eq!(usage.groups.len(), 1);
        let windows = &usage.groups[0].windows;
        assert_eq!(windows[0].remaining_percent, Some(75));
        assert_eq!(windows[0].window_duration_mins, Some(300));
        assert_eq!(windows[1].remaining_percent, Some(50));
        assert_eq!(windows[1].window_duration_mins, Some(10080));
    }

    #[test]
    fn accepts_root_groups_and_snake_case_timestamps() {
        let payload = r#"{
            "groups": [
                { "name": "Claude and GPT models", "buckets": [
                    { "id": "weekly", "window": "weekly", "remaining_fraction": "0.25", "reset_time": 1780000000 }
                ] }
            ]
        }"#;

        let usage = parse_usage(payload, 1).unwrap();
        assert_eq!(usage.groups[0].name, "Claude and GPT models");
        assert_eq!(usage.groups[0].windows[0].remaining_percent, Some(25));
        assert_eq!(
            usage.groups[0].windows[0].resets_at_epoch,
            Some(1_780_000_000)
        );
    }

    #[test]
    fn failure_status_is_reported() {
        let error =
            parse_usage(r#"{"status":"ERROR","error":"authentication required"}"#, 1).unwrap_err();
        assert_eq!(error.code, "auth-expired");

        let error =
            parse_usage(r#"{"status":"ERROR","error":"quota endpoint changed"}"#, 1).unwrap_err();
        assert_eq!(error.code, "cli-failed");
    }

    #[test]
    fn parses_versions_from_cli_banner() {
        assert_eq!(
            parse_version("Antigravity CLI v1.2.6 (build 42)"),
            Some(Version::new(1, 2, 6))
        );
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn recognizes_the_cli_login_prompt() {
        // 实际 `agy -p /usage` 未登录时写入 stderr 的首行。
        let stderr = b"Authentication required. Please visit the URL to log in:\n";
        assert!(contains_auth_marker(stderr));
        assert_eq!(classify_failure(stderr).code, "auth-expired");
    }
}
