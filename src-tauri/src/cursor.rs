//! Cursor 个人订阅额度。
//!
//! 每个账户使用独立的 `HOME` 启动官方 Cursor Agent CLI（`agent login`），并强制
//! `AGENT_CLI_CREDENTIAL_STORE=file`，让 CLI 把登录令牌写入该隔离目录的
//! `.cursor/auth.json`。应用只读取自己创建的 profile 下的令牌，不读取用户现有
//! `~/.cursor`、Cursor.app 的 `state.vscdb` 或浏览器 Cookie。
//!
//! 额度接口是 Cursor 桌面端使用的 `GetCurrentPeriodUsage`，未公开文档；只解析
//! 套餐用量百分比与计费周期。令牌、响应正文与登录输出都不进入错误消息或日志。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use url::Url;

use crate::quota::now_epoch;
use crate::usage_api::{post_json, ProviderError};

const USAGE_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const LOGIN_URL_TIMEOUT: Duration = Duration::from_secs(20);
const WAIT_POLL: Duration = Duration::from_millis(200);
const MAX_AUTH_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorUsage {
    pub checked_at_epoch: i64,
    pub total_percent_used: Option<i64>,
    pub auto_percent_used: Option<i64>,
    pub api_percent_used: Option<i64>,
    pub cycle_start_epoch: Option<i64>,
    pub cycle_end_epoch: Option<i64>,
}

/// 浏览器授权流程的公开信息：只含流程标识与登录 URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    pub login_id: String,
    pub auth_url: String,
}

pub fn read_usage(profile: &Path) -> Result<CursorUsage, ProviderError> {
    let token = read_access_token(profile)?;
    let payload = post_json(
        USAGE_URL,
        &token,
        "{}",
        REQUEST_TIMEOUT,
        &[("Connect-Protocol-Version", "1")],
    )
    .map_err(|error| match error.status {
        Some(401) | Some(403) => {
            ProviderError::new("auth-expired", "Cursor 登录已失效，请重新登录。")
        }
        Some(429) => ProviderError::new("rate-limited", "Cursor 查询过于频繁，请稍后重试。"),
        Some(status) if status >= 500 => {
            ProviderError::new("upstream", "Cursor 服务暂时不可用，请稍后重试。")
        }
        Some(_) => ProviderError::new("http", error.message),
        None => ProviderError::new("network", error.message),
    })?;

    parse_usage(&payload, now_epoch())
}

fn parse_usage(payload: &Value, checked_at_epoch: i64) -> Result<CursorUsage, ProviderError> {
    let plan = payload
        .get("planUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::new("decode", "Cursor 未返回套餐额度数据。"))?;

    let total = percent(plan.get("totalPercentUsed"));
    let auto = percent(plan.get("autoPercentUsed"));
    let api = percent(plan.get("apiPercentUsed"));
    if total.is_none() && auto.is_none() && api.is_none() {
        return Err(ProviderError::new(
            "decode",
            "Cursor 未返回可识别的额度比例。",
        ));
    }

    Ok(CursorUsage {
        checked_at_epoch,
        total_percent_used: total,
        auto_percent_used: auto,
        api_percent_used: api,
        cycle_start_epoch: epoch_value(payload.get("billingCycleStart")),
        cycle_end_epoch: epoch_value(payload.get("billingCycleEnd")),
    })
}

fn percent(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0).ceil() as i64)
}

/// Cursor 的周期时间戳是毫秒字符串；同时兼容秒值与 RFC 3339。
fn epoch_value(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().map(normalize_epoch),
        Value::String(text) => {
            let text = text.trim();
            if let Ok(value) = text.parse::<i64>() {
                return Some(normalize_epoch(value));
            }
            OffsetDateTime::parse(text, &Rfc3339)
                .ok()
                .map(|value| value.unix_timestamp())
        }
        _ => None,
    }
}

fn normalize_epoch(value: i64) -> i64 {
    if value.abs() >= 100_000_000_000 {
        value / 1000
    } else {
        value
    }
}

#[cfg(unix)]
fn harden_auth_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.permissions().mode() & 0o077 != 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn harden_auth_permissions(_path: &Path) {}

fn auth_candidates(profile: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    let mut paths = vec![profile.join(".cursor").join("auth.json")];
    #[cfg(not(windows))]
    let paths = vec![profile.join(".cursor").join("auth.json")];
    #[cfg(windows)]
    {
        // Windows CLI 可能把登录态放在隔离的 %APPDATA%\Cursor 下。
        paths.push(
            profile
                .join("AppData")
                .join("Roaming")
                .join("Cursor")
                .join("auth.json"),
        );
    }
    paths
}

