//! 腾讯 WorkBuddy（CodeBuddy 桌面端）Credits 只读适配器。
//!
//! 授权复用官方客户端 / CLI 的浏览器设备码流程：应用先向官方接口申请 `state`
//! 与登录页地址，用户在系统浏览器完成登录后，Rust 轮询官方接口取回访问令牌。
//! 令牌只写入应用私有目录，不进入前端状态、事件、错误消息或日志。
//!
//! 额度接口是桌面客户端在用的计费网关；这里只读取 Credits 的周期总量、
//! 剩余量与重置时间，不读取 Cookie、钥匙串或 WorkBuddy 自己的凭证文件。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const ENDPOINT: &str = "https://www.workbuddy.cn";
const AUTH_STATE_PATH: &str = "/v2/plugin/auth/state?platform=workbuddy";
const AUTH_TOKEN_PATH: &str = "/v2/plugin/auth/token?state=";
const LOGIN_ACCOUNT_PATH: &str = "/v2/plugin/login/account?state=";
const TOKEN_REFRESH_PATH: &str = "/v2/plugin/auth/token/refresh";
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const PAID_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-paid-packages";
const FREE_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-free-packages";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const LOGIN_POLL_INTERVAL: Duration = Duration::from_millis(1_500);
const LOGIN_ACCOUNT_RETRIES: usize = 6;
const TOKEN_PENDING_CODE: i64 = 11217;
const ACCOUNT_PENDING_CODE: i64 = 12151;
const CREDENTIALS_FILE: &str = "credentials.json";
/// 访问令牌到期前多久主动续期；只在有 refresh token 时生效。
pub const ACCESS_TOKEN_REFRESH_MARGIN_SECS: i64 = 60;

// ---------- 错误与凭证 ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbuddyError {
    pub code: String,
    pub message: String,
}

impl WorkbuddyError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn auth(message: impl Into<String>) -> Self {
        Self::new("auth-expired", message)
    }

    fn network(message: impl Into<String>) -> Self {
        Self::new("network", message)
    }

    fn gateway(message: impl Into<String>) -> Self {
        Self::new("gateway", message)
    }

    fn decode(message: impl Into<String>) -> Self {
        Self::new("decode", message)
    }

    pub fn is_auth_error(&self) -> bool {
        self.code == "auth-expired"
    }
}

/// 官方登录接口返回的令牌。`expires_at_epoch` 是本地换算后的 Unix 秒。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub access_expires_at_epoch: Option<i64>,
    pub refresh_expires_at_epoch: Option<i64>,
}

/// 登录账户的公开信息。UID / 企业 ID 只用于请求头与账户隔离，不进入前端状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountIdentity {
    pub nickname: Option<String>,
    pub user_id: Option<String>,
    pub enterprise_id: Option<String>,
}

/// 只保存令牌与请求头需要的账户标识；不保存邮箱、Cookie 或密码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub access_expires_at_epoch: Option<i64>,
    #[serde(default)]
    pub refresh_expires_at_epoch: Option<i64>,
    #[serde(default)]
    pub issued_at_epoch: Option<i64>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub enterprise_id: Option<String>,
}

impl Credentials {
    pub fn from_token(token: AuthToken, identity: Option<AccountIdentity>) -> Self {
        Self {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            access_expires_at_epoch: token.access_expires_at_epoch,
            refresh_expires_at_epoch: token.refresh_expires_at_epoch,
            issued_at_epoch: Some(epoch_now()),
            user_id: identity
                .as_ref()
                .and_then(|identity| identity.user_id.clone()),
            enterprise_id: identity
                .as_ref()
                .and_then(|identity| identity.enterprise_id.clone()),
        }
    }

    /// 用续期结果更新令牌；接口没有返回新的 refresh token 时保留旧值。
    pub fn apply_token(&mut self, token: AuthToken) {
        self.access_token = token.access_token;
        if token.refresh_token.is_some() {
            self.refresh_token = token.refresh_token;
        }
        if token.access_expires_at_epoch.is_some() {
            self.access_expires_at_epoch = token.access_expires_at_epoch;
        }
        if token.refresh_expires_at_epoch.is_some() {
            self.refresh_expires_at_epoch = token.refresh_expires_at_epoch;
        }
        self.issued_at_epoch = Some(epoch_now());
    }

    /// 访问令牌已到期或即将到期，且存在 refresh token。
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.refresh_token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty())
            && self
                .access_expires_at_epoch
                .is_some_and(|expires_at| expires_at <= now + ACCESS_TOKEN_REFRESH_MARGIN_SECS)
    }
}

pub fn credentials_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(CREDENTIALS_FILE)
}

