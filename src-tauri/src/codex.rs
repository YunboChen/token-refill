//! Codex 订阅用量读取：通过本机 `codex app-server` 的 stdio JSON-RPC 协议查询额度。
//!
//! 不读取、不复制 Codex 登录凭证；只启动官方 app-server 子进程发起只读查询，
//! 完成后立即退出。CLI 缺失或未登录时返回结构化错误，由界面提示。
//! 协议结构来自 `codex app-server generate-json-schema`（CLI 0.163.0）。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsage {
    /// 本次成功获取数据的时间（Unix 秒）。
    pub checked_at_epoch: i64,
    pub buckets: Vec<CodexLimitBucket>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLimitBucket {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub plan_type: Option<String>,
    pub windows: Vec<CodexWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexWindow {
    /// 协议中窗口的角色字段名，如 primary / secondary。
    pub role: String,
    /// 协议口径：已用百分比（schema 字段 usedPercent）。
    pub used_percent: i64,
    /// 派生展示值：max(0, 100 - used_percent)，超限时为 0。
    pub remaining_percent: i64,
    pub window_duration_mins: Option<i64>,
    /// 协议给的窗口重置时间（Unix 秒）；缺失时为 None，不自行推算。
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageError {
    /// 稳定错误码：codex-not-found / server-exit / timeout / rpc / decode / io
    pub code: String,
    pub message: String,
}

impl UsageError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

fn io_error(err: impl std::fmt::Display) -> UsageError {
    UsageError::new("io", format!("启动 Codex app-server 失败：{err}"))
}

// ---------- 协议响应结构（仅声明本功能用到的字段，其余忽略） ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsResponse {
    rate_limits: LimitSnapshot,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, LimitSnapshot>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LimitSnapshot {
    #[serde(default)]
    limit_id: Option<String>,
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    primary: Option<WindowPayload>,
    #[serde(default)]
    secondary: Option<WindowPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowPayload {
    used_percent: i64,
    #[serde(default)]
    window_duration_mins: Option<i64>,
    #[serde(default)]
    resets_at: Option<i64>,
}

fn snapshot_to_bucket(snapshot: LimitSnapshot) -> CodexLimitBucket {
    let mut windows = Vec::new();
    for (role, window) in [
        ("primary", snapshot.primary),
        ("secondary", snapshot.secondary),
    ] {
        if let Some(window) = window {
            windows.push(CodexWindow {
                role: role.to_string(),
                used_percent: window.used_percent,
                remaining_percent: (100 - window.used_percent).max(0),
                window_duration_mins: window.window_duration_mins,
                resets_at_epoch: window.resets_at,
            });
        }
    }
    CodexLimitBucket {
        limit_id: snapshot.limit_id,
        limit_name: snapshot.limit_name,
        plan_type: snapshot.plan_type,
        windows,
    }
}

fn parse_rate_limits(
    value: serde_json::Value,
    checked_at_epoch: i64,
) -> Result<CodexUsage, UsageError> {
    let response: RateLimitsResponse = serde_json::from_value(value)
        .map_err(|err| UsageError::new("decode", format!("额度响应解析失败：{err}")))?;

    // 优先使用多窗口视图；缺失时回退到向后兼容的单窗口视图。
    let buckets = match response.rate_limits_by_limit_id {
        Some(map) if !map.is_empty() => map
            .into_values()
            .map(snapshot_to_bucket)
            .collect::<Vec<_>>(),
        _ => vec![snapshot_to_bucket(response.rate_limits)],
    };

    Ok(CodexUsage {
        checked_at_epoch,
        buckets,
    })
}

// ---------- Codex CLI 查找 ----------

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn find_in_nvm(home: &Path) -> Option<PathBuf> {
    let versions = home.join(".nvm").join("versions").join("node");
    let mut found: Vec<PathBuf> = fs::read_dir(&versions)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("bin").join("codex"))
        .filter(|candidate| is_executable(candidate))
        .collect();
    // 版本目录名可排序（v26.12.0 > v20.0.0 按字符串降序基本成立），取最新。
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().next()
}

pub fn find_codex_binary() -> Result<PathBuf, UsageError> {
    if let Ok(explicit) = std::env::var("CODEX_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable(&path) {
            return Ok(path);
        }
        return Err(UsageError::new(
            "codex-not-found",
            format!("CODEX_BIN 指向的文件不可执行：{}", path.display()),
        ));
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join("codex");
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }

    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = &home {
        if let Some(candidate) = find_in_nvm(home) {
            return Ok(candidate);
        }
    }
    let mut candidates = vec![
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
    ];
    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/codex"));
    }
    for candidate in candidates {
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }

    Err(UsageError::new(
        "codex-not-found",
        "未找到 codex 可执行文件。请安装 Codex CLI，或设置 CODEX_BIN 指向其路径。",
    ))
}

