//! Kiro 个人订阅额度。
//!
//! 额度只通过官方 `kiro-cli` 读取本机登录态：优先启动
//! `kiro-cli acp --agent-engine v3`，用 JSON-RPC 调用 v3 引擎自带的
//! `_kiro/account/getUsage`（CLI 自己的 `/usage` 界面也走这个方法）；
//! 旧版 CLI 没有该引擎时，回退到把 `/usage` 写进标准输入的屏幕输出解析。
//!
//! 应用不读取 Kiro IDE 的 `~/.aws/sso/cache/kiro-auth-token.json`、
//! `state.vscdb` 或浏览器 Cookie，也不复制、不刷新 CLI 维护的令牌。
//! 令牌、原始响应正文与 CLI 输出都不进入错误消息或日志。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use time::{Date, Month, PrimitiveDateTime, Time};

use crate::quota::now_epoch;
use crate::usage_api::ProviderError;

/// v3 引擎冷启动约 10 秒，给足余量但不无限等待。
const ACP_TIMEOUT: Duration = Duration::from_secs(45);
/// 旧版 CLI 的 `/usage` 屏幕输出。
const SCREEN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const READ_POLL: Duration = Duration::from_millis(200);

const NOT_LOGGED_IN_MESSAGE: &str = "kiro-cli 尚未登录，请先在终端运行 kiro-cli login。";
const UNAVAILABLE_MESSAGE: &str = "无法从 kiro-cli 读取额度，请确认已安装并登录 Kiro CLI 后重试。";

#[derive(Debug, Clone, PartialEq)]
pub struct KiroUsage {
    pub checked_at_epoch: i64,
    pub plan_name: Option<String>,
    pub billing_cycle_reset_epoch: Option<i64>,
    pub breakdowns: Vec<KiroBreakdown>,
    pub bonus_credits: Vec<KiroBonusCredit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KiroBreakdown {
    pub display_name: String,
    pub used: f64,
    pub limit: f64,
    pub has_limit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KiroBonusCredit {
    pub name: Option<String>,
    pub used: f64,
    pub total: f64,
    pub days_until_expiry: Option<i64>,
}

pub fn read_usage() -> Result<KiroUsage, ProviderError> {
    let binary = find_kiro_binary()?;
    read_usage_from_binary(&binary)
}

/// 供测试与显式路径调用；不读取除额度以外的任何输出。
pub fn read_usage_from_binary(binary: &Path) -> Result<KiroUsage, ProviderError> {
    match run_acp(binary) {
        Ok(usage) => Ok(usage),
        // 超时说明引擎没有在预算内启动，再跑一次旧屏解析只会让等待翻倍。
        Err(error) if error.code == "timeout" || error.code == "not-logged-in" => Err(error),
        Err(acp_error) => match run_screen(binary) {
            Ok(usage) => Ok(usage),
            Err(screen_error) if screen_error.code == "not-logged-in" => Err(screen_error),
            Err(_) if acp_error.code == "method" => Err(ProviderError::new(
                "unsupported",
                "当前 kiro-cli 版本未返回额度数据，请升级 Kiro CLI 后重试。",
            )),
            Err(screen_error) => Err(screen_error),
        },
    }
}

// ---------- 可执行文件查找 ----------

pub fn find_kiro_binary() -> Result<PathBuf, ProviderError> {
    if let Ok(explicit) = std::env::var("KIRO_BIN") {
        let path = PathBuf::from(explicit);
        if is_executable(&path) {
            return Ok(path);
        }
        return Err(ProviderError::new(
            "cli-not-found",
            format!("KIRO_BIN 指向的文件不可执行：{}", path.display()),
        ));
    }

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            if let Some(candidate) = find_in_dir(&dir) {
                return Ok(candidate);
            }
        }
    }

    fallback_dirs(home_dir())
        .into_iter()
        .find_map(|dir| find_in_dir(&dir))
        .ok_or_else(|| {
            ProviderError::new(
                "cli-not-found",
                "未找到 kiro-cli。请先安装 Kiro CLI 并登录，或设置 KIRO_BIN 指向其路径。",
            )
        })
}

fn fallback_dirs(home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        if let Some(home) = home {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(home.join("bin"));
        }
    }
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            // 官方安装器通过 MSI 安装并写入 PATH；这里只兜底 PATH 被裁剪的情况。
            dirs.push(base.join("kiro-cli"));
            dirs.push(base.join("Programs").join("Kiro CLI"));
            dirs.push(base.join("Programs").join("kiro-cli"));
        }
        if let Some(base) = std::env::var_os("PROGRAMFILES").map(PathBuf::from) {
            dirs.push(base.join("Kiro CLI"));
            dirs.push(base.join("kiro-cli"));
        }
        if let Some(home) = home {
            dirs.push(home.join(".local").join("bin"));
        }
    }
    dirs
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn find_in_dir(dir: &Path) -> Option<PathBuf> {
    BINARY_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|path| is_executable(path))
}