/// 写入隔离目录下的凭证文件。Unix 使用 0700 目录 + 0600 文件；
/// Windows 继承应用数据目录的按用户 ACL。先写临时文件再重命名，避免半截 JSON。
pub fn save_credentials(profile_dir: &Path, credentials: &Credentials) -> Result<(), String> {
    if credentials.access_token.trim().is_empty() {
        return Err("WorkBuddy 没有返回有效的访问令牌".to_string());
    }
    fs::create_dir_all(profile_dir).map_err(|err| format!("创建 WorkBuddy 授权目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(profile_dir, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置 WorkBuddy 授权目录权限失败：{err}"))?;
    }

    let body = serde_json::to_vec(credentials)
        .map_err(|err| format!("序列化 WorkBuddy 凭证失败：{err}"))?;
    let path = credentials_path(profile_dir);
    let temp = profile_dir.join(format!("{CREDENTIALS_FILE}.tmp"));
    {
        let mut file =
            fs::File::create(&temp).map_err(|err| format!("创建 WorkBuddy 凭证文件失败：{err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("设置 WorkBuddy 凭证权限失败：{err}"))?;
        }
        file.write_all(&body)
            .map_err(|err| format!("写入 WorkBuddy 凭证失败：{err}"))?;
        file.sync_all()
            .map_err(|err| format!("保存 WorkBuddy 凭证失败：{err}"))?;
    }
    #[cfg(windows)]
    {
        // Windows 的 rename 不覆盖已存在文件；重新授权或续期时需要先移除旧凭证。
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("替换 WorkBuddy 凭证失败：{err}"))?;
        }
    }
    fs::rename(&temp, &path).map_err(|err| format!("保存 WorkBuddy 凭证失败：{err}"))
}

pub fn load_credentials(profile_dir: &Path) -> Result<Credentials, WorkbuddyError> {
    let path = credentials_path(profile_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|_| WorkbuddyError::auth("未找到 WorkBuddy 授权，请重新授权。"))?;
    let mut credentials: Credentials = serde_json::from_str(&raw)
        .map_err(|_| WorkbuddyError::auth("WorkBuddy 授权文件无法读取，请重新授权。"))?;
    credentials.access_token = credentials.access_token.trim().to_string();
    if credentials.access_token.is_empty() {
        return Err(WorkbuddyError::auth("WorkBuddy 授权已失效，请重新授权。"));
    }
    Ok(credentials)
}

// ---------- 额度模型 ----------

/// WorkBuddy 把 Credits 拆成套餐基础、套餐赠送、购买与平台奖励四类；
/// `CreditGroups` 是官方客户端使用的分组口径，应用按它分别展示。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreditGroupUsage {
    pub kind: String,
    pub name: String,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub used: Option<f64>,
    /// 套餐基础积分的下一次刷新时间。
    pub resets_at_epoch: Option<i64>,
    /// 奖励 / 赠送 / 购买积分的最近到期时间。
    pub expires_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreditsUsage {
    pub plan_name: Option<String>,
    pub is_paid: Option<bool>,
    /// 所有分组的总和，用于判断账户是否已开通；展示以 `groups` 为准。
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub used: Option<f64>,
    pub groups: Vec<CreditGroupUsage>,
    pub capability_gaps: Vec<String>,
}

/// 把一次官方接口响应写入前所需的全部展示信息。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkbuddyReadout {
    pub checked_at_epoch: i64,
    pub usage: CreditsUsage,
}

// ---------- HTTP ----------

fn agent() -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = crate::proxy::ureq_proxy_for(ENDPOINT) {
        config = config.proxy(Some(proxy));
    }
    config.build().into()
}

fn request_json(
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Result<Value, WorkbuddyError> {
    let url = format!("{ENDPOINT}{path}");
    let agent = agent();
    let mut response = if method == "GET" {
        let mut request = agent.get(&url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.call()
    } else {
        let mut request = agent.post(&url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request
            .header("Content-Type", "application/json")
            .send(body.unwrap_or_else(|| json!({})).to_string())
    }
    .map_err(|error| match error {
        ureq::Error::StatusCode(401) | ureq::Error::StatusCode(403) => {
            WorkbuddyError::auth("WorkBuddy 授权已失效，请重新授权。")
        }
        ureq::Error::StatusCode(code) => {
            WorkbuddyError::gateway(format!("WorkBuddy 接口返回 HTTP {code}。"))
        }
        other => WorkbuddyError::network(format!("连接 WorkBuddy 失败：{other}")),
    })?;

    let raw = response
        .body_mut()
        .read_to_string()
        .map_err(|err| WorkbuddyError::network(format!("读取 WorkBuddy 响应失败：{err}")))?;
    serde_json::from_str(&raw)
        .map_err(|_| WorkbuddyError::decode("WorkBuddy 接口响应格式无法识别。"))
}

fn unauth_headers() -> [(&'static str, &'static str); 4] {
    [
        ("X-No-Authorization", "true"),
        ("X-No-User-Id", "true"),
        ("X-No-Enterprise-Id", "true"),
        ("X-No-Department-Info", "true"),
    ]
}

fn authorized_headers(credentials: &Credentials) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("Accept", "application/json".to_string()),
        (
            "Authorization",
            format!("Bearer {}", credentials.access_token),
        ),
        ("X-Domain", "www.workbuddy.cn".to_string()),
    ];
    if let Some(user_id) = credentials
        .user_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        headers.push(("X-User-Id", user_id.to_string()));
    }
    if let Some(enterprise_id) = credentials
        .enterprise_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        headers.push(("X-Enterprise-Id", enterprise_id.to_string()));
        headers.push(("X-Tenant-Id", enterprise_id.to_string()));
    }
    headers
}

fn header_refs<'a>(headers: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect()
}