// ---------- app-server 子进程与 JSON-RPC ----------

struct AppServer {
    child: Child,
    stdin: std::process::ChildStdin,
    incoming: mpsc::Receiver<serde_json::Value>,
}

impl AppServer {
    fn spawn(binary: &Path) -> Result<Self, UsageError> {
        let mut child = Command::new(binary)
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(io_error)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io_error("无法写入 stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io_error("无法读取 stdout"))?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                    if tx.send(value).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            incoming: rx,
        })
    }

    fn send(&mut self, message: serde_json::Value) -> Result<(), UsageError> {
        let mut line = serde_json::to_string(&message).map_err(io_error)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).map_err(io_error)?;
        self.stdin.flush().map_err(io_error)
    }

    /// 读取消息直到收到指定 id 的响应；通知与其他响应被忽略。
    fn await_response(
        &mut self,
        id: i64,
        timeout: Duration,
    ) -> Result<serde_json::Value, UsageError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(UsageError::new(
                    "timeout",
                    "Codex app-server 响应超时，请稍后重试。",
                ));
            }
            let message = match self.incoming.recv_timeout(remaining) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(UsageError::new(
                        "timeout",
                        "Codex app-server 响应超时，请稍后重试。",
                    ))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = self.child.wait().ok();
                    let code = status.and_then(|s| s.code()).unwrap_or(-1);
                    return Err(UsageError::new(
                        "server-exit",
                        format!("Codex app-server 意外退出（退出码 {code}），请确认 CLI 可用。"),
                    ));
                }
            };
            let Some(message_id) = message.get("id").and_then(|id| id.as_i64()) else {
                continue;
            };
            if message_id != id {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("未知错误");
                return Err(UsageError::new("rpc", format!("Codex 查询失败：{text}")));
            }
            return Ok(message
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null));
        }
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn read_usage() -> Result<CodexUsage, UsageError> {
    let binary = find_codex_binary()?;
    read_usage_from(&binary)
}