#[cfg(windows)]
const BINARY_NAMES: &[&str] = &[
    "kiro-cli.exe",
    "kiro-cli.cmd",
    "kiro-cli.bat",
    "kiro-cli.com",
];
#[cfg(not(windows))]
const BINARY_NAMES: &[&str] = &["kiro-cli"];

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

// ---------- ACP JSON-RPC ----------

fn spawn_acp(binary: &Path) -> Result<Child, ProviderError> {
    let mut command = Command::new(binary);
    command
        .args(["acp", "--agent-engine", "v3"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // 额度查询不属于任何项目；避免 CLI 读取当前目录的 Kiro 配置。
        .current_dir(std::env::temp_dir())
        .env("NO_COLOR", "1")
        // 只读取本机登录态，不让环境里的 API Key 改变要查询的账户。
        .env_remove("KIRO_API_KEY");
    crate::cli::apply_child_path(&mut command, binary);
    crate::cli::apply_no_window(&mut command);
    crate::proxy::apply_to_command(&mut command);

    command
        .spawn()
        .map_err(|error| ProviderError::new("cli-error", format!("无法启动 kiro-cli：{error}")))
}

fn run_acp(binary: &Path) -> Result<KiroUsage, ProviderError> {
    let mut child = spawn_acp(binary)?;
    let result = drive_acp(&mut child);
    kill_child(&mut child);
    result
}

fn drive_acp(child: &mut Child) -> Result<KiroUsage, ProviderError> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法写入 kiro-cli。"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法读取 kiro-cli 输出。"))?;
    let stderr = child.stderr.take();

    let logged_out = Arc::new(AtomicBool::new(false));
    let (lines_tx, lines_rx) = mpsc::channel();
    let stdout_logged_out = Arc::clone(&logged_out);
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            // 未登录提示可能出现在任意一条非 JSON 行，先记下再继续读，
            // 让最终错误能准确落到“需要登录”而不是泛化的启动失败。
            if looks_logged_out(&line) {
                stdout_logged_out.store(true, Ordering::SeqCst);
            }
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });

    if let Some(stderr) = stderr {
        let logged_out = Arc::clone(&logged_out);
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if looks_logged_out(&line) {
                    logged_out.store(true, Ordering::SeqCst);
                }
            }
        });
    }

    let deadline = Instant::now() + ACP_TIMEOUT;
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": { "name": "token-refill", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    send_request(&mut stdin, &initialize, &logged_out)?;

    let initialize_response = read_response(&lines_rx, child, 1, deadline, &logged_out)?;
    if initialize_response.get("error").is_some() {
        return Err(method_error(&logged_out));
    }

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "_kiro/account/getUsage",
        "params": {}
    });
    send_request(&mut stdin, &request, &logged_out)?;

    let response = read_response(&lines_rx, child, 2, deadline, &logged_out)?;
    if response.get("error").is_some() {
        return Err(method_error(&logged_out));
    }
    parse_acp_response(&response, now_epoch())
}

fn send_request(
    stdin: &mut impl Write,
    request: &Value,
    logged_out: &Arc<AtomicBool>,
) -> Result<(), ProviderError> {
    let write = writeln!(stdin, "{request}").and_then(|_| stdin.flush());
    write.map_err(|_| {
        if logged_out.load(Ordering::SeqCst) {
            auth_error()
        } else {
            ProviderError::new("io", "写入 kiro-cli 失败。")
        }
    })
}

fn read_response(
    lines: &mpsc::Receiver<String>,
    child: &mut Child,
    expected_id: u64,
    deadline: Instant,
    logged_out: &Arc<AtomicBool>,
) -> Result<Value, ProviderError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(timeout_error());
        }
        match lines.recv_timeout(remaining.min(READ_POLL)) {
            // ACP 会先输出通知；只取 id 匹配的响应。
            Ok(line) => {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("id").and_then(Value::as_u64) == Some(expected_id) {
                    return Ok(value);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(Some(_)) = child.try_wait() {
                    return Err(if logged_out.load(Ordering::SeqCst) {
                        auth_error()
                    } else {
                        method_error(logged_out)
                    });
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = child.wait();
                return Err(if logged_out.load(Ordering::SeqCst) {
                    auth_error()
                } else {
                    method_error(logged_out)
                });
            }
        }
    }
}