fn code_of(value: &Value) -> Option<i64> {
    match value.get("code")? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn api_data(value: &Value) -> Result<&Value, WorkbuddyError> {
    let code = code_of(value).unwrap_or(0);
    if code != 0 {
        // 401/403 与官方业务码 14015/11140/11142 都表示登录态失效；
        // 其他业务码（如余额耗尽、限流）保留为普通查询错误。
        // 不转述服务端 msg：它可能夹带账号标识，错误消息只保留业务码。
        if matches!(code, 401 | 403 | 11_140 | 11_142 | 14_015) {
            return Err(WorkbuddyError::auth(format!(
                "WorkBuddy 授权已失效，请重新授权。（code {code}）"
            )));
        }
        return Err(WorkbuddyError::gateway(format!(
            "WorkBuddy 接口返回错误（code {code}）。"
        )));
    }
    value
        .get("data")
        .ok_or_else(|| WorkbuddyError::decode("WorkBuddy 接口没有返回数据。"))
}

// ---------- 浏览器授权 ----------

pub struct LoginSession {
    pub login_id: String,
    pub state: String,
    pub auth_url: String,
}

/// 申请一次性登录 state 与官方登录页地址；此时还没有任何账户凭证。
pub fn begin_login() -> Result<LoginSession, WorkbuddyError> {
    let response = request_json("POST", AUTH_STATE_PATH, &unauth_headers(), Some(json!({})))?;
    let data = api_data(&response)?;
    let state = data
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| WorkbuddyError::decode("WorkBuddy 登录接口没有返回 state。"))?;
    let auth_url = data
        .get("authUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| WorkbuddyError::decode("WorkBuddy 登录接口没有返回登录地址。"))?;
    validate_auth_url(auth_url)?;
    Ok(LoginSession {
        login_id: format!("workbuddy-{}", random_hex(8)),
        state: state.to_string(),
        auth_url: auth_url.to_string(),
    })
}

/// 登录页只接受官方 HTTPS 域名；即使接口被劫持也不把任意地址交给系统浏览器。
fn validate_auth_url(auth_url: &str) -> Result<(), WorkbuddyError> {
    let parsed = url::Url::parse(auth_url)
        .map_err(|_| WorkbuddyError::decode("WorkBuddy 登录地址无法解析。"))?;
    if parsed.scheme() != "https" {
        return Err(WorkbuddyError::decode(
            "WorkBuddy 登录地址不是 HTTPS，已拒绝打开。",
        ));
    }
    let host = parsed.host_str().unwrap_or_default();
    if !(host == "www.workbuddy.cn" || host.ends_with(".workbuddy.cn")) {
        return Err(WorkbuddyError::decode(
            "WorkBuddy 登录地址不属于官方域名，已拒绝打开。",
        ));
    }
    Ok(())
}

impl LoginSession {
    /// 轮询官方登录结果。未完成时接口返回 11217；只有拿到 accessToken 才结束。
    pub fn wait_for_token(
        self,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<AuthToken, WorkbuddyError> {
        let started = SystemTime::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(WorkbuddyError::new("canceled", "已取消授权"));
            }
            if started
                .elapsed()
                .map(|elapsed| elapsed >= timeout)
                .unwrap_or(false)
            {
                return Err(WorkbuddyError::new(
                    "timeout",
                    "等待浏览器授权超时，请重试。",
                ));
            }
            std::thread::sleep(LOGIN_POLL_INTERVAL);

            let path = format!("{AUTH_TOKEN_PATH}{}", self.state);
            let response = request_json("GET", &path, &unauth_headers(), None)?;
            if code_of(&response) == Some(TOKEN_PENDING_CODE) {
                continue;
            }
            let data = api_data(&response)?;
            if let Some(token) = parse_auth_token(data) {
                return Ok(token);
            }
            return Err(WorkbuddyError::decode("WorkBuddy 登录结果里没有访问令牌。"));
        }
    }
}

