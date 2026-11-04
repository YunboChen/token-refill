//! 商汤 SenseNova Token Plan 额度（实验性）。
//!
//! 额度只存在于控制台账户接口，`sk-` 推理 Key 调用会返回 401；因此需要先用
//! 官方 OAuth 授权码流程（公开客户端 `nova`）换取 refresh_token，再每次续期
//! 得到约 3 小时有效的 access_token。refresh_token 会轮换，续期成功后必须
//! 立即写回。
//!
//! 授权页由应用内 WebView 打开，只截获跳回
//! `https://platform.sensenova.cn/?code=...` 的授权码；不读取浏览器 Cookie，
//! 也不保存账号密码。接口与登录流程均未公开文档，按实验性功能对待。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::quota::now_epoch;
use crate::usage_api::ProviderError;

const AUTH_URL: &str = "https://platform.sensenova.cn/oauth2/auth";
const TOKEN_URL: &str = "https://platform.sensenova.cn/oauth2/token";
const USAGE_URL: &str = "https://platform.sensenova.cn/lite/console/v1/tokenplan/pool-usage";
const CLIENT_ID: &str = "nova";
const REDIRECT_URI: &str = "https://platform.sensenova.cn";
const REDIRECT_URI_ENCODED: &str = "https%3A%2F%2Fplatform.sensenova.cn";
const SCOPE_ENCODED: &str = "openid%20offline%20offline_access";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE_BYTES: u64 = 512 * 1024;
const SESSION_FILE: &str = "session.json";
/// access_token 临近过期时提前续期，避免查询请求正好撞上失效。
const ACCESS_TOKEN_MARGIN_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenseNovaError {
    pub code: String,
    pub message: String,
}

impl SenseNovaError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn to_provider_error(&self) -> ProviderError {
        ProviderError::new(&self.code, &self.message)
    }
}

fn auth_error() -> SenseNovaError {
    SenseNovaError::new("auth-expired", "商汤授权已失效，请重新登录。")
}

fn map_status(status: u16) -> SenseNovaError {
    match status {
        401 | 403 => auth_error(),
        429 => SenseNovaError::new("rate-limited", "商汤查询过于频繁，请稍后重试。"),
        status if status >= 500 => {
            SenseNovaError::new("upstream", "商汤服务暂时不可用，请稍后重试。")
        }
        status => SenseNovaError::new("http", format!("商汤接口返回 HTTP {status}。")),
    }
}

// ---------- 登录：PKCE 与授权码 ----------

pub struct LoginRequest {
    pub auth_url: String,
    pub verifier: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    Code { code: String, state: Option<String> },
    Error(String),
}

fn random_b64(bytes: usize) -> Result<String, SenseNovaError> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer)
        .map_err(|_| SenseNovaError::new("internal", "无法生成登录随机数，请重试。"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer))
}

/// 生成一次授权请求。verifier 与 state 只保留在内存里；应用重启后本次流程作废。
pub fn start_login() -> Result<LoginRequest, SenseNovaError> {
    let verifier = random_b64(32)?;
    let state = random_b64(16)?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    Ok(LoginRequest {
        auth_url: format!(
            "{AUTH_URL}?response_type=code&client_id={CLIENT_ID}\
             &code_challenge_method=S256&code_challenge={challenge}\
             &redirect_uri={REDIRECT_URI_ENCODED}&scope={SCOPE_ENCODED}\
             &state={state}&lang=zh-CN"
        ),
        verifier,
        state,
    })
}

/// 登录窗口只允许在商汤域名内跳转，避免被第三方页面顶替。
pub fn navigation_allowed(url: &Url) -> bool {
    if url.scheme() == "about" {
        return true;
    }
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    host == "sensenova.cn"
        || host.ends_with(".sensenova.cn")
        || host == "sensecore.cn"
        || host.ends_with(".sensecore.cn")
        || host == "sensecoreapi.cn"
        || host.ends_with(".sensecoreapi.cn")
}