fn read_usage_from(binary: &Path) -> Result<CodexUsage, UsageError> {
    let mut server = AppServer::spawn(binary)?;

    server.send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "token-refill",
                "title": "Token Refill",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    }))?;
    server.await_response(1, INITIALIZE_TIMEOUT)?;

    server.send(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized"
    }))?;

    let result = {
        server.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "account/rateLimits/read",
            "params": { "excludeResetCreditDetails": true }
        }))?;
        server.await_response(2, READ_TIMEOUT)?
    };

    let checked_at_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    parse_rate_limits(result, checked_at_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "checkedAtEpochIgnored": true,
            "accountId": "acct-synthetic",
            "rateLimits": {
                "limitId": "codex",
                "limitName": null,
                "planType": "plus",
                "primary": { "usedPercent": 4, "windowDurationMins": 300, "resetsAt": 1790000000 },
                "secondary": { "usedPercent": 13, "windowDurationMins": 10080, "resetsAt": 1790999999 }
            },
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId": "codex",
                    "limitName": null,
                    "planType": "plus",
                    "primary": { "usedPercent": 4, "windowDurationMins": 300, "resetsAt": 1790000000 },
                    "secondary": { "usedPercent": 13, "windowDurationMins": 10080, "resetsAt": 1790999999 }
                }
            }
        })
    }

    #[test]
    fn rate_limits_parse_into_buckets_and_remaining_windows() {
        let usage = parse_rate_limits(fixture(), 1790000000).expect("fixture parses");

        assert_eq!(usage.checked_at_epoch, 1790000000);
        assert_eq!(usage.buckets.len(), 1);
        let bucket = &usage.buckets[0];
        assert_eq!(bucket.limit_id.as_deref(), Some("codex"));
        assert_eq!(bucket.plan_type.as_deref(), Some("plus"));
        assert_eq!(bucket.windows.len(), 2);
        assert_eq!(bucket.windows[0].role, "primary");
        assert_eq!(bucket.windows[0].used_percent, 4);
        assert_eq!(bucket.windows[0].remaining_percent, 96);
        assert_eq!(bucket.windows[0].window_duration_mins, Some(300));
        assert_eq!(bucket.windows[1].remaining_percent, 87);
    }

    #[test]
    fn single_bucket_view_used_when_multi_view_absent() {
        let mut value = fixture();
        value["rateLimitsByLimitId"] = serde_json::Value::Null;
        let usage = parse_rate_limits(value, 0).expect("parses");
        assert_eq!(usage.buckets.len(), 1);
    }

    #[test]
    fn missing_optional_window_fields_stay_unknown() {
        let value = serde_json::json!({
            "rateLimits": {
                "planType": "plus",
                "primary": { "usedPercent": 200 }
            }
        });
        let usage = parse_rate_limits(value, 0).expect("parses");
        let window = &usage.buckets[0].windows[0];
        assert_eq!(window.used_percent, 200);
        assert_eq!(window.remaining_percent, 0);
        assert_eq!(window.window_duration_mins, None);
        assert_eq!(window.resets_at_epoch, None);
        assert_eq!(usage.buckets[0].limit_id, None);
    }

    #[test]
    fn unknown_shape_reports_decode_error_not_panic() {
        let err = parse_rate_limits(serde_json::json!({ "unexpected": true }), 0)
            .expect_err("should fail");
        assert_eq!(err.code, "decode");
    }

    #[test]
    fn read_usage_drives_fake_app_server_over_stdio() {
        use std::os::unix::fs::PermissionsExt;

        // 合成数据脚本模拟 `codex app-server --stdio`：验证进程管理、
        // 行分隔 JSON-RPC、通知跳过与响应解析全链路，不使用真实账户。
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "rateLimits": fixture()["rateLimits"],
                "rateLimitsByLimitId": fixture()["rateLimitsByLimitId"]
            }
        })
        .to_string();
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json
RESPONSE = {response:?}
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    if msg.get("method") == "initialize":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": {{"userAgent": "fake"}}}}), flush=True)
        print(json.dumps({{"jsonrpc": "2.0", "method": "account/rateLimits/updated", "params": {{}}}}), flush=True)
    elif msg.get("method") == "account/rateLimits/read":
        print(RESPONSE, flush=True)
"#
        );
        let path =
            std::env::temp_dir().join(format!("token-refill-fake-codex-{}.py", std::process::id()));
        fs::write(&path, script).expect("write fake server");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake server");

        let usage = read_usage_from(&path).expect("fake server responds");
        fs::remove_file(&path).ok();

        assert!(usage.checked_at_epoch > 0);
        assert_eq!(usage.buckets.len(), 1);
        assert_eq!(usage.buckets[0].windows.len(), 2);
        assert_eq!(usage.buckets[0].windows[0].remaining_percent, 96);
        assert_eq!(usage.buckets[0].windows[1].remaining_percent, 87);
    }

    #[test]
    fn explicit_codex_bin_must_be_executable() {
        // 仅验证纯逻辑：不可执行路径返回稳定错误码，不启动进程。
        // 环境变量隔离测试：使用不存在的路径。
        let previous = std::env::var_os("CODEX_BIN");
        std::env::set_var("CODEX_BIN", "/nonexistent/codex-binary-for-test");
        let result = find_codex_binary();
        match previous {
            Some(value) => std::env::set_var("CODEX_BIN", value),
            None => std::env::remove_var("CODEX_BIN"),
        }
        let err = result.expect_err("should fail");
        assert_eq!(err.code, "codex-not-found");
    }
}