fn parse_auth_token(data: &Value) -> Option<AuthToken> {
    let access_token = data
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let refresh_token = data
        .get("refreshToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let now = epoch_now();
    let access_expires_at_epoch = expiry_epoch(data, "expiresAt", "expiresIn", now);
    let refresh_expires_at_epoch = expiry_epoch(data, "refreshExpiresAt", "refreshExpiresIn", now);
    Some(AuthToken {
        access_token,
        refresh_token,
        access_expires_at_epoch,
        refresh_expires_at_epoch,
    })
}

/// 官方接口优先返回毫秒时间戳；缺少时用 `expiresIn` 秒数换算。
fn expiry_epoch(data: &Value, at_key: &str, in_key: &str, now: i64) -> Option<i64> {
    if let Some(millis) = as_i64(data.get(at_key)) {
        if millis > 0 {
            return Some(millis.div_euclid(1000));
        }
    }
    as_i64(data.get(in_key))
        .filter(|seconds| *seconds > 0)
        .map(|seconds| now.saturating_add(seconds))
}

/// 换取新令牌。失败按授权失效处理，由账户页引导重新授权。
pub fn refresh_token(refresh_token: &str) -> Result<AuthToken, WorkbuddyError> {
    let token = refresh_token.trim();
    if token.is_empty() {
        return Err(WorkbuddyError::auth("WorkBuddy 授权已失效，请重新授权。"));
    }
    let headers = [
        ("Accept", "application/json".to_string()),
        ("X-Refresh-Token", token.to_string()),
        ("X-Auth-Refresh-Source", "plugin".to_string()),
        ("X-Domain", "www.workbuddy.cn".to_string()),
    ];
    let response = request_json(
        "POST",
        TOKEN_REFRESH_PATH,
        &header_refs(&headers),
        Some(json!({})),
    )?;
    let data = api_data(&response)?;
    parse_auth_token(data)
        .ok_or_else(|| WorkbuddyError::decode("WorkBuddy 续期结果里没有访问令牌。"))
}

/// 登录完成后读取当前账户的昵称与请求头需要的标识；失败不影响授权本身。
pub fn fetch_account(state: &str, access_token: &str) -> Option<AccountIdentity> {
    let path = format!("{LOGIN_ACCOUNT_PATH}{state}");
    for attempt in 0..LOGIN_ACCOUNT_RETRIES {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
        let response = match request_json(
            "GET",
            &path,
            &[
                ("Authorization", &format!("Bearer {access_token}")),
                ("X-No-User-Id", "true"),
                ("X-No-Enterprise-Id", "true"),
                ("X-No-Department-Info", "true"),
                ("X-Domain", "www.workbuddy.cn"),
            ],
            None,
        ) {
            Ok(response) => response,
            // 刚拿到的令牌可能还没同步到账户接口，重试即可。
            Err(_) => continue,
        };
        if code_of(&response) == Some(ACCOUNT_PENDING_CODE) {
            continue;
        }
        let Ok(data) = api_data(&response) else {
            continue;
        };
        return Some(parse_account_identity(data));
    }
    None
}

/// 只保留适合展示的昵称。纯数字（通常是 UID）与邮箱不进入账户名；
/// UID / 企业 ID 仅用于后续请求头。
fn parse_account_identity(data: &Value) -> AccountIdentity {
    let nickname = data
        .get("nickname")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && !value.chars().all(|c| c.is_ascii_digit())
                // 昵称可能是邮箱；邮箱原文不得进入状态或界面。
                && !value.contains('@')
        })
        .map(str::to_string);
    AccountIdentity {
        nickname,
        user_id: data
            .get("uid")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        enterprise_id: data
            .get("enterpriseId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }
}

// ---------- 额度 ----------

/// 读取 Credits 分组额度与各自的重置 / 到期时间。摘要接口失败直接返回错误；
/// 分页接口只用于补充时间，失败时保留余额并记录能力缺口。
pub fn read_usage(credentials: &Credentials) -> Result<WorkbuddyReadout, WorkbuddyError> {
    let headers = authorized_headers(credentials);
    let response = request_json(
        "POST",
        SUMMARY_PATH,
        &header_refs(&headers),
        Some(json!({})),
    )?;
    let data = api_data(&response)?;
    let (mut usage, package_codes) = parse_summary(data);

    if !package_codes.is_empty() {
        match read_group_resets(credentials, &package_codes) {
            Ok(resets) => {
                for group in &mut usage.groups {
                    let time = resets.get(&group.kind).copied();
                    // 套餐基础是周期刷新；奖励 / 赠送 / 购买积分是最近到期。
                    if group.kind == "plan_base" {
                        group.resets_at_epoch = time;
                    } else {
                        group.expires_at_epoch = time;
                    }
                    if time.is_none() && group.total.is_some_and(|total| total > 0.0) {
                        let gap = if group.kind == "plan_base" {
                            format!("未能读取「{}」的重置时间", group.name)
                        } else {
                            format!("未能读取「{}」的到期时间", group.name)
                        };
                        push_gap(&mut usage.capability_gaps, &gap);
                    }
                }
            }
            Err(_) => push_gap(&mut usage.capability_gaps, "未能读取 Credits 时间信息"),
        }
    }

    Ok(WorkbuddyReadout {
        checked_at_epoch: epoch_now(),
        usage,
    })
}

/// 解析计费摘要。分组以官方 `CreditGroups` 为准；缺少分组时回退到套餐汇总，
/// 避免旧接口或字段变化时整块额度消失。
fn parse_summary(data: &Value) -> (CreditsUsage, Vec<String>) {
    let mut capability_gaps = Vec::new();
    let mut package_total = 0.0;
    let mut package_remaining = 0.0;
    let mut package_used = 0.0;
    let mut included = 0usize;
    let mut total_complete = true;
    let mut remaining_complete = true;
    let mut used_present = false;
    let mut used_complete = true;
    let mut package_codes = Vec::new();

    if let Some(packages) = data.get("Packages").and_then(Value::as_array) {
        for package in packages {
            let unit = package
                .get("CapacityUnit")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase);
            if unit.as_deref() != Some("credits") {
                continue;
            }
            included += 1;
            if let Some(code) = package
                .get("PackageCode")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !package_codes.iter().any(|existing| existing == code) {
                    package_codes.push(code.to_string());
                }
            }
            match package_number(
                package,
                &[
                    "CycleTotalCapacity",
                    "CycleCapacitySizePrecise",
                    "CycleCapacitySize",
                ],
            ) {
                Some(value) => package_total += value,
                None => total_complete = false,
            }
            match package_number(
                package,
                &[
                    "CycleRemainCapacity",
                    "CycleCapacityRemainPrecise",
                    "CycleCapacityRemain",
                ],
            ) {
                Some(value) => package_remaining += value,
                None => remaining_complete = false,
            }
            match package_number(
                package,
                &[
                    "CycleUsedCapacity",
                    "CycleCapacityUsedPrecise",
                    "CycleCapacityUsed",
                ],
            ) {
                Some(value) => {
                    used_present = true;
                    package_used += value;
                }
                None => used_complete = false,
            }
        }
    }

    let package_total = if included == 0 {
        Some(0.0)
    } else {
        total_complete.then_some(package_total)
    };
    let package_remaining = if included == 0 {
        Some(0.0)
    } else {
        remaining_complete.then_some(package_remaining)
    };
    let package_used = if included == 0 {
        Some(0.0)
    } else if used_present && used_complete {
        Some(package_used)
    } else {
        match (package_total, package_remaining) {
            (Some(total), Some(remaining)) => Some((total - remaining).max(0.0)),
            _ => None,
        }
    };

    let mut groups = parse_credit_groups(data);
    if groups.is_empty() {
        if included == 0 {
            push_gap(&mut capability_gaps, "未找到有效的 Credits 套餐");
        }
        groups.push(CreditGroupUsage {
            kind: "credits".to_string(),
            name: "Credits".to_string(),
            total: package_total,
            remaining: package_remaining,
            used: package_used,
            resets_at_epoch: None,
            expires_at_epoch: None,
        });
    }

    for group in &groups {
        if group.total.is_none() {
            let gap = format!("「{}」未返回 Credits 总量", group.name);
            push_gap(&mut capability_gaps, &gap);
        }
        if group.remaining.is_none() {
            let gap = format!("「{}」未返回 Credits 剩余量", group.name);
            push_gap(&mut capability_gaps, &gap);
        }
    }

    let total = sum_options(groups.iter().map(|group| group.total));
    let remaining = sum_options(groups.iter().map(|group| group.remaining));
    let used = sum_options(groups.iter().map(|group| group.used));

    let plan_name = data
        .get("SubscriptionPackageName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let is_paid = data.get("IsPaidUser").and_then(|value| {
        value
            .as_bool()
            .or_else(|| value.as_i64().map(|number| number != 0))
    });

    (
        CreditsUsage {
            plan_name,
            is_paid,
            total,
            remaining,
            used,
            groups,
            capability_gaps,
        },
        package_codes,
    )
}