fn parse_acp_response(response: &Value, checked_at_epoch: i64) -> Result<KiroUsage, ProviderError> {
    let data = response
        .get("result")
        .and_then(|result| result.get("data"))
        .ok_or_else(decode_error)?;

    let plan_name = data
        .get("planName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);

    let billing_cycle_reset_epoch = data
        .get("billingCycleReset")
        .and_then(Value::as_str)
        .and_then(parse_reset_date);

    let breakdowns = data
        .get("usageBreakdowns")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_breakdown)
                .collect::<Vec<KiroBreakdown>>()
        })
        .unwrap_or_default();

    let bonus_credits = data
        .get("bonusCredits")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_bonus).collect::<Vec<_>>())
        .unwrap_or_default();

    if breakdowns.is_empty() && bonus_credits.is_empty() {
        return Err(decode_error());
    }

    Ok(KiroUsage {
        checked_at_epoch,
        plan_name,
        billing_cycle_reset_epoch,
        breakdowns,
        bonus_credits,
    })
}

fn parse_breakdown(item: &Value) -> Option<KiroBreakdown> {
    let display_name = item
        .get("displayName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Credits")
        .to_string();
    let used = number(item.get("used"))?;
    let limit = number(item.get("limit")).unwrap_or(0.0);
    let has_limit = item
        .get("hasLimit")
        .and_then(Value::as_bool)
        .unwrap_or(limit > 0.0);
    Some(KiroBreakdown {
        display_name,
        used,
        limit,
        has_limit,
    })
}

fn parse_bonus(item: &Value) -> Option<KiroBonusCredit> {
    let total = number(item.get("total"))?;
    let used = number(item.get("used")).unwrap_or(0.0);
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let days_until_expiry = number(item.get("daysUntilExpiry")).map(|days| days.max(0.0) as i64);
    Some(KiroBonusCredit {
        name,
        used,
        total,
        days_until_expiry,
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|text| text.trim().parse::<f64>().ok())
    })
}

fn decode_error() -> ProviderError {
    ProviderError::new("decode", "Kiro 未返回可识别的额度数据。")
}

fn auth_error() -> ProviderError {
    ProviderError::new("not-logged-in", NOT_LOGGED_IN_MESSAGE)
}

fn timeout_error() -> ProviderError {
    ProviderError::new(
        "timeout",
        "等待 kiro-cli 返回额度超时。请确认已在终端运行 kiro-cli login，或稍后重试。",
    )
}

fn method_error(logged_out: &Arc<AtomicBool>) -> ProviderError {
    if logged_out.load(Ordering::SeqCst) {
        auth_error()
    } else {
        ProviderError::new("method", UNAVAILABLE_MESSAGE)
    }
}

// ---------- 旧版 CLI 的 /usage 回退 ----------

fn run_screen(binary: &Path) -> Result<KiroUsage, ProviderError> {
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(std::env::temp_dir())
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .env_remove("KIRO_API_KEY");
    crate::cli::apply_child_path(&mut command, binary);
    crate::cli::apply_no_window(&mut command);
    crate::proxy::apply_to_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| ProviderError::new("cli-error", format!("无法启动 kiro-cli：{error}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        // 旧版 CLI 在普通 stdin 上按顺序执行斜杠命令；写完立即关闭，
        // 避免 CLI 留在交互模式。
        let _ = stdin.write_all(b"/usage\n/quit\n");
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::new("io", "无法读取 kiro-cli 输出。"))?;
    let stderr = child.stderr.take();
    let (out_tx, out_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = out_tx.send(read_capped(stdout));
    });
    let (err_tx, err_rx) = mpsc::channel();
    if let Some(stderr) = stderr {
        thread::spawn(move || {
            let _ = err_tx.send(read_capped(stderr));
        });
    }

    let deadline = Instant::now() + SCREEN_TIMEOUT;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                kill_child(&mut child);
                break;
            }
            Ok(None) => thread::sleep(READ_POLL),
            Err(_) => {
                kill_child(&mut child);
                break;
            }
        }
    }
    let _ = child.wait();

    if timed_out {
        return Err(timeout_error());
    }

    let stdout = out_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let stderr = err_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&stdout);
    let errors = String::from_utf8_lossy(&stderr);

    if looks_logged_out(&text) || looks_logged_out(&errors) {
        return Err(auth_error());
    }
    parse_screen(&text, now_epoch())
}

fn read_capped(mut reader: impl Read) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let remaining = MAX_OUTPUT_BYTES.saturating_sub(buffer.len());
                if remaining == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read.min(remaining)]);
            }
            Err(_) => break,
        }
    }
    buffer
}