fn read_access_token(profile: &Path) -> Result<String, ProviderError> {
    let mut found = false;
    let mut decode_error = false;
    for path in auth_candidates(profile) {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        found = true;
        harden_auth_permissions(&path);
        if metadata.len() > MAX_AUTH_BYTES {
            decode_error = true;
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
            decode_error = true;
            continue;
        };
        if let Some(token) = payload
            .get("accessToken")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            return Ok(token.to_string());
        }
        decode_error = true;
    }
    if decode_error {
        return Err(ProviderError::new(
            "decode",
            "Cursor 登录凭证格式无法识别，请重新登录。",
        ));
    }
    if found {
        return Err(ProviderError::new(
            "credentials-missing",
            "Cursor 登录凭证缺少访问令牌，请重新登录。",
        ));
    }
    Err(ProviderError::new(
        "credentials-missing",
        "未找到 Cursor 登录凭证，请重新登录后再刷新。",
    ))
}

// ---------- CLI 查找 ----------

#[cfg(windows)]
const BINARY_NAMES: &[&str] = &[
    "cursor-agent.exe",
    "cursor-agent.cmd",
    "cursor-agent.bat",
    "cursor-agent.com",
    "agent.exe",
    "agent.cmd",
    "agent.bat",
    "agent.com",
];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["cursor-agent", "agent"];

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    let is_file = fs::metadata(path)
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