/// 官方 `CreditGroups` 已经按套餐基础 / 赠送 / 购买 / 奖励聚合好；
/// 全零分组（例如未购买加油包）不展示。
fn parse_credit_groups(data: &Value) -> Vec<CreditGroupUsage> {
    let Some(items) = data.get("CreditGroups").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    for item in items {
        let Some(kind) = item
            .get("Kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let total = package_number(
            item,
            &[
                "CycleTotalCapacity",
                "CycleCapacitySizePrecise",
                "CycleCapacitySize",
            ],
        );
        let remaining = package_number(
            item,
            &[
                "CycleRemainCapacity",
                "CycleCapacityRemainPrecise",
                "CycleCapacityRemain",
            ],
        );
        let used = package_number(
            item,
            &[
                "CycleUsedCapacity",
                "CycleCapacityUsedPrecise",
                "CycleCapacityUsed",
            ],
        );
        let has_value = [total, remaining, used]
            .into_iter()
            .flatten()
            .any(|value| value > 0.0);
        if !has_value {
            continue;
        }
        groups.push(CreditGroupUsage {
            kind: kind.to_string(),
            name: group_display_name(kind, item.get("Name").and_then(Value::as_str)),
            total,
            remaining,
            used,
            resets_at_epoch: None,
            expires_at_epoch: None,
        });
    }
    groups
}

fn group_display_name(kind: &str, name: Option<&str>) -> String {
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    match kind {
        "plan_base" => "套餐基础积分",
        "plan_bonus" => "套餐赠送积分",
        "addon" => "购买积分",
        "benefit" => "平台奖励积分",
        _ => "Credits",
    }
    .to_string()
}

fn sum_options(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut sum = 0.0;
    let mut any = false;
    for value in values {
        let value = value?;
        any = true;
        sum += value;
    }
    any.then_some(sum)
}

fn push_gap(gaps: &mut Vec<String>, gap: &str) {
    if !gaps.iter().any(|existing| existing == gap) {
        gaps.push(gap.to_string());
    }
}

/// 计费摘要与旧版资源接口使用不同字段名；按优先级取第一个可解析的值。
fn package_number(package: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| as_f64(package.get(*key)))
}