fn parse_screen(text: &str, checked_at_epoch: i64) -> Result<KiroUsage, ProviderError> {
    let cleaned = strip_ansi(text);
    let mut breakdowns = Vec::new();
    let mut bonus_credits = Vec::new();

    for line in cleaned.lines() {
        if let Some((used, limit)) = parse_screen_breakdown(line) {
            breakdowns.push(KiroBreakdown {
                display_name: "Credits".to_string(),
                used,
                limit,
                has_limit: limit > 0.0,
            });
        }
        if let Some((used, total)) = parse_screen_bonus(line) {
            bonus_credits.push(KiroBonusCredit {
                name: None,
                used,
                total,
                days_until_expiry: None,
            });
        }
    }

    // 赠送额度与到期天数可能出现在相邻行，统一从全文补一次。
    if let Some(days) = parse_expiry_days(&cleaned) {
        for bonus in &mut bonus_credits {
            bonus.days_until_expiry = Some(days);
        }
    }

    if breakdowns.is_empty() && bonus_credits.is_empty() {
        return Err(decode_error());
    }

    Ok(KiroUsage {
        checked_at_epoch,
        plan_name: parse_screen_plan(&cleaned),
        billing_cycle_reset_epoch: parse_screen_reset(&cleaned),
        breakdowns,
        bonus_credits,
    })
}

fn parse_screen_breakdown(line: &str) -> Option<(f64, f64)> {
    let lower = line.to_ascii_lowercase();
    let start = lower.find("credits (")? + "credits (".len();
    let rest = &line[start..];
    let end = rest.find(')')?;
    let (used, limit) = rest[..end].split_once(" of ")?;
    Some((first_number(used)?, first_number(limit)?))
}

fn parse_screen_bonus(line: &str) -> Option<(f64, f64)> {
    let lower = line.to_ascii_lowercase();
    let start = lower.find("bonus credits:")? + "bonus credits:".len();
    let rest = line[start..].trim_start();
    let pair = rest.split_whitespace().next()?;
    let (used, total) = pair.split_once('/')?;
    Some((first_number(used)?, first_number(total)?))
}

fn parse_expiry_days(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("expires in ")? + "expires in ".len();
    let rest = &text[start..];
    first_number(rest).map(|days| days.max(0.0) as i64)
}

fn parse_screen_plan(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line
            .trim_matches(|c: char| c == '|' || c == '│' || c.is_whitespace())
            .trim();
        if trimmed.starts_with("KIRO ") {
            Some(trimmed.to_string())
        } else {
            None
        }
    })
}

fn parse_screen_reset(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("resets on ")? + "resets on ".len();
    let token = text[start..].split_whitespace().next()?;
    parse_reset_date(token)
}

fn first_number(text: &str) -> Option<f64> {
    let token: String = text
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    if token.is_empty() {
        return None;
    }
    token.parse().ok()
}