pub fn find_agent_binary() -> Result<PathBuf, ProviderError> {
    if let Ok(explicit) = std::env::var("CURSOR_AGENT_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable(&path) {
            return Ok(path);
        }
        return Err(ProviderError::new(
            "cli-not-found",
            format!("CURSOR_AGENT_BIN 指向的文件不可执行：{}", path.display()),
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
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.iter().find_map(|dir| find_in_dir(dir)).ok_or_else(|| {
        ProviderError::new(
            "cli-not-found",
            "未找到 Cursor Agent CLI（agent）。请先安装 Cursor CLI 后再添加账户。",
        )
    })
}

// ---------- 浏览器授权 ----------

/// 一次进行中的 Cursor CLI 登录。`wait` 持有子进程直到登录完成或取消。
pub struct LoginSession {
    child: Child,
    login_id: String,
    auth_url: String,
    profile: PathBuf,
}

impl LoginSession {
    /// 在独立 profile 中启动 `agent login`。
    ///
    /// `AGENT_CLI_CREDENTIAL_STORE=file` 必须同时用于登录和后续所有 CLI 调用；
    /// macOS 默认会改读 Keychain，只写文件不会生效。
    pub fn start(binary: &Path, profile: &Path, login_id: String) -> Result<Self, ProviderError> {
        fs::create_dir_all(profile).map_err(|error| {
            ProviderError::new("io", format!("创建 Cursor 授权目录失败：{error}"))
        })?;

        let mut command = Command::new(binary);
        command
            .arg("login")
            .env("HOME", profile)
            .env("AGENT_CLI_CREDENTIAL_STORE", "file")
            .env("NO_OPEN_BROWSER", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            command
                .env("USERPROFILE", profile)
                .env("APPDATA", profile.join("AppData").join("Roaming"))
                .env("LOCALAPPDATA", profile.join("AppData").join("Local"));
        }
        crate::cli::apply_child_path(&mut command, binary);
        crate::cli::apply_no_window(&mut command);

        let mut child = command
            .spawn()
            .map_err(|error| ProviderError::new("io", format!("启动 Cursor CLI 失败：{error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::new("io", "无法读取 Cursor CLI 登录输出。"))?;
        let stderr = child.stderr.take();

        let (lines_tx, lines_rx) = mpsc::channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                // start 返回后接收端会被丢弃；继续读取并丢弃，避免子进程写满管道。
                let _ = lines_tx.send(line);
            }
        });
        if let Some(stderr) = stderr {
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for _ in reader.lines() {}
            });
        }

        let deadline = Instant::now() + LOGIN_URL_TIMEOUT;
        let auth_url = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                kill_child(&mut child);
                return Err(ProviderError::new(
                    "timeout",
                    "Cursor CLI 未能返回登录地址，请确认 CLI 可用后重试。",
                ));
            }
            match lines_rx.recv_timeout(remaining.min(Duration::from_millis(500))) {
                Ok(line) => {
                    if let Some(url) = parse_login_url(&line) {
                        break url;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        return Err(ProviderError::new(
                            "login",
                            format!(
                                "Cursor CLI 在返回登录地址前退出（退出码 {}）。",
                                status.code().unwrap_or(-1)
                            ),
                        ));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = child.wait().ok();
                    return Err(ProviderError::new(
                        "login",
                        format!(
                            "Cursor CLI 在返回登录地址前退出（退出码 {}）。",
                            status.and_then(|status| status.code()).unwrap_or(-1)
                        ),
                    ));
                }
            }
        };

        Ok(Self {
            child,
            login_id,
            auth_url,
            profile: profile.to_path_buf(),
        })
    }

    pub fn info(&self) -> LoginStart {
        LoginStart {
            login_id: self.login_id.clone(),
            auth_url: self.auth_url.clone(),
        }
    }

    /// 等待登录进程结束并确认隔离目录中已写入凭证。
    pub fn wait(&mut self, cancel: &AtomicBool, timeout: Duration) -> Result<(), ProviderError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                kill_child(&mut self.child);
                return Err(ProviderError::new("canceled", "已取消 Cursor 登录。"));
            }
            if Instant::now() >= deadline {
                kill_child(&mut self.child);
                return Err(ProviderError::new(
                    "timeout",
                    "等待 Cursor 浏览器登录超时，请重新发起登录。",
                ));
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return Err(ProviderError::new(
                            "login",
                            format!(
                                "Cursor 登录未完成（退出码 {}）。",
                                status.code().unwrap_or(-1)
                            ),
                        ));
                    }
                    // CLI 先写文件再退出；留一点余量处理文件可见性延迟。
                    for _ in 0..10 {
                        if read_access_token(&self.profile).is_ok() {
                            return Ok(());
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    return Err(ProviderError::new(
                        "credentials-missing",
                        "Cursor 登录已完成，但隔离目录中未找到凭证，请重试。",
                    ));
                }
                Ok(None) => thread::sleep(WAIT_POLL),
                Err(error) => {
                    return Err(ProviderError::new(
                        "io",
                        format!("等待 Cursor 登录进程失败：{error}"),
                    ))
                }
            }
        }
    }
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// 从 CLI 输出中提取并校验登录地址。只接受 Cursor 官方登录页。
fn parse_login_url(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("navigate to this link:")?;
    let candidate = rest.split_whitespace().next()?;
    let url = Url::parse(candidate).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    if !matches!(url.host_str(), Some("cursor.com") | Some("www.cursor.com")) {
        return None;
    }
    if !url.path().starts_with("/loginDeepControl") {
        return None;
    }
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_usage_reads_percentages_and_cycle() {
        let payload = json!({
            "planUsage": {
                "totalPercentUsed": 12.5,
                "autoPercentUsed": 0,
                "apiPercentUsed": 99.2
            },
            "billingCycleStart": "1792728069124",
            "billingCycleEnd": "1795406469124"
        });
        let usage = parse_usage(&payload, 100).unwrap();
        assert_eq!(usage.checked_at_epoch, 100);
        assert_eq!(usage.total_percent_used, Some(13));
        assert_eq!(usage.auto_percent_used, Some(0));
        assert_eq!(usage.api_percent_used, Some(100));
        assert_eq!(usage.cycle_start_epoch, Some(1_792_728_069));
        assert_eq!(usage.cycle_end_epoch, Some(1_795_406_469));
    }

    #[test]
    fn parse_usage_accepts_rfc3339_cycle() {
        let payload = json!({
            "planUsage": { "totalPercentUsed": 1 },
            "billingCycleEnd": "2026-11-23T04:01:09.124Z"
        });
        let usage = parse_usage(&payload, 100).unwrap();
        assert_eq!(usage.cycle_end_epoch, Some(1_795_406_469));
    }

    #[test]
    fn parse_usage_requires_a_percentage() {
        let payload = json!({ "planUsage": { "totalPercentUsed": null } });
        assert_eq!(parse_usage(&payload, 100).unwrap_err().code, "decode");
    }

    #[test]
    fn parse_login_url_only_accepts_cursor_login_page() {
        let line = "Open a browser and navigate to this link: \
                    https://cursor.com/loginDeepControl?challenge=abc&uuid=def";
        assert_eq!(
            parse_login_url(line).as_deref(),
            Some("https://cursor.com/loginDeepControl?challenge=abc&uuid=def")
        );
        assert!(parse_login_url(
            "Open a browser and navigate to this link: https://example.com/loginDeepControl"
        )
        .is_none());
        assert!(parse_login_url(
            "Open a browser and navigate to this link: https://cursor.com/other"
        )
        .is_none());
        assert!(parse_login_url("no link here").is_none());
    }

    #[test]
    fn normalize_epoch_handles_seconds_and_millis() {
        assert_eq!(normalize_epoch(1_792_728_069), 1_792_728_069);
        assert_eq!(normalize_epoch(1_792_728_069_124), 1_792_728_069);
    }

    #[cfg(unix)]
    #[test]
    fn login_session_drives_fake_agent_in_isolated_home() {
        use std::os::unix::fs::PermissionsExt;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "token-refill-cursor-login-{}-{stamp}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let profile = dir.join("profile");
        let binary = dir.join("agent");
        fs::write(
            &binary,
            r#"#!/bin/sh
echo "Open a browser and navigate to this link: https://cursor.com/loginDeepControl?challenge=test"
mkdir -p "$HOME/.cursor"
printf '%s' '{"accessToken":"fake-token"}' > "$HOME/.cursor/auth.json"
"#,
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

        let mut session = LoginSession::start(&binary, &profile, "test-login".to_string())
            .expect("fake agent should return a login URL");
        assert_eq!(
            session.info().auth_url,
            "https://cursor.com/loginDeepControl?challenge=test"
        );
        session
            .wait(&AtomicBool::new(false), Duration::from_secs(5))
            .expect("fake agent should leave a readable auth file");
        assert_eq!(
            read_access_token(&profile).unwrap(),
            "fake-token".to_string()
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