/// 读取每个分组的重置时间。套餐基础取“下次刷新”，其余分组取最近一次到期。
fn read_group_resets(
    credentials: &Credentials,
    package_codes: &[String],
) -> Result<std::collections::HashMap<String, i64>, WorkbuddyError> {
    let body = json!({
        "PageNumber": 1,
        "PageSize": 100,
        "PackageCodes": package_codes,
        "Status": [0, 3],
    });
    let now = epoch_now();
    let mut resets = std::collections::HashMap::new();
    let mut any_success = false;
    for path in [PAID_PACKAGES_PATH, FREE_PACKAGES_PATH] {
        let headers = authorized_headers(credentials);
        let response = match request_json("POST", path, &header_refs(&headers), Some(body.clone()))
        {
            Ok(response) => response,
            Err(error) if error.is_auth_error() => return Err(error),
            Err(_) => continue,
        };
        let Ok(data) = api_data(&response) else {
            continue;
        };
        any_success = true;
        if let Some(accounts) = data.get("Accounts").and_then(Value::as_array) {
            merge_group_resets(&mut resets, accounts, now);
        }
    }
    if any_success {
        Ok(resets)
    } else {
        Err(WorkbuddyError::gateway("读取 WorkBuddy 套餐周期失败。"))
    }
}

fn merge_group_resets(
    resets: &mut std::collections::HashMap<String, i64>,
    accounts: &[Value],
    now: i64,
) {
    for account in accounts {
        let Some(code) = account.get("PackageCode").and_then(Value::as_str) else {
            continue;
        };
        let Some(kind) = credit_group_kind(code) else {
            continue;
        };
        let Some(end) = account
            .get("CycleEndTime")
            .and_then(Value::as_str)
            .and_then(parse_cst_datetime)
        else {
            continue;
        };
        // 与 WorkBuddy 客户端一致，展示平台返回的周期结束 / 最近到期时间。
        let reset = end;
        if reset <= now {
            continue;
        }
        match resets.get(kind) {
            Some(current) if *current <= reset => {}
            _ => {
                resets.insert(kind.to_string(), reset);
            }
        }
    }
}

/// 官方客户端按套餐编码把资源归入四个分组。这里沿用同一套编码；
/// 未知编码不参与时间计算，分组额度仍以 `CreditGroups` 为准。
fn credit_group_kind(package_code: &str) -> Option<&'static str> {
    let number = package_code
        .strip_prefix("TCACA_code_")?
        .split('_')
        .next()?
        .parse::<u32>()
        .ok()?;
    match number {
        2 | 3 | 5 | 8 | 23 | 26 | 27 | 35 | 39 | 40 | 43 => Some("plan_base"),
        28 | 37 => Some("plan_bonus"),
        9 | 36 | 38 => Some("addon"),
        7 | 29 | 30 => Some("benefit"),
        _ => None,
    }
}

/// 官方时间字段是不带时区的 `yyyy-MM-dd HH:mm:ss`，按中国标准时间（UTC+8）解释。
fn parse_cst_datetime(text: &str) -> Option<i64> {
    let format = time::format_description::parse_borrowed::<3>(
        "[year]-[month]-[day] [hour]:[minute]:[second]",
    )
    .ok()?;
    let datetime = time::PrimitiveDateTime::parse(text.trim(), &format).ok()?;
    let offset = time::UtcOffset::from_hms(8, 0, 0).ok()?;
    Some(datetime.assume_offset(offset).unix_timestamp())
}

fn as_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn as_f64(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        _ => None,
    }
}