/// 去掉 CSI 与 OSC 转义，只保留可读文本。
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for item in chars.by_ref() {
                    if ('@'..='~').contains(&item) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(item) = chars.next() {
                    if item == '\u{7}' {
                        break;
                    }
                    if item == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

fn looks_logged_out(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "not logged in",
        "login required",
        "please log in",
        "please login",
        "run `kiro-cli login`",
        "kiro-cli login",
        "sign in to kiro",
        "未登录",
        "请登录",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

// ---------- 日期 ----------

/// `billingCycleReset` 是 `YYYY-MM-DD`；`MM/DD` 只出现在旧版屏幕输出。
/// 日期表示“当天重置”，取 UTC 正午，保证任何时区展示出来都是同一天。
fn parse_reset_date(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Some((year, month, day)) = split_ymd(text) {
        return epoch_from_ymd(year, month, day);
    }
    let (month, day) = text.split_once('/')?;
    let month: u8 = month.trim().parse().ok()?;
    let day: u8 = day.trim().parse().ok()?;
    let now_year = time::OffsetDateTime::now_utc().year();
    let this_year = epoch_from_ymd(now_year, month, day)?;
    if this_year >= now_epoch() {
        Some(this_year)
    } else {
        epoch_from_ymd(now_year + 1, month, day)
    }
}

fn split_ymd(text: &str) -> Option<(i32, u8, u8)> {
    let mut parts = text.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((year, month, day))
}

fn epoch_from_ymd(year: i32, month: u8, day: u8) -> Option<i64> {
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let noon = PrimitiveDateTime::new(date, Time::from_hms(12, 0, 0).ok()?);
    Some(noon.assume_utc().unix_timestamp())
}

// ---------- 进程收尾 ----------

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default()
    }

    #[test]
    fn parses_acp_plan_credits_and_bonus() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "success": true,
                "data": {
                    "planName": "KIRO PRO+",
                    "billingCycleReset": "2026-12-01",
                    "usageBreakdowns": [
                        { "displayName": "Credits", "used": 140.08, "limit": 2000, "hasLimit": true }
                    ],
                    "bonusCredits": [
                        { "name": "Launch bonus", "used": 10, "total": 100, "daysUntilExpiry": 30 }
                    ]
                }
            }
        });

        let usage = parse_acp_response(&response, 1_790_000_000).unwrap();
        assert_eq!(usage.plan_name.as_deref(), Some("KIRO PRO+"));
        assert_eq!(usage.checked_at_epoch, 1_790_000_000);
        assert_eq!(usage.breakdowns.len(), 1);
        assert_eq!(usage.breakdowns[0].display_name, "Credits");
        assert_eq!(usage.breakdowns[0].used, 140.08);
        assert_eq!(usage.breakdowns[0].limit, 2000.0);
        assert_eq!(usage.bonus_credits.len(), 1);
        assert_eq!(usage.bonus_credits[0].days_until_expiry, Some(30));
        assert_eq!(usage.billing_cycle_reset_epoch, epoch_from_ymd(2026, 12, 1));
    }

    #[test]
    fn acp_keeps_unlimited_breakdowns_out_of_limits() {
        let response = json!({
            "result": {
                "data": {
                    "usageBreakdowns": [
                        { "displayName": "Credits", "used": 12, "limit": 0, "hasLimit": false }
                    ]
                }
            }
        });
        let usage = parse_acp_response(&response, now()).unwrap();
        assert_eq!(usage.breakdowns.len(), 1);
        assert!(!usage.breakdowns[0].has_limit);
        assert_eq!(usage.breakdowns[0].limit, 0.0);
    }

    #[test]
    fn acp_rejects_missing_usage_data() {
        let response = json!({ "result": { "data": {} } });
        assert_eq!(
            parse_acp_response(&response, now()).unwrap_err().code,
            "decode"
        );
    }

    #[test]
    fn parses_screen_output_with_colors_and_reset_date() {
        let text = "\u{1b}[32m| KIRO PRO+\u{1b}[0m\n\
                    Credits (73.42 of 950 covered in plan)\n\
                    resets on 2026-12-01\n\
                    Bonus credits: 10/100 expires in 30 days\n";
        let usage = parse_screen(text, now()).unwrap();
        assert_eq!(usage.plan_name.as_deref(), Some("KIRO PRO+"));
        assert_eq!(usage.breakdowns[0].used, 73.42);
        assert_eq!(usage.breakdowns[0].limit, 950.0);
        assert_eq!(usage.bonus_credits[0].used, 10.0);
        assert_eq!(usage.bonus_credits[0].total, 100.0);
        assert_eq!(usage.bonus_credits[0].days_until_expiry, Some(30));
        assert_eq!(usage.billing_cycle_reset_epoch, epoch_from_ymd(2026, 12, 1));
    }

    #[test]
    fn screen_rejects_output_without_usage() {
        assert_eq!(parse_screen("welcome\n", now()).unwrap_err().code, "decode");
    }

    #[test]
    fn reset_without_year_rolls_to_next_year_when_past() {
        let epoch = parse_reset_date("01/01").expect("date should parse");
        assert!(epoch > now());
    }

    #[cfg(unix)]
    #[test]
    fn reads_usage_from_fake_acp_cli() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("token-refill-kiro-{}-{stamp}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("kiro-cli");
        fs::write(
            &binary,
            r#"#!/bin/sh
if [ "$*" != "acp --agent-engine v3" ]; then
  exit 2
fi
while IFS= read -r request; do
  case "$request" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","method":"_kiro/log","params":{}}'
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
      ;;
    *'_kiro/account/getUsage'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"success":true,"data":{"planName":"KIRO PRO","billingCycleReset":"2026-12-01","usageBreakdowns":[{"displayName":"Credits","used":12.5,"limit":100,"hasLimit":true}],"bonusCredits":[]}}}'
      ;;
  esac
done
"#,
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

        let usage = read_usage_from_binary(&binary).expect("fake CLI should answer");
        assert_eq!(usage.plan_name.as_deref(), Some("KIRO PRO"));
        assert_eq!(usage.breakdowns[0].used, 12.5);
        assert_eq!(usage.breakdowns[0].limit, 100.0);

        let _ = fs::remove_dir_all(&dir);
    }
}
