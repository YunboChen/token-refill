//! Codex 订阅用量读取：通过本机 `codex app-server` 的 stdio JSON-RPC 协议查询额度。
//!
//! 两数据来源：
//! - 本机账户：沿用用户已有的 Codex 登录态（不读取、不复制其凭证）。
//! - 托管账户：为每个账户设置独立 `CODEX_HOME`，通过官方 `account/login/start`
//!   浏览器授权登录；授权令牌由 Codex CLI 写入该独立目录，应用不解析、不回传。
//!
//! 查询只启动官方 app-server 子进程发起只读请求，完成后立即退出；CLI 缺失或
//! 未登录时返回结构化错误，由界面提示。协议结构来自
//! `codex app-server generate-json-schema`（CLI 0.160.1）。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// `account/read` 返回的脱敏身份信息；不含任何令牌。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexIdentity {
    pub account_type: String,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

/// 浏览器授权流程的公开信息：只含流程标识与授权 URL。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub auth_url: String,
}

/// 授权完成后的结果。额度读取失败不影响账户已授权的事实。
#[derive(Debug)]
pub struct LoginOutcome {
    pub identity: Option<CodexIdentity>,
    pub usage: Option<CodexUsage>,
    pub usage_error: Option<UsageError>,
}

/// 一次查询的结果。额度失败不丢弃已读到的身份：网络异常时仍能展示本机
/// 登录的是哪个账户。`identity_checked` 表示 `account/read` 是否成功返回；
/// 只有成功返回且账户为空时，才应清除界面上的身份提示。
#[derive(Debug)]
pub struct CodexReadout {
    pub usage: Result<CodexUsage, UsageError>,
    pub identity: Option<CodexIdentity>,
    pub identity_checked: bool,
}

/// 只把明确的登录/未授权错误归为授权失效；普通网络或协议错误仍按 rpc 处理，
/// 避免把超时误报成需要重新登录。
fn classify_rpc_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    let auth_markers = [
        "not logged in",
        "unauthorized",
        "unauthenticated",
        "authentication required",
        "401",
    ];
    if auth_markers.iter().any(|marker| lower.contains(marker)) {
        "auth-expired"
    } else {
        "rpc"
    }
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

/// 解析 `account/read` 结果。账户为空时返回 None，而不是伪造身份。
fn parse_identity(value: &serde_json::Value) -> Option<CodexIdentity> {
    let account = value.get("account")?.as_object()?;
    Some(CodexIdentity {
        account_type: account
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string(),
        email: account
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        plan_type: account
            .get("planType")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

// ---------- Codex CLI 查找 ----------
//
// 查找顺序：CODEX_BIN → PATH → 版本管理器（nvm/fnm/nvm-windows）→ 各包管理器默认目录。
// Windows 上 CLI 常以 `codex.exe`（官方安装包）或 `codex.cmd`（npm/pnpm/scoop
// 包装脚本）存在，没有 Unix 的执行位，因此可执行判定与候选文件名都按平台区分。

#[cfg(windows)]
const BINARY_NAMES: &[&str] = &["codex.exe", "codex.cmd", "codex.bat", "codex.com"];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["codex"];

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Windows 没有执行位，只按扩展名判定。`.cmd`/`.bat` 由 Rust 标准库（≥ 1.77.2）
/// 自动经 `cmd.exe` 启动并转义参数，因此可以直接交给 `Command`。
#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    let is_file = fs::metadata(path)
        .map(|meta| meta.is_file())
        .unwrap_or(false);
    if !is_file {
        return false;
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "exe" | "cmd" | "bat" | "com"
            )
        })
}

/// `HOME` 在 Windows 上可能由 Git Bash 等终端设成 `/c/Users/...` 形式，
/// 因此 Windows 先取 `USERPROFILE`；Unix 没有 `USERPROFILE`，仍以 `HOME` 为准。
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