fn epoch_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if getrandom::fill(&mut buffer).is_err() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = (nanos.rotate_left(index as u32 % 64) as u8) ^ (index as u8);
        }
    }
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_response() -> Value {
        json!({
            "code": 0,
            "msg": "OK",
            "data": {
                "SubscriptionPackageName": "标准版",
                "IsPaidUser": true,
                "Packages": [
                    {
                        "PackageCode": "plan-base",
                        "CapacityUnit": "credits",
                        "CycleTotalCapacity": "2000.5",
                        "CycleRemainCapacity": "1500.25",
                        "CycleUsedCapacity": "500.25",
                        "CycleFrozenCapacity": "10"
                    },
                    {
                        "PackageCode": "plan-bonus",
                        "CapacityUnit": "credits",
                        "CycleTotalCapacity": "500",
                        "CycleRemainCapacity": "500",
                        "CycleUsedCapacity": "0",
                        "CycleFrozenCapacity": "0"
                    },
                    {
                        "PackageCode": "ignored",
                        "CapacityUnit": "tokens",
                        "CycleTotalCapacity": "99999",
                        "CycleRemainCapacity": "99999"
                    }
                ],
                "CreditGroups": [
                    {
                        "Kind": "plan_base",
                        "Name": "套餐基础积分",
                        "CycleTotalCapacity": "2000.5",
                        "CycleRemainCapacity": "1500.25",
                        "CycleUsedCapacity": "500.25"
                    },
                    {
                        "Kind": "plan_bonus",
                        "Name": "套餐赠送积分",
                        "CycleTotalCapacity": "500",
                        "CycleRemainCapacity": "500",
                        "CycleUsedCapacity": "0"
                    },
                    {
                        "Kind": "addon",
                        "Name": "购买积分",
                        "CycleTotalCapacity": "0",
                        "CycleRemainCapacity": "0",
                        "CycleUsedCapacity": "0"
                    }
                ]
            }
        })
    }

    #[test]
    fn auth_token_prefers_millisecond_expiry_and_trims_tokens() {
        let token = parse_auth_token(&json!({
            "accessToken": " access ",
            "refreshToken": " refresh ",
            "expiresAt": 1_800_000_000_000_i64,
            "refreshExpiresAt": 1_900_000_000_000_i64,
        }))
        .unwrap();
        assert_eq!(token.access_token, "access");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(token.access_expires_at_epoch, Some(1_800_000_000));
        assert_eq!(token.refresh_expires_at_epoch, Some(1_900_000_000));
    }

    #[test]
    fn auth_token_falls_back_to_expires_in_seconds() {
        let now = epoch_now();
        let token = parse_auth_token(&json!({
            "accessToken": "access",
            "expiresIn": 3600,
            "refreshExpiresIn": 7200,
        }))
        .unwrap();
        assert!(token.access_expires_at_epoch.unwrap() >= now + 3599);
        assert!(token.refresh_expires_at_epoch.unwrap() >= now + 7199);
    }

    #[test]
    fn empty_refresh_token_is_not_treated_as_refreshable() {
        let credentials = Credentials {
            access_token: "access".to_string(),
            refresh_token: Some("  ".to_string()),
            access_expires_at_epoch: Some(0),
            refresh_expires_at_epoch: None,
            issued_at_epoch: None,
            user_id: None,
            enterprise_id: None,
        };
        assert!(!credentials.needs_refresh(epoch_now()));
    }

    #[test]
    fn account_identity_drops_emails_and_numeric_nicknames() {
        let named = parse_account_identity(&json!({
            "nickname": "小明",
            "uid": "1234567890",
            "enterpriseId": "ent-1"
        }));
        assert_eq!(named.nickname.as_deref(), Some("小明"));
        assert_eq!(named.user_id.as_deref(), Some("1234567890"));
        assert_eq!(named.enterprise_id.as_deref(), Some("ent-1"));

        let email = parse_account_identity(&json!({ "nickname": "a@example.com" }));
        assert_eq!(email.nickname, None);

        let numeric = parse_account_identity(&json!({ "nickname": "1234567890" }));
        assert_eq!(numeric.nickname, None);
    }

    #[test]
    fn auth_business_codes_are_classified_as_auth_expired() {
        for code in [401, 403, 11_140, 11_142, 14_015] {
            let error = api_data(&json!({ "code": code, "msg": "expired" })).unwrap_err();
            assert!(error.is_auth_error(), "code {code} should require reauth");
        }
        let quota = api_data(&json!({ "code": 14_001, "msg": "balance exhausted" })).unwrap_err();
        assert!(!quota.is_auth_error());
    }

    #[test]
    fn pending_login_codes_are_recognized() {
        assert_eq!(TOKEN_PENDING_CODE, 11217);
        assert_eq!(ACCOUNT_PENDING_CODE, 12151);
    }

    #[test]
    fn login_url_must_be_official_https() {
        assert!(validate_auth_url("https://www.workbuddy.cn/login?state=x").is_ok());
        assert!(validate_auth_url("http://www.workbuddy.cn/login?state=x").is_err());
        assert!(validate_auth_url("https://evil.example/login?state=x").is_err());
    }

    #[test]
    fn cst_datetime_is_parsed_as_utc_plus_eight() {
        // 2026-11-01 08:00:00 UTC+8 = 2026-11-01 00:00:00 UTC
        assert_eq!(
            parse_cst_datetime("2026-11-01 08:00:00"),
            Some(1_793_491_200)
        );
        assert_eq!(parse_cst_datetime("not a date"), None);
    }

    #[test]
    fn summary_parses_credit_groups_and_plan() {
        let (usage, codes) = parse_summary(api_data(&summary_response()).unwrap());
        assert_eq!(usage.total, Some(2_500.5));
        assert_eq!(usage.remaining, Some(2_000.25));
        assert_eq!(usage.used, Some(500.25));
        assert_eq!(usage.plan_name.as_deref(), Some("标准版"));
        assert_eq!(usage.is_paid, Some(true));
        assert_eq!(codes, vec!["plan-base", "plan-bonus"]);
        assert!(usage.capability_gaps.is_empty());
        assert_eq!(usage.groups.len(), 2, "全零分组不展示");
        assert_eq!(usage.groups[0].kind, "plan_base");
        assert_eq!(usage.groups[0].name, "套餐基础积分");
        assert_eq!(usage.groups[0].total, Some(2_000.5));
        assert_eq!(usage.groups[1].kind, "plan_bonus");
        assert_eq!(usage.groups[1].remaining, Some(500.0));
    }

    #[test]
    fn summary_falls_back_to_one_group_without_credit_groups() {
        let value = json!({
            "code": 0,
            "data": {
                "Packages": [{
                    "PackageCode": "ignored",
                    "CapacityUnit": "credits",
                    "CycleTotalCapacity": "100",
                    "CycleRemainCapacity": "40",
                    "CycleUsedCapacity": "60"
                }]
            }
        });
        let (usage, _) = parse_summary(api_data(&value).unwrap());
        assert_eq!(usage.groups.len(), 1);
        assert_eq!(usage.groups[0].kind, "credits");
        assert_eq!(usage.groups[0].total, Some(100.0));
        assert_eq!(usage.groups[0].remaining, Some(40.0));
    }

    #[test]
    fn package_codes_map_to_the_same_groups_as_the_official_client() {
        assert_eq!(
            credit_group_kind("TCACA_code_008_cfWoLwvjU4"),
            Some("plan_base")
        );
        assert_eq!(
            credit_group_kind("TCACA_code_002_AkiJS3ZHF5"),
            Some("plan_base")
        );
        assert_eq!(
            credit_group_kind("TCACA_code_028_NtpWi0jzXs"),
            Some("plan_bonus")
        );
        assert_eq!(
            credit_group_kind("TCACA_code_009_0XmEQc2xOf"),
            Some("addon")
        );
        assert_eq!(
            credit_group_kind("TCACA_code_007_nzdH5h4Nl0"),
            Some("benefit")
        );
        assert_eq!(
            credit_group_kind("TCACA_code_030_BjSt89qTvr"),
            Some("benefit")
        );
        assert_eq!(credit_group_kind("TCACA_code_001_PqouKr6QWV"), None);
        assert_eq!(credit_group_kind("not-a-code"), None);
    }

    #[test]
    fn group_resets_use_the_nearest_future_end_per_group() {
        let mut resets = std::collections::HashMap::new();
        let accounts = vec![
            json!({
                "PackageCode": "TCACA_code_008_cfWoLwvjU4",
                "CycleEndTime": "2026-11-30 23:59:59"
            }),
            json!({
                "PackageCode": "TCACA_code_007_nzdH5h4Nl0",
                "CycleEndTime": "2026-11-08 09:00:02"
            }),
            json!({
                "PackageCode": "TCACA_code_030_BjSt89qTvr",
                "CycleEndTime": "2026-11-10 13:29:11"
            }),
            json!({
                "PackageCode": "TCACA_code_001_PqouKr6QWV",
                "CycleEndTime": "2026-11-01 00:00:00"
            }),
        ];
        merge_group_resets(&mut resets, &accounts, 1_793_491_200);
        // 2026-11-30 23:59:59
        assert_eq!(resets.get("plan_base"), Some(&1_796_054_399));
        // 最近一次奖励到期：2026-11-08 09:00:02
        assert_eq!(resets.get("benefit"), Some(&1_794_099_602));
        assert!(!resets.contains_key("plan_bonus"));
    }

    #[test]
    fn legacy_resource_fields_are_accepted() {
        let value = json!({
            "code": 0,
            "data": {
                "Packages": [{
                    "CapacityUnit": "credits",
                    "CycleCapacitySizePrecise": "100",
                    "CycleCapacityRemainPrecise": "40",
                    "CycleCapacityUsedPrecise": "60"
                }]
            }
        });
        let (usage, _) = parse_summary(api_data(&value).unwrap());
        assert_eq!(usage.total, Some(100.0));
        assert_eq!(usage.remaining, Some(40.0));
        assert_eq!(usage.used, Some(60.0));
    }

    #[test]
    fn summary_without_credit_packages_is_zero_with_a_gap() {
        let value = json!({
            "code": 0,
            "data": {
                "Packages": [
                    { "CapacityUnit": "tokens", "CycleTotalCapacity": "10" }
                ]
            }
        });
        let (usage, codes) = parse_summary(api_data(&value).unwrap());
        assert_eq!(usage.total, Some(0.0));
        assert_eq!(usage.remaining, Some(0.0));
        assert_eq!(usage.used, Some(0.0));
        assert!(codes.is_empty());
        assert!(usage
            .capability_gaps
            .iter()
            .any(|gap| gap.contains("未找到有效的 Credits 套餐")));
    }

    #[test]
    fn partial_package_fields_stay_unknown_instead_of_zero() {
        let value = json!({
            "code": 0,
            "data": {
                "Packages": [
                    { "CapacityUnit": "credits", "CycleRemainCapacity": "10" }
                ]
            }
        });
        let (usage, _) = parse_summary(api_data(&value).unwrap());
        assert_eq!(usage.total, None);
        assert_eq!(usage.remaining, Some(10.0));
        assert_eq!(usage.used, None);
        assert!(usage.capability_gaps.iter().any(|gap| gap.contains("总量")));
    }

    #[test]
    fn numbers_accept_string_and_number_forms() {
        assert_eq!(as_f64(Some(&json!("12.5"))), Some(12.5));
        assert_eq!(as_f64(Some(&json!(12.5))), Some(12.5));
        assert_eq!(as_f64(Some(&json!("bad"))), None);
        assert_eq!(as_i64(Some(&json!("11217"))), Some(11217));
    }

    #[test]
    fn credentials_round_trip_through_private_directory() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-workbuddy-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let credentials = Credentials {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            access_expires_at_epoch: Some(1_800_000_000),
            refresh_expires_at_epoch: Some(1_900_000_000),
            issued_at_epoch: Some(1_700_000_000),
            user_id: Some("uid".to_string()),
            enterprise_id: None,
        };
        save_credentials(&dir, &credentials).unwrap();
        assert_eq!(load_credentials(&dir).unwrap(), credentials);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(credentials_path(&dir))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_credentials_are_reported_as_auth_expired() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-workbuddy-missing-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let error = load_credentials(&dir).unwrap_err();
        assert!(error.is_auth_error());
    }
}