/// 从授权跳转 URL 里取 code / state；错误跳转保留简短原因。
pub fn parse_callback(url: &Url) -> Option<Callback> {
    if url.scheme() != "https" || url.host_str() != Some("platform.sensenova.cn") {
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        let message = if error.trim().is_empty() {
            "商汤授权被拒绝。".to_string()
        } else {
            format!("商汤授权被拒绝：{error}")
        };
        return Some(Callback::Error(message));
    }
    code.filter(|value| !value.trim().is_empty())
        .map(|code| Callback::Code { code, state })
}

/// 用授权码换取令牌。返回的 refresh_token 是后续自动续期的唯一凭据。
pub fn exchange_code(
    code: &str,
    verifier: &str,
    expected_state: &str,
    received_state: Option<&str>,
) -> Result<Session, SenseNovaError> {
    if let Some(received) = received_state.filter(|value| !value.is_empty()) {
        if received != expected_state {
            return Err(SenseNovaError::new(
                "auth-state",
                "商汤授权状态校验失败，请重新发起登录。",
            ));
        }
    }
    let (status, body) = post_form(&[
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", verifier),
    ])?;
    if status == 400 && body.to_lowercase().contains("invalid_grant") {
        return Err(SenseNovaError::new(
            "auth-expired",
            "商汤授权码已失效，请重新登录。",
        ));
    }
    if status != 200 {
        return Err(map_status(status));
    }
    token_response(&body, None)
}

// ---------- 令牌续期与额度查询 ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token_expires_at_epoch: Option<i64>,
}

fn token_response(body: &str, previous_refresh: Option<&str>) -> Result<Session, SenseNovaError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| SenseNovaError::new("decode", "商汤登录响应格式无法识别。"))?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(auth_error)?
        .to_string();
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| previous_refresh.map(str::to_string))
        .ok_or_else(|| {
            SenseNovaError::new("decode", "商汤登录响应未包含 refresh_token，请重新登录。")
        })?;
    let expires_in = value
        .get("expires_in")
        .and_then(number)
        .filter(|seconds| *seconds > 0.0)
        .unwrap_or(10_800.0) as i64;
    Ok(Session {
        refresh_token,
        access_token: Some(access_token),
        access_token_expires_at_epoch: Some(now_epoch() + expires_in),
    })
}

fn refresh_session(refresh_token: &str) -> Result<Session, SenseNovaError> {
    let (status, body) = post_form(&[
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
    ])?;
    if status == 400 && body.to_lowercase().contains("invalid_grant") {
        return Err(auth_error());
    }
    if status != 200 {
        return Err(map_status(status));
    }
    token_response(&body, Some(refresh_token))
}

fn access_token_valid(session: &Session, now: i64) -> bool {
    let Some(token) = session.access_token.as_deref() else {
        return false;
    };
    if token.trim().is_empty() {
        return false;
    }
    session
        .access_token_expires_at_epoch
        .or_else(|| jwt_expiry(token))
        .is_some_and(|expires| expires > now + ACCESS_TOKEN_MARGIN_SECS)
}

fn jwt_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("exp").and_then(Value::as_i64)
}

/// 读取额度：access_token 仍有效就直接查；否则续期一次。
/// 续期成功后立即把轮换后的 refresh_token 写回私有目录。
pub fn read_usage(profile_dir: &Path) -> Result<Usage, SenseNovaError> {
    let mut session = load_session(profile_dir)?;
    if !access_token_valid(&session, now_epoch()) {
        session = refresh_session(&session.refresh_token)?;
        save_session(profile_dir, &session)?;
    }
    let token = session
        .access_token
        .as_deref()
        .ok_or_else(auth_error)?
        .to_string();
    match fetch_usage(&token) {
        Err(error) if error.code == "auth-expired" => {
            // access_token 被服务端提前拒绝时，续期后只重试一次。
            let refreshed = refresh_session(&session.refresh_token)?;
            save_session(profile_dir, &refreshed)?;
            fetch_usage(refreshed.access_token.as_deref().ok_or_else(auth_error)?)
        }
        result => result,
    }
}