/// 在版本管理器的版本根目录里取最新一份 codex。目录名按字符串降序
/// （v26.12.0 > v20.0.0 基本成立），没有安装 CLI 的版本目录会被跳过。
fn find_in_version_dirs(root: &Path, suffix: &[&str]) -> Option<PathBuf> {
    let mut versions: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort_by(|a, b| b.cmp(a));
    versions.iter().find_map(|version| {
        let mut dir = version.clone();
        for part in suffix {
            dir.push(part);
        }
        find_in_dir(&dir)
    })
}

/// 版本管理器根目录及其相对可执行文件所在的子路径。
fn version_dirs() -> Vec<(PathBuf, &'static [&'static str])> {
    let mut roots: Vec<(PathBuf, &'static [&'static str])> = Vec::new();
    #[cfg(unix)]
    if let Some(home) = home_dir() {
        roots.push((home.join(".nvm").join("versions").join("node"), &["bin"]));
    }
    #[cfg(windows)]
    {
        // nvm-windows：版本目录直接位于 NVM_HOME（默认 %APPDATA%\nvm）下。
        let nvm_root = std::env::var_os("NVM_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("nvm")));
        if let Some(root) = nvm_root {
            roots.push((root, &[]));
        }
        // fnm：node-versions\<version>\installation\codex.cmd
        if let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from) {
            roots.push((base.join("fnm").join("node-versions"), &["installation"]));
        }
    }
    roots
}