fn fetch_usage(access_token: &str) -> Result<Usage, SenseNovaError> {
    let agent = agent(USAGE_URL);
    let mut response = agent
        .get(USAGE_URL)
        .header("Accept", "application/json")
        .header("Accept-Language", "zh-CN")
        .header("Authorization", format!("Bearer {access_token}"))
        .call()
        .map_err(|_| SenseNovaError::new("network", "无法连接到商汤额度服务，请检查网络。"))?;
    let status = response.status().as_u16();
    let body = read_body(&mut response)?;
    if status != 200 {
        return Err(map_status(status));
    }
    parse_usage(&body)
}

// ---------- 响应模型 ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub plan_name: Option<String>,
    pub pools: Vec<PoolUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolUsage {
    pub id: Option<String>,
    pub name: String,
    pub pool_type: Option<String>,
    pub window_5h: Option<WindowUsage>,
    pub window_7d: Option<WindowUsage>,
    pub grant_balance: Option<f64>,
    pub nearest_grant_expiry_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowUsage {
    pub limit: f64,
    pub used: f64,
    pub remaining: f64,
    pub resets_at_epoch: Option<i64>,
}

pub fn parse_usage(body: &str) -> Result<Usage, SenseNovaError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| SenseNovaError::new("decode", "商汤额度响应格式无法识别。"))?;
    let pools = value
        .get("pools")
        .and_then(Value::as_array)
        .ok_or_else(|| SenseNovaError::new("decode", "商汤额度响应缺少积分池。"))?;
    let plan_name = value
        .pointer("/plan/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut parsed = Vec::new();
    for pool in pools {
        let name = pool
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("Token Plan 积分池")
            .to_string();
        let id = pool.get("id").and_then(Value::as_str).map(str::to_string);
        let pool_type = pool
            .get("pool_type")
            .and_then(Value::as_str)
            .map(str::to_string);
        parsed.push(PoolUsage {
            id,
            name,
            pool_type,
            window_5h: window(pool.get("window_5h")),
            window_7d: window(pool.get("window_7d")),
            grant_balance: pool.get("grant_balance").and_then(number),
            nearest_grant_expiry_epoch: pool.get("nearest_grant_expiry").and_then(epoch_secs),
        });
    }
    Ok(Usage {
        plan_name,
        pools: parsed,
    })
}

fn window(value: Option<&Value>) -> Option<WindowUsage> {
    let detail = value?;
    let limit = detail.get("limit").and_then(number)?;
    if limit <= 0.0 {
        return None;
    }
    let used = detail.get("used").and_then(number).unwrap_or(0.0);
    let remaining = detail
        .get("remaining")
        .and_then(number)
        .unwrap_or((limit - used).max(0.0));
    Some(WindowUsage {
        limit,
        used,
        remaining: remaining.clamp(0.0, limit),
        resets_at_epoch: detail.get("reset_at").and_then(epoch_secs),
    })
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn epoch_secs(value: &Value) -> Option<i64> {
    let raw = number(value)?;
    if raw <= 0.0 || raw > 9_999_999_999.0 {
        None
    } else {
        Some(raw as i64)
    }
}

// ---------- 凭证文件 ----------

pub fn session_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(SESSION_FILE)
}

pub fn load_session(profile_dir: &Path) -> Result<Session, SenseNovaError> {
    let raw = fs::read_to_string(session_path(profile_dir))
        .map_err(|_| SenseNovaError::new("credentials-missing", "未找到商汤授权，请重新登录。"))?;
    let mut session: Session = serde_json::from_str(&raw).map_err(|_| {
        SenseNovaError::new("credentials-missing", "商汤授权文件无法读取，请重新登录。")
    })?;
    session.refresh_token = session.refresh_token.trim().to_string();
    if session.refresh_token.is_empty() {
        return Err(SenseNovaError::new(
            "credentials-missing",
            "商汤授权已失效，请重新登录。",
        ));
    }
    Ok(session)
}