/// PATH 之外的常见安装位置。从资源管理器启动的 GUI 应用拿不到终端里临时
/// 追加的 PATH，所以在包管理器的默认目录再找一遍。
fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
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
        let env_dir = |key: &str| std::env::var_os(key).map(PathBuf::from);
        if let Some(base) = env_dir("APPDATA") {
            dirs.push(base.join("npm"));
        }
        if let Some(base) = env_dir("LOCALAPPDATA") {
            dirs.push(base.join("pnpm"));
            dirs.push(base.join("Volta").join("bin"));
            // 官方 Windows 安装包的默认位置
            dirs.push(
                base.join("Programs")
                    .join("OpenAI")
                    .join("Codex")
                    .join("bin"),
            );
            dirs.push(base.join("Microsoft").join("WindowsApps"));
        }
        // nvm-windows 活动版本的软链接目录
        if let Some(base) = env_dir("NVM_SYMLINK") {
            dirs.push(base);
        }
        if let Some(home) = home_dir() {
            dirs.push(home.join(".bun").join("bin"));
            dirs.push(home.join("scoop").join("shims"));
        }
    }
    dirs
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
            if let Some(candidate) = find_in_dir(&dir) {
                return Ok(candidate);
            }
        }
    }

    for (root, suffix) in version_dirs() {
        if let Some(candidate) = find_in_version_dirs(&root, suffix) {
            return Ok(candidate);
        }
    }

    for dir in extra_search_dirs() {
        if let Some(candidate) = find_in_dir(&dir) {
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
    /// `codex_home` 为 Some 时使用独立的 CODEX_HOME；None 时沿用进程环境，
    /// 即本机已登录的 Codex 账户。
    fn spawn(binary: &Path, codex_home: Option<&Path>) -> Result<Self, UsageError> {
        let mut command = Command::new(binary);
        command
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(codex_home) = codex_home {
            command.env("CODEX_HOME", codex_home);
        }
        let mut child = command.spawn().map_err(io_error)?;

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

    fn initialize(&mut self) -> Result<(), UsageError> {
        self.send(serde_json::json!({
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
        self.await_response(1, INITIALIZE_TIMEOUT)?;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized"
        }))
    }

    fn send_request(
        &mut self,
        id: i64,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, UsageError> {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        self.await_response(id, timeout)
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
                return Err(UsageError::new(
                    classify_rpc_error(text),
                    format!("Codex 查询失败：{text}"),
                ));
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

/// `codex_home` 为 Some 时查询该独立授权目录下的账户，None 时查询本机登录态。
pub fn read_usage(codex_home: Option<&Path>) -> Result<CodexReadout, UsageError> {
    let binary = find_codex_binary()?;
    read_usage_from(&binary, codex_home)
}

fn read_usage_from(binary: &Path, codex_home: Option<&Path>) -> Result<CodexReadout, UsageError> {
    let mut server = AppServer::spawn(binary, codex_home)?;
    server.initialize()?;

    // 身份读取是附带能力：失败不影响额度查询，也不能据此清空已有身份提示。
    let (identity, identity_checked) = match server.send_request(
        2,
        "account/read",
        serde_json::json!({ "refreshToken": false }),
        READ_TIMEOUT,
    ) {
        Ok(result) => (parse_identity(&result), true),
        Err(_) => (None, false),
    };

    let usage = server
        .send_request(
            3,
            "account/rateLimits/read",
            serde_json::json!({ "excludeResetCreditDetails": true }),
            READ_TIMEOUT,
        )
        .and_then(|result| {
            let checked_at_epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or_default();
            parse_rate_limits(result, checked_at_epoch)
        });

    Ok(CodexReadout {
        usage,
        identity,
        identity_checked,
    })
}

// ---------- 浏览器授权登录 ----------

/// 一次进行中的浏览器授权。`wait` 持有 app-server 进程直到授权完成或取消。
pub struct LoginSession {
    server: AppServer,
    login_id: String,
    auth_url: String,
    next_request_id: i64,
}

/// Codex 的登录回调端口固定为 1455（1457 只在端口被占用时回退），app-server
/// 没有改端口的参数。Windows 的 Hyper-V/WinNAT 会把一段 TCP 端口划进预留区间，
/// 落在其中的端口绑定会返回 WSAEACCES(10013)；这里把系统错误翻译成可执行的修复步骤。
fn explain_login_start_error(error: UsageError) -> UsageError {
    #[cfg(target_os = "windows")]
    {
        if error.message.contains("10013") {
            return UsageError::new(
                "login-port-blocked",
                "Codex 授权回调端口（1455/1457）被 Windows 预留，无法监听（os error 10013）。\
                 请以管理员身份依次执行：net stop winnat；\
                 netsh int ipv4 add excludedportrange protocol=tcp startport=1455 numberofports=3；\
                 net start winnat。完成后重试添加账户。"
                    .to_string(),
            );
        }
    }
    error
}

impl LoginSession {
    /// 在独立 `CODEX_HOME` 中启动官方 ChatGPT 授权流程。
    pub fn start(binary: &Path, codex_home: &Path) -> Result<Self, UsageError> {
        let mut server = AppServer::spawn(binary, Some(codex_home))?;
        server.initialize()?;
        server.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "account/login/start",
            "params": { "type": "chatgpt" }
        }))?;
        let result = server
            .await_response(2, INITIALIZE_TIMEOUT)
            .map_err(explain_login_start_error)?;
        let login_id = result
            .get("loginId")
            .and_then(|value| value.as_str())
            .ok_or_else(|| UsageError::new("decode", "Codex 授权响应缺少 loginId"))?
            .to_string();
        let auth_url = result
            .get("authUrl")
            .and_then(|value| value.as_str())
            .ok_or_else(|| UsageError::new("decode", "Codex 授权响应缺少授权地址"))?
            .to_string();
        Ok(Self {
            server,
            login_id,
            auth_url,
            next_request_id: 3,
        })
    }

    pub fn info(&self) -> LoginStart {
        LoginStart {
            login_id: self.login_id.clone(),
            auth_url: self.auth_url.clone(),
        }
    }

    /// 等待 `account/login/completed`。`cancel` 置位后主动取消并返回 canceled。
    pub fn wait(
        &mut self,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<LoginOutcome, UsageError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                self.cancel();
                return Err(UsageError::new("canceled", "已取消授权。"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.cancel();
                return Err(UsageError::new(
                    "timeout",
                    "等待浏览器授权超时，请重新发起授权。",
                ));
            }
            let wait = remaining.min(Duration::from_millis(250));
            match self.server.incoming.recv_timeout(wait) {
                Ok(message) => {
                    if message.get("method").and_then(|value| value.as_str())
                        != Some("account/login/completed")
                    {
                        continue;
                    }
                    let params = message
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let same_login = params
                        .get("loginId")
                        .and_then(|value| value.as_str())
                        .map(|id| id == self.login_id)
                        .unwrap_or(true);
                    if !same_login {
                        continue;
                    }
                    if params
                        .get("success")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        break;
                    }
                    let detail = params
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("浏览器授权未完成");
                    return Err(UsageError::new(
                        "login",
                        format!("Codex 授权失败：{detail}"),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = self.server.child.wait().ok();
                    let code = status.and_then(|s| s.code()).unwrap_or(-1);
                    return Err(UsageError::new(
                        "server-exit",
                        format!("Codex app-server 在授权期间退出（退出码 {code}）。"),
                    ));
                }
            }
        }

        let identity = self.identity().ok().flatten();
        let (usage, usage_error) = match self.rate_limits() {
            Ok(usage) => (Some(usage), None),
            Err(error) => (None, Some(error)),
        };
        Ok(LoginOutcome {
            identity,
            usage,
            usage_error,
        })
    }

    fn identity(&mut self) -> Result<Option<CodexIdentity>, UsageError> {
        let id = self.take_request_id();
        self.server.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "account/read",
            "params": { "refreshToken": false }
        }))?;
        let result = self.server.await_response(id, READ_TIMEOUT)?;
        Ok(parse_identity(&result))
    }

    fn rate_limits(&mut self) -> Result<CodexUsage, UsageError> {
        let id = self.take_request_id();
        self.server.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "account/rateLimits/read",
            "params": { "excludeResetCreditDetails": true }
        }))?;
        let result = self.server.await_response(id, READ_TIMEOUT)?;
        let checked_at_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        parse_rate_limits(result, checked_at_epoch)
    }

    fn take_request_id(&mut self) -> i64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    fn cancel(&mut self) {
        let id = self.take_request_id();
        if self
            .server
            .send(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "account/login/cancel",
                "params": { "loginId": self.login_id }
            }))
            .is_ok()
        {
            let _ = self.server.await_response(id, Duration::from_secs(3));
        }
    }
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

    /// 合成 app-server 用 Python 脚本实现，只在有 python3 的 Unix 环境验证；
    /// Windows 下的查找与启动由纯逻辑测试覆盖。
    #[cfg(unix)]
    #[test]
    fn read_usage_drives_fake_app_server_over_stdio() {
        use std::os::unix::fs::PermissionsExt;

        // 合成数据脚本模拟 `codex app-server --stdio`：验证进程管理、
        // 行分隔 JSON-RPC、通知跳过与响应解析全链路，不使用真实账户。
        let result = serde_json::json!({
            "rateLimits": fixture()["rateLimits"],
            "rateLimitsByLimitId": fixture()["rateLimitsByLimitId"]
        })
        .to_string();
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json
RESULT = {result:?}
IDENTITY = {{"account": {{"type": "chatgpt", "email": "local@example.com", "planType": "plus"}}, "requiresOpenaiAuth": True}}
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": {{"userAgent": "fake"}}}}), flush=True)
        print(json.dumps({{"jsonrpc": "2.0", "method": "account/rateLimits/updated", "params": {{}}}}), flush=True)
    elif method == "account/read":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": IDENTITY}}), flush=True)
    elif method == "account/rateLimits/read":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": json.loads(RESULT)}}), flush=True)
"#
        );
        let path =
            std::env::temp_dir().join(format!("token-refill-fake-codex-{}.py", std::process::id()));
        fs::write(&path, script).expect("write fake server");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake server");

        let readout = read_usage_from(&path, None).expect("fake server responds");
        fs::remove_file(&path).ok();

        let usage = readout.usage.expect("usage parses");
        assert!(usage.checked_at_epoch > 0);
        assert_eq!(usage.buckets.len(), 1);
        assert_eq!(usage.buckets[0].windows.len(), 2);
        assert_eq!(usage.buckets[0].windows[0].remaining_percent, 96);
        assert_eq!(usage.buckets[0].windows[1].remaining_percent, 87);
        assert!(readout.identity_checked);
        assert_eq!(
            readout
                .identity
                .and_then(|identity| identity.email)
                .as_deref(),
            Some("local@example.com")
        );
    }

    #[test]
    fn account_response_parses_identity_without_tokens() {
        let identity = parse_identity(&serde_json::json!({
            "account": {
                "type": "chatgpt",
                "email": "synth@example.com",
                "planType": "plus"
            },
            "requiresOpenaiAuth": true
        }))
        .expect("identity");
        assert_eq!(identity.account_type, "chatgpt");
        assert_eq!(identity.email.as_deref(), Some("synth@example.com"));
        assert_eq!(identity.plan_type.as_deref(), Some("plus"));

        assert!(parse_identity(&serde_json::json!({ "account": null })).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn login_session_drives_fake_app_server_in_isolated_home() {
        use std::os::unix::fs::PermissionsExt;

        // 合成脚本同时验证：授权流程、完成通知、独立 CODEX_HOME 与额度读取。
        // 不使用真实账户，也不写入任何真实凭证。
        let home =
            std::env::temp_dir().join(format!("token-refill-login-home-{}", std::process::id()));
        fs::remove_dir_all(&home).ok();
        fs::create_dir_all(&home).expect("create fake codex home");

        let usage = serde_json::json!({
            "rateLimits": fixture()["rateLimits"],
            "rateLimitsByLimitId": fixture()["rateLimitsByLimitId"]
        })
        .to_string();
        let identity = serde_json::json!({
            "account": {
                "type": "chatgpt",
                "email": "synth@example.com",
                "planType": "plus"
            },
            "requiresOpenaiAuth": true
        })
        .to_string();
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json, os
HOME = {home:?}
USAGE = {usage:?}
IDENTITY = {identity:?}
if os.environ.get("CODEX_HOME") != HOME:
    sys.exit(7)
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except json.JSONDecodeError:
        continue
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": {{"userAgent": "fake"}}}}), flush=True)
    elif method == "account/login/start":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": {{"type": "chatgpt", "loginId": "login-1", "authUrl": "https://auth.example.test/authorize"}}}}), flush=True)
        print(json.dumps({{"jsonrpc": "2.0", "method": "account/login/completed", "params": {{"loginId": "login-1", "success": True, "error": None}}}}), flush=True)
    elif method == "account/read":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": json.loads(IDENTITY)}}), flush=True)
    elif method == "account/rateLimits/read":
        print(json.dumps({{"jsonrpc": "2.0", "id": msg["id"], "result": json.loads(USAGE)}}), flush=True)
"#
        );
        let path = std::env::temp_dir().join(format!(
            "token-refill-fake-login-codex-{}.py",
            std::process::id()
        ));
        fs::write(&path, script).expect("write fake server");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake server");

        let mut session = LoginSession::start(&path, &home).expect("login starts");
        assert_eq!(session.info().login_id, "login-1");
        assert_eq!(
            session.info().auth_url,
            "https://auth.example.test/authorize"
        );
        let outcome = session
            .wait(&AtomicBool::new(false), Duration::from_secs(5))
            .expect("login completes");
        fs::remove_file(&path).ok();
        fs::remove_dir_all(&home).ok();

        let identity = outcome.identity.expect("identity");
        assert_eq!(identity.email.as_deref(), Some("synth@example.com"));
        let usage = outcome.usage.expect("usage");
        assert_eq!(usage.buckets[0].windows[0].remaining_percent, 96);
        assert!(outcome.usage_error.is_none());
    }

    #[test]
    fn only_explicit_auth_failures_are_classified_as_expired() {
        assert_eq!(classify_rpc_error("not logged in"), "auth-expired");
        assert_eq!(classify_rpc_error("HTTP 401 Unauthorized"), "auth-expired");
        assert_eq!(classify_rpc_error("request timed out"), "rpc");
        assert_eq!(classify_rpc_error("unexpected response shape"), "rpc");
    }

    /// CODEX_BIN 是进程级全局状态，改动它的用例必须串行执行。
    fn codex_bin_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_codex_bin<T>(value: &Path, run: impl FnOnce() -> T) -> T {
        let previous = std::env::var_os("CODEX_BIN");
        std::env::set_var("CODEX_BIN", value);
        let result = run();
        match previous {
            Some(value) => std::env::set_var("CODEX_BIN", value),
            None => std::env::remove_var("CODEX_BIN"),
        }
        result
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "token-refill-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    /// 写一个可被发现的 codex 桩文件：Windows 用 `codex.exe`，Unix 用 `codex` 并补执行位。
    fn write_stub(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).expect("create stub dir");
        let path = dir.join(BINARY_NAMES[0]);
        fs::write(&path, b"").expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        }
        path
    }

    #[test]
    fn explicit_codex_bin_must_be_executable() {
        // 仅验证纯逻辑：不可执行路径返回稳定错误码，不启动进程。
        let _guard = codex_bin_lock();
        let err = with_codex_bin(
            Path::new("/nonexistent/codex-binary-for-test"),
            find_codex_binary,
        )
        .expect_err("should fail");
        assert_eq!(err.code, "codex-not-found");
    }

    #[test]
    fn explicit_codex_bin_is_used_when_executable() {
        let _guard = codex_bin_lock();
        let dir = temp_dir("codex-bin");
        let stub = write_stub(&dir);

        let found = with_codex_bin(&stub, find_codex_binary).expect("CODEX_BIN is accepted");
        fs::remove_dir_all(&dir).ok();

        assert_eq!(found, stub);
    }

    #[test]
    fn binary_names_match_platform_packaging() {
        #[cfg(windows)]
        {
            // 官方安装包是 .exe，npm/pnpm/scoop 落地为 .cmd；优先原生可执行文件。
            assert_eq!(BINARY_NAMES[0], "codex.exe");
            assert!(BINARY_NAMES.contains(&"codex.cmd"));
        }
        #[cfg(not(windows))]
        assert_eq!(BINARY_NAMES, &["codex"]);
    }

    #[test]
    fn version_search_picks_newest_version_that_has_a_binary() {
        let root = temp_dir("versions");
        let older = write_stub(&root.join("v20.1.0").join("bin"));
        let newer = write_stub(&root.join("v22.0.0").join("bin"));
        // 更新的版本目录里没有安装 CLI，应跳过而不是报错。
        fs::create_dir_all(root.join("v23.0.0").join("bin")).expect("create empty version");

        let found = find_in_version_dirs(&root, &["bin"]).expect("finds newest installed");
        fs::remove_dir_all(&root).ok();

        assert_eq!(found, newer);
        assert_ne!(found, older);
    }

    #[cfg(windows)]
    #[test]
    fn windows_probes_follow_extension_rules() {
        let dir = temp_dir("windows-probe");
        fs::create_dir_all(&dir).expect("create probe dir");

        fs::write(dir.join("codex.txt"), b"").expect("write txt stub");
        assert!(find_in_dir(&dir).is_none(), "非可执行扩展名不应命中");

        fs::write(dir.join("codex.cmd"), b"").expect("write cmd stub");
        assert_eq!(find_in_dir(&dir), Some(dir.join("codex.cmd")));

        // 同时存在时优先原生 .exe。
        fs::write(dir.join("codex.exe"), b"").expect("write exe stub");
        assert_eq!(find_in_dir(&dir), Some(dir.join("codex.exe")));

        assert!(!is_executable(&dir), "目录不算可执行文件");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_port_login_errors_are_passed_through() {
        let error = explain_login_start_error(UsageError::new("rpc", "Codex 查询失败：网络不可用"));
        assert_eq!(error.code, "rpc");
        assert!(error.message.contains("网络不可用"));
    }

    #[cfg(windows)]
    #[test]
    fn blocked_callback_port_gets_actionable_guidance() {
        let error = explain_login_start_error(UsageError::new(
            "rpc",
            "Codex 查询失败：failed to start login server: 以一种访问权限不允许的方式做了一个访问套接字的尝试。 (os error 10013)",
        ));
        assert_eq!(error.code, "login-port-blocked");
        assert!(error.message.contains("excludedportrange"));
        assert!(error.message.contains("1455"));
    }
}