/// 写入隔离目录下的会话文件。Unix 使用 0700 目录 + 0600 文件；
/// Windows 沿用 `%APPDATA%` 的按用户 ACL。先写临时文件再原子替换。
pub fn save_session(profile_dir: &Path, session: &Session) -> Result<(), SenseNovaError> {
    let storage = |message: String| SenseNovaError::new("storage", message);
    if session.refresh_token.trim().is_empty() {
        return Err(storage("商汤没有返回有效的授权凭据".to_string()));
    }
    fs::create_dir_all(profile_dir)
        .map_err(|err| storage(format!("创建商汤授权目录失败：{err}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(profile_dir, fs::Permissions::from_mode(0o700))
            .map_err(|err| storage(format!("设置商汤授权目录权限失败：{err}")))?;
    }
    let body =
        serde_json::to_vec(session).map_err(|err| storage(format!("序列化商汤授权失败：{err}")))?;
    let path = session_path(profile_dir);
    let temp = profile_dir.join(format!("{SESSION_FILE}.tmp"));
    {
        let mut file = fs::File::create(&temp)
            .map_err(|err| storage(format!("创建商汤授权文件失败：{err}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|err| storage(format!("设置商汤授权权限失败：{err}")))?;
        }
        file.write_all(&body)
            .map_err(|err| storage(format!("写入商汤授权失败：{err}")))?;
        file.sync_all()
            .map_err(|err| storage(format!("保存商汤授权失败：{err}")))?;
    }
    #[cfg(windows)]
    {
        if path.exists() {
            fs::remove_file(&path).map_err(|err| storage(format!("替换商汤授权失败：{err}")))?;
        }
    }
    fs::rename(&temp, &path).map_err(|err| storage(format!("保存商汤授权失败：{err}")))
}

// ---------- HTTP ----------

fn agent(url: &str) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = crate::proxy::ureq_proxy_for(url) {
        config = config.proxy(Some(proxy));
    }
    config.build().into()
}

fn post_form(form: &[(&str, &str)]) -> Result<(u16, String), SenseNovaError> {
    let mut response = agent(TOKEN_URL)
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .send_form(form.iter().copied())
        .map_err(|_| SenseNovaError::new("network", "无法连接到商汤登录服务，请检查网络。"))?;
    let status = response.status().as_u16();
    let body = read_body(&mut response)?;
    Ok((status, body))
}

fn read_body(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, SenseNovaError> {
    response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|_| SenseNovaError::new("network", "读取商汤响应失败。"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_usage_reads_pools_and_string_numbers() {
        let body = json!({
            "plan": { "name": "Free Plan" },
            "pools": [
                {
                    "id": "default",
                    "name": "通用积分池",
                    "pool_type": "default",
                    "window_5h": {
                        "limit": "60000",
                        "used": "33586.3",
                        "remaining": "26413.7",
                        "reset_at": "1760000000"
                    },
                    "window_7d": {
                        "limit": "600000",
                        "used": "120000",
                        "reset_at": "1760500000.0"
                    },
                    "grant_balance": "1200.5"
                },
                {
                    "name": "Flash-Lite积分池",
                    "pool_type": "dedicated",
                    "window_5h": { "limit": 60000, "used": 60000 }
                }
            ]
        })
        .to_string();
        let usage = parse_usage(&body).unwrap();
        assert_eq!(usage.plan_name.as_deref(), Some("Free Plan"));
        assert_eq!(usage.pools.len(), 2);
        let default = &usage.pools[0];
        assert_eq!(default.name, "通用积分池");
        let five = default.window_5h.as_ref().unwrap();
        assert_eq!(five.limit, 60_000.0);
        assert_eq!(five.used, 33_586.3);
        assert_eq!(five.remaining, 26_413.7);
        assert_eq!(five.resets_at_epoch, Some(1_760_000_000));
        let weekly = default.window_7d.as_ref().unwrap();
        assert_eq!(weekly.remaining, 480_000.0);
        assert_eq!(weekly.resets_at_epoch, Some(1_760_500_000));
        assert_eq!(default.grant_balance, Some(1200.5));
        assert!(usage.pools[1].window_7d.is_none());
        assert_eq!(usage.pools[1].window_5h.as_ref().unwrap().remaining, 0.0);
    }

    #[test]
    fn parse_usage_rejects_missing_pools() {
        let error = parse_usage(r#"{"plan":{}}"#).unwrap_err();
        assert_eq!(error.code, "decode");
    }

    #[test]
    fn exchange_code_rejects_state_mismatch_without_network() {
        let error = exchange_code("code", "verifier", "expected", Some("other")).unwrap_err();
        assert_eq!(error.code, "auth-state");
    }

    #[test]
    fn token_response_keeps_old_refresh_when_platform_does_not_rotate() {
        let session = token_response(
            r#"{"access_token":"at-new","expires_in":10800}"#,
            Some("rt-old"),
        )
        .unwrap();
        assert_eq!(session.access_token.as_deref(), Some("at-new"));
        assert_eq!(session.refresh_token, "rt-old");
        assert!(session.access_token_expires_at_epoch.unwrap() > now_epoch());
    }

    #[test]
    fn callback_extracts_code_and_state() {
        let url = Url::parse("https://platform.sensenova.cn/?code=abc123&scope=openid&state=st-42")
            .unwrap();
        assert_eq!(
            parse_callback(&url),
            Some(Callback::Code {
                code: "abc123".to_string(),
                state: Some("st-42".to_string())
            })
        );
    }

    #[test]
    fn callback_reports_error_and_ignores_other_hosts() {
        let denied = Url::parse("https://platform.sensenova.cn/?error=access_denied").unwrap();
        assert!(matches!(
            parse_callback(&denied),
            Some(Callback::Error(message)) if message.contains("access_denied")
        ));
        let other = Url::parse("https://example.com/?code=abc").unwrap();
        assert_eq!(parse_callback(&other), None);
    }

    #[test]
    fn navigation_stays_on_sensetime_domains() {
        assert!(navigation_allowed(
            &Url::parse("https://platform.sensenova.cn/login?x=1").unwrap()
        ));
        assert!(navigation_allowed(
            &Url::parse("https://iam.sensecoreapi.cn/iam/authn/v1/auth/consent").unwrap()
        ));
        assert!(!navigation_allowed(
            &Url::parse("https://example.com/").unwrap()
        ));
        assert!(!navigation_allowed(
            &Url::parse("http://platform.sensenova.cn/").unwrap()
        ));
    }

    #[test]
    fn jwt_expiry_reads_exp() {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"exp":1760000000}"#);
        assert_eq!(
            jwt_expiry(&format!("header.{payload}.signature")),
            Some(1_760_000_000)
        );
    }

    #[test]
    fn session_roundtrip_keeps_rotated_refresh_token() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-sensenova-session-{}-{}",
            std::process::id(),
            now_epoch()
        ));
        let session = Session {
            refresh_token: "rt-new".to_string(),
            access_token: Some("at".to_string()),
            access_token_expires_at_epoch: Some(1_760_000_000),
        };
        save_session(&dir, &session).unwrap();
        assert_eq!(load_session(&dir).unwrap(), session);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn access_token_validity_uses_expiry_margin() {
        let now = 1_760_000_000;
        let session = Session {
            refresh_token: "rt".to_string(),
            access_token: Some("at".to_string()),
            access_token_expires_at_epoch: Some(now + 600),
        };
        assert!(access_token_valid(&session, now));
        let soon = Session {
            access_token_expires_at_epoch: Some(now + 60),
            ..session.clone()
        };
        assert!(!access_token_valid(&soon, now));
        let missing = Session {
            refresh_token: "rt".to_string(),
            ..Session::default()
        };
        assert!(!access_token_valid(&missing, now));
    }

    #[test]
    fn start_login_builds_pkce_authorize_url() {
        let request = start_login().unwrap();
        assert!(request.auth_url.starts_with(AUTH_URL));
        assert!(request.auth_url.contains("client_id=nova"));
        assert!(request.auth_url.contains("code_challenge_method=S256"));
        assert!(request.auth_url.contains("state="));
        assert!(!request.verifier.is_empty());
    }
}
