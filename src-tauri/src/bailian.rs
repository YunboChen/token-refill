//! 阿里云百炼（Model Studio / DashScope）额度只读适配器。
//!
//! Token Plan 个人版与 Coding Plan 中国站的订阅额度只能通过控制台会话读取，
//! 推理 API Key 不能代替。这里复用百炼 CLI 使用的控制台网关协议：
//! 浏览器完成阿里云登录后，把 `access_token` 回传到本机回环地址；应用只把
//! 令牌写入自己的私有目录，不进入前端状态、事件或日志。
//!
//! 网关请求全部在 Rust 侧完成。解析时按字段名做有限递归查找，避免把某一层
//! 包装结构写死；字段缺失保持未知，不用 0 补。

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 中国站控制台网关；用户已确认只接入国内站。
const CONSOLE_GATEWAY: &str = "https://bailian-cs.console.aliyun.com";
const CONSOLE_ACTION: &str = "BroadScopeAspnGateway";
const CONSOLE_REGION: &str = "cn-beijing";
const CONSOLE_LOGIN_BASE: &str = "https://bailian.console.aliyun.com";
const TOKEN_PLAN_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";
const TOKEN_PLAN_SUBSCRIPTION_API: &str =
    "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription";
const TOKEN_PLAN_COMMODITY: &str = "sfm_tokenplansolo_public_cn";
const LOGIN_INFO_API: &str = "zeldaEasy.cornerstone-portal.cs-console.loginInfo";
const CODING_PLAN_API: &str =
    "zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2";
const CODING_PLAN_COMMODITY: &str = "sfm_codingplan_public_cn";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CALLBACK_TIMEOUT_READ: Duration = Duration::from_secs(3);
const CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(120);
const MAX_CALLBACK_BYTES: usize = 64 * 1024;
const CREDENTIALS_FILE: &str = "credentials.json";

// ---------- 错误与凭证 ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BailianError {
    pub code: String,
    pub message: String,
}

impl BailianError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub(crate) fn auth(message: impl Into<String>) -> Self {
        Self::new("auth-expired", message)
    }

    pub(crate) fn network(message: impl Into<String>) -> Self {
        Self::new("network", message)
    }

    pub(crate) fn gateway(message: impl Into<String>) -> Self {
        Self::new("gateway", message)
    }

    pub(crate) fn decode(message: impl Into<String>) -> Self {
        Self::new("decode", message)
    }

    pub fn is_auth_error(&self) -> bool {
        self.code == "auth-expired"
    }
}

/// 只保存控制台会话令牌；不保存账号密码、API Key 或 Cookie。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
}

pub fn credentials_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(CREDENTIALS_FILE)
}

/// 写入隔离目录下的凭证文件。Unix 使用 0700 目录 + 0600 文件；
/// Windows 继承应用数据目录的按用户 ACL。先写临时文件再重命名，避免半截 JSON。
pub fn save_credentials(profile_dir: &Path, access_token: &str) -> Result<(), String> {
    let token = access_token.trim();
    if token.is_empty() {
        return Err("控制台没有返回有效的访问令牌".to_string());
    }
    fs::create_dir_all(profile_dir).map_err(|err| format!("创建百炼授权目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(profile_dir, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置百炼授权目录权限失败：{err}"))?;
    }

    let body = serde_json::to_vec(&Credentials {
        access_token: token.to_string(),
    })
    .map_err(|err| format!("序列化百炼凭证失败：{err}"))?;
    let path = credentials_path(profile_dir);
    let temp = profile_dir.join(format!("{CREDENTIALS_FILE}.tmp"));
    {
        let mut file =
            fs::File::create(&temp).map_err(|err| format!("创建百炼凭证文件失败：{err}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("设置百炼凭证权限失败：{err}"))?;
        }
        file.write_all(&body)
            .map_err(|err| format!("写入百炼凭证失败：{err}"))?;
        file.sync_all()
            .map_err(|err| format!("保存百炼凭证失败：{err}"))?;
    }
    #[cfg(windows)]
    {
        // Windows 的 rename 不覆盖已存在文件；重新授权时需要先移除旧凭证。
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("替换百炼凭证失败：{err}"))?;
        }
    }
    fs::rename(&temp, &path).map_err(|err| format!("保存百炼凭证失败：{err}"))
}

pub fn load_credentials(profile_dir: &Path) -> Result<Credentials, BailianError> {
    let path = credentials_path(profile_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|_| BailianError::auth("未找到百炼授权，请重新授权。"))?;
    let mut credentials: Credentials = serde_json::from_str(&raw)
        .map_err(|_| BailianError::auth("百炼授权文件无法读取，请重新授权。"))?;
    credentials.access_token = credentials.access_token.trim().to_string();
    if credentials.access_token.is_empty() {
        return Err(BailianError::auth("百炼授权已失效，请重新授权。"));
    }
    Ok(credentials)
}

// ---------- 额度模型 ----------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenPlanUsage {
    /// 平台以 0..1 的比例返回；这里保留原始已用百分比。
    pub used_percent: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodingPlanWindow {
    pub used: Option<i64>,
    pub total: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodingPlanUsage {
    /// 没有有效订阅时保留默认值，`found` 为 false。
    pub found: bool,
    pub instance_type: Option<String>,
    pub per_5_hour: CodingPlanWindow,
    pub per_week: CodingPlanWindow,
    pub per_bill_month: CodingPlanWindow,
}

/// Token Plan 订阅信息。个人版以 `specCode` 区分 Lite / Essential /
/// Standard / Pro 档位；没有有效订阅时 `active` 为 false。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenPlanSubscription {
    pub active: bool,
    pub spec_code: Option<String>,
    pub status: Option<String>,
    pub remaining_days: Option<i64>,
    pub end_time_epoch: Option<i64>,
}

/// 控制台登录信息里的账号提示。`nick_name` 可能是用户设置的昵称，
/// 也可能退化成长度为 16 位的数字 UID。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountIdentity {
    pub uid: Option<String>,
    pub nick_name: Option<String>,
}

impl AccountIdentity {
    /// 真实昵称；纯数字（通常是 UID）不算昵称。
    pub fn nickname(&self) -> Option<&str> {
        self.nick_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()))
    }

    /// 只返回适合直接展示的提示：昵称与 UID 相同时仍用脱敏 UID。
    pub fn hint(&self) -> Option<String> {
        if let Some(nick_name) = self.nickname() {
            return Some(nick_name.to_string());
        }
        self.uid.as_deref().map(mask_uid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BailianReadout {
    pub checked_at_epoch: i64,
    pub token_plan: Result<TokenPlanUsage, BailianError>,
    pub coding_plan: Result<CodingPlanUsage, BailianError>,
    /// 订阅信息接口失败时为 None；失败不影响额度读取。
    pub token_plan_subscription: Option<TokenPlanSubscription>,
    /// 控制台登录信息接口失败时为 None。
    pub identity: Option<AccountIdentity>,
}

// ---------- 控制台网关 ----------

/// 网关要求为每个业务 API 拼一层 `cornerstoneParam`；与实际控制台请求保持一致。
fn request_params(api: &str, data: Value) -> String {
    let mut data = if data.is_object() { data } else { json!({}) };
    data["cornerstoneParam"] = json!({
        "protocol": "V2",
        "console": "ONE_CONSOLE",
        "productCode": "p_efm",
        "switchUserType": 3,
        "consoleSite": "BAILIAN_ALIYUN",
    });
    json!({
        "Api": api,
        "V": "1.0",
        "Data": data,
    })
    .to_string()
}

/// RFC 3986 表单编码，避免额外引入 URL 依赖。
fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push_str("%20"),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn call_console(token: &str, api: &str, data: Value) -> Result<Value, BailianError> {
    call_console_at(CONSOLE_GATEWAY, token, api, data)
}

fn call_console_at(
    gateway: &str,
    token: &str,
    api: &str,
    data: Value,
) -> Result<Value, BailianError> {
    let body = format!(
        "params={}&region={}",
        encode_component(&request_params(api, data)),
        encode_component(CONSOLE_REGION)
    );
    let url = format!(
        "{gateway}/cli/api.json?action={CONSOLE_ACTION}&product=sfm_bailian&api={}",
        encode_component(api)
    );
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent: ureq::Agent = config.into();

    let mut response = agent
        .post(&url)
        .header("Accept", "*/*")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Authorization", &format!("Bearer {token}"))
        .send(body)
        .map_err(|error| match error {
            ureq::Error::StatusCode(401) => {
                BailianError::auth("百炼控制台会话已失效，请重新授权。")
            }
            ureq::Error::StatusCode(403) => {
                BailianError::gateway("百炼控制台拒绝访问（HTTP 403），请确认账号权限。")
            }
            ureq::Error::StatusCode(code) => {
                BailianError::gateway(format!("百炼控制台返回 HTTP {code}。"))
            }
            other => BailianError::network(format!("连接百炼控制台失败：{other}")),
        })?;

    let raw = response
        .body_mut()
        .read_to_string()
        .map_err(|err| BailianError::network(format!("读取百炼控制台响应失败：{err}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|_| BailianError::decode("百炼控制台响应格式无法识别。"))?;

    if let Some(false) = value.get("successResponse").and_then(Value::as_bool) {
        return Err(BailianError::gateway(format!(
            "百炼控制台请求失败：{}",
            gateway_error_message(&value)
        )));
    }
    if let Some(false) = value
        .get("data")
        .and_then(|data| data.get("success"))
        .and_then(Value::as_bool)
    {
        let code = value
            .get("data")
            .and_then(|data| data.get("errorCode"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let message = value
            .get("data")
            .and_then(|data| data.get("errorMsg"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if is_not_logged_in(&code, &message) || raw.contains("NotLogined") {
            return Err(BailianError::auth("百炼控制台会话已失效，请重新授权。"));
        }
        return Err(BailianError::gateway(format!(
            "百炼控制台返回错误：{}",
            if message.is_empty() { code } else { message }
        )));
    }
    Ok(value)
}

fn gateway_error_message(value: &Value) -> String {
    value
        .get("data")
        .and_then(|data| {
            data.get("errorMsg")
                .or_else(|| data.get("errorCode"))
                .or_else(|| data.get("message"))
        })
        .and_then(Value::as_str)
        .filter(|message| !message.is_empty())
        .unwrap_or("未知错误")
        .to_string()
}

fn is_not_logged_in(code: &str, message: &str) -> bool {
    let haystack = format!("{code} {message}").to_ascii_lowercase();
    haystack.contains("notlogined")
        || haystack.contains("not login")
        || haystack.contains("notlogin")
        || haystack.contains("session expired")
        || haystack.contains("sessionexpired")
}

/// 按字段名在响应中做有限递归查找；找到第一个匹配即可。
fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|child| find_key(child, key))),
        Value::Array(items) => items.iter().find_map(|child| find_key(child, key)),
        _ => None,
    }
}

fn as_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|float| float.is_finite())
                .map(|float| float.round() as i64)
        }),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn millis_to_epoch(value: Option<&Value>) -> Option<i64> {
    as_i64(value).map(|millis| millis.div_euclid(1000))
}

fn parse_token_plan(value: &Value) -> Result<TokenPlanUsage, BailianError> {
    let percentage = find_key(value, "per1MonthPercentage")
        .and_then(|raw| {
            raw.as_f64().or_else(|| {
                raw.as_str()
                    .and_then(|text| text.trim().parse::<f64>().ok())
            })
        })
        .map(|ratio| (ratio * 100.0).round().clamp(0.0, 100.0) as i64);
    let resets_at_epoch = millis_to_epoch(find_key(value, "per1MonthResetTime"));
    Ok(TokenPlanUsage {
        used_percent: percentage,
        resets_at_epoch,
    })
}

fn coding_window(value: &Value, prefix: &str) -> CodingPlanWindow {
    CodingPlanWindow {
        used: as_i64(find_key(value, &format!("{prefix}UsedQuota"))),
        total: as_i64(find_key(value, &format!("{prefix}TotalQuota"))),
        resets_at_epoch: millis_to_epoch(find_key(value, &format!("{prefix}QuotaNextRefreshTime"))),
    }
}

fn parse_coding_plan(value: &Value) -> Result<CodingPlanUsage, BailianError> {
    let Some(instances) = find_key(value, "codingPlanInstanceInfos").and_then(Value::as_array)
    else {
        return Ok(CodingPlanUsage::default());
    };
    let Some(instance) = instances.iter().find(|item| {
        item.get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status == "VALID")
    }) else {
        return Ok(CodingPlanUsage::default());
    };
    let null = Value::Null;
    let quota = instance.get("codingPlanQuotaInfo").unwrap_or(&null);
    Ok(CodingPlanUsage {
        found: true,
        instance_type: instance
            .get("instanceType")
            .and_then(Value::as_str)
            .map(str::to_string),
        per_5_hour: coding_window(quota, "per5Hour"),
        per_week: coding_window(quota, "perWeek"),
        per_bill_month: coding_window(quota, "perBillMonth"),
    })
}

/// 找到第一个包含指定字段的对象，用于在不写死包装层的前提下读取一段结构。
fn find_object_with_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if map.contains_key(key) {
                Some(value)
            } else {
                map.values()
                    .find_map(|child| find_object_with_key(child, key))
            }
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_object_with_key(child, key)),
        _ => None,
    }
}

/// 账号提示只保留末四位，避免完整 UID 进入常规展示或通知。
fn mask_uid(uid: &str) -> String {
    let tail: String = uid
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("UID ***{tail}")
}

/// 从 Coding Plan 响应里的 `userId` 兜底构造账号提示。
fn identity_from_user_id(value: &Value) -> Option<AccountIdentity> {
    let uid = find_key(value, "userId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(AccountIdentity {
        uid: Some(uid.to_string()),
        nick_name: None,
    })
}

/// 控制台登录信息：`aliyun.uid` 与 `aliyun.nickName`。
fn parse_identity(value: &Value) -> Option<AccountIdentity> {
    let aliyun = find_key(value, "aliyun")?.as_object()?;
    let uid = aliyun
        .get("uid")
        .and_then(Value::as_str)
        .map(str::to_string);
    let nick_name = aliyun
        .get("nickName")
        .and_then(Value::as_str)
        .map(str::to_string);
    (uid.is_some() || nick_name.is_some()).then_some(AccountIdentity { uid, nick_name })
}

/// 订阅接口返回 `specCode`（lite / essential / standard / pro）、状态与到期时间。
/// 没有有效订阅时字段可能缺失，此时返回默认值并由调用方标记为非有效订阅。
fn parse_token_plan_subscription(value: &Value) -> Option<TokenPlanSubscription> {
    let object = find_object_with_key(value, "specCode")?;
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let spec_code = object
        .get("specCode")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(TokenPlanSubscription {
        active: status.as_deref() == Some("VALID"),
        spec_code,
        status,
        remaining_days: as_i64(object.get("remainingDays")),
        end_time_epoch: millis_to_epoch(object.get("endTime")),
    })
}

fn epoch_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

/// 同时读取两类套餐、Token Plan 订阅档位与账号提示。
/// 订阅/身份接口失败只影响对应展示，不改变额度读取结果。
pub fn read_usage(access_token: &str) -> BailianReadout {
    let token_plan_value = call_console(access_token, TOKEN_PLAN_API, json!({}));
    let coding_plan_value = call_console(
        access_token,
        CODING_PLAN_API,
        json!({
            "queryCodingPlanInstanceInfoRequest": {
                "commodityCode": CODING_PLAN_COMMODITY,
                "onlyLatestOne": true,
            }
        }),
    );
    let subscription_value = call_console(
        access_token,
        TOKEN_PLAN_SUBSCRIPTION_API,
        json!({
            "queryInstanceInfoRequest": {
                "commodityCode": TOKEN_PLAN_COMMODITY,
            }
        }),
    );
    let login_info_value = call_console(access_token, LOGIN_INFO_API, json!({}));

    let identity = login_info_value
        .as_ref()
        .ok()
        .and_then(parse_identity)
        .or_else(|| {
            coding_plan_value
                .as_ref()
                .ok()
                .and_then(identity_from_user_id)
        });
    // 接口成功但没有 specCode 时视为“无有效订阅”，与接口失败（None）区分。
    let token_plan_subscription = subscription_value
        .as_ref()
        .ok()
        .map(|value| parse_token_plan_subscription(value).unwrap_or_default());

    BailianReadout {
        checked_at_epoch: epoch_now(),
        token_plan: token_plan_value.and_then(|value| parse_token_plan(&value)),
        coding_plan: coding_plan_value.and_then(|value| parse_coding_plan(&value)),
        token_plan_subscription,
        identity,
    }
}

// ---------- 浏览器登录回调 ----------

/// 一次等待中的控制台授权：本地回环服务器 + 官方登录页地址。
pub struct ConsoleLogin {
    pub login_id: String,
    pub auth_url: String,
    listener: TcpListener,
    state: String,
}

/// 在随机端口启动回环回调服务，并生成带随机 state 的官方登录页地址。
pub fn begin_console_login() -> Result<ConsoleLogin, BailianError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|err| {
        BailianError::new(
            "login-unavailable",
            format!("无法在 127.0.0.1 启动本地授权回调：{err}"),
        )
    })?;
    listener.set_nonblocking(true).map_err(|err| {
        BailianError::new("login-unavailable", format!("设置回调监听失败：{err}"))
    })?;
    let port = listener
        .local_addr()
        .map_err(|err| BailianError::new("login-unavailable", format!("读取回调端口失败：{err}")))?
        .port();
    let state = random_hex(16);
    let login_id = format!("bailian-{}", random_hex(8));
    let auth_url =
        format!("{CONSOLE_LOGIN_BASE}/console-login?notice=127.0.0.1:{port}?state={state}");
    Ok(ConsoleLogin {
        login_id,
        auth_url,
        listener,
        state,
    })
}

impl ConsoleLogin {
    /// 等待浏览器把 `access_token` 回传到本机。只有 state 匹配且解析到令牌才结束；
    /// 预检、缺少令牌的请求以及错误的 state 都不会终止等待。
    pub fn wait_for_token(
        self,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<String, BailianError> {
        let started = SystemTime::now();
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(BailianError::new("canceled", "已取消授权"));
            }
            if started
                .elapsed()
                .map(|elapsed| elapsed >= timeout)
                .unwrap_or(false)
            {
                return Err(BailianError::new("timeout", "等待浏览器授权超时，请重试。"));
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    // 部分平台（含 macOS）的 accepted socket 会继承监听套接字的
                    // O_NONBLOCK；若浏览器数据还没到，read 会立刻返回 WouldBlock
                    // 并把这次回调误判为无效请求。这里改回阻塞，让 3 秒读超时生效。
                    let _ = stream.set_nonblocking(false);
                    match handle_callback(&mut stream, &self.state) {
                        Callback::Token(token) => return Ok(token),
                        Callback::Preflight => {}
                        Callback::BadState => {}
                        Callback::Ignored => {}
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(CALLBACK_POLL_INTERVAL);
                }
                Err(err) => {
                    return Err(BailianError::new(
                        "login-unavailable",
                        format!("本地授权回调失败：{err}"),
                    ));
                }
            }
        }
    }
}

enum Callback {
    Token(String),
    Preflight,
    BadState,
    Ignored,
}

struct HttpRequest {
    method: String,
    path: String,
    content_type: String,
    body: Vec<u8>,
}

fn handle_callback(stream: &mut TcpStream, expected_state: &str) -> Callback {
    stream.set_read_timeout(Some(CALLBACK_TIMEOUT_READ)).ok();
    let Some(request) = read_request(stream) else {
        respond(stream, 400, "bad request");
        return Callback::Ignored;
    };
    if request.method == "OPTIONS" {
        respond(stream, 204, "");
        return Callback::Preflight;
    }

    let (path, query) = request
        .path
        .split_once('?')
        .map_or((request.path.as_str(), ""), |(path, query)| (path, query));
    let _ = path;

    let query_token =
        query_param(query, "access_token").or_else(|| query_param(query, "accessToken"));
    let body_token = extract_body_token(&request.content_type, &request.body);
    let body = String::from_utf8_lossy(&request.body);
    let state = query_param(query, "state")
        .or_else(|| query_param(&body, "state"))
        .or_else(|| json_state(&body));

    if state.as_deref() != Some(expected_state) {
        respond(stream, 400, "bad state");
        return Callback::BadState;
    }

    match query_token.or(body_token) {
        Some(token) if !token.trim().is_empty() => {
            respond(stream, 200, "OK");
            Callback::Token(token.trim().to_string())
        }
        _ => {
            // 登录页可能先探测回调地址再提交凭证；保持监听，继续等待。
            respond(stream, 200, "OK");
            Callback::Ignored
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_CALLBACK_BYTES + 8192 {
            return None;
        }
        if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
            break index;
        }
    };

    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut content_type = String::new();
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-type") {
            content_type = value.trim().to_string();
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().ok()?;
        }
    }
    if content_length > MAX_CALLBACK_BYTES {
        return None;
    }

    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_CALLBACK_BYTES {
            return None;
        }
    }
    body.truncate(content_length);

    Some(HttpRequest {
        method,
        path,
        content_type,
        body,
    })
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, PUT, PATCH, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    decoded.push(byte);
                    index += 3;
                    continue;
                }
                decoded.push(bytes[index]);
                index += 1;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (url_decode(name) == key).then(|| url_decode(value))
    })
}

fn extract_body_token(content_type: &str, body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("application/json") || lower.contains("text/json") {
        if let Some(token) = json_token(trimmed) {
            return Some(token);
        }
    }
    if lower.contains("application/x-www-form-urlencoded") {
        if let Some(token) =
            query_param(trimmed, "access_token").or_else(|| query_param(trimmed, "accessToken"))
        {
            return Some(token);
        }
    }
    if lower.contains("multipart/form-data") {
        if let Some(token) = multipart_token(trimmed) {
            return Some(token);
        }
    }
    json_token(trimmed)
        .or_else(|| query_param(trimmed, "access_token"))
        .or_else(|| query_param(trimmed, "accessToken"))
}

fn json_token(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    let direct = value
        .get("access_token")
        .or_else(|| value.get("accessToken"))
        .and_then(Value::as_str);
    direct
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("access_token").or_else(|| data.get("accessToken")))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

fn multipart_token(text: &str) -> Option<String> {
    let marker = "name=\"access_token\"";
    let start = text
        .find(marker)
        .or_else(|| text.find("name='access_token'"))?;
    let rest = &text[start..];
    let value_start = rest.find("\r\n\r\n").map(|index| index + 4)?;
    let value = &rest[value_start..];
    let end = value.find("\r\n").unwrap_or(value.len());
    Some(value[..end].trim().to_string())
}

fn json_state(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_string)
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

    fn token_plan_response() -> Value {
        json!({
            "code": "200",
            "data": {
                "DataV2": {
                    "data": {
                        "data": {
                            "per1MonthPercentage": 0.05000950666666667,
                            "per1MonthResetTime": 1_795_363_200_000_i64
                        }
                    }
                },
                "success": true
            },
            "successResponse": true
        })
    }

    fn coding_plan_response() -> Value {
        json!({
            "data": {
                "DataV2": {
                    "data": {
                        "data": {
                            "codingPlanInstanceInfos": [{
                                "status": "VALID",
                                "instanceType": "pro",
                                "codingPlanQuotaInfo": {
                                    "per5HourUsedQuota": 0,
                                    "per5HourTotalQuota": 6000,
                                    "per5HourQuotaNextRefreshTime": 1_792_739_314_000_i64,
                                    "perWeekUsedQuota": 1,
                                    "perWeekTotalQuota": 45000,
                                    "perWeekQuotaNextRefreshTime": 1_792_944_000_000_i64,
                                    "perBillMonthUsedQuota": 1,
                                    "perBillMonthTotalQuota": 90000,
                                    "perBillMonthQuotaNextRefreshTime": 1_794_412_800_000_i64
                                }
                            }],
                            "userId": "1679446554858477"
                        }
                    }
                },
                "success": true
            },
            "successResponse": true
        })
    }

    #[test]
    fn token_plan_percentage_becomes_used_percent() {
        let usage = parse_token_plan(&token_plan_response()).unwrap();
        assert_eq!(usage.used_percent, Some(5));
        assert_eq!(usage.resets_at_epoch, Some(1_795_363_200));
    }

    #[test]
    fn coding_plan_reads_all_three_windows() {
        let usage = parse_coding_plan(&coding_plan_response()).unwrap();
        assert!(usage.found);
        assert_eq!(usage.instance_type.as_deref(), Some("pro"));
        assert_eq!(usage.per_5_hour.total, Some(6000));
        assert_eq!(usage.per_week.used, Some(1));
        assert_eq!(usage.per_bill_month.total, Some(90000));
        assert_eq!(usage.per_5_hour.resets_at_epoch, Some(1_792_739_314));
    }

    #[test]
    fn coding_plan_without_valid_instance_is_not_an_error() {
        let value = json!({
            "data": { "DataV2": { "data": { "data": {
                "codingPlanInstanceInfos": [{ "status": "EXPIRED" }]
            }}}}
        });
        let usage = parse_coding_plan(&value).unwrap();
        assert!(!usage.found);
        assert_eq!(usage.per_week, CodingPlanWindow::default());
    }

    #[test]
    fn missing_fields_stay_unknown_instead_of_zero() {
        let usage = parse_token_plan(&json!({ "data": {} })).unwrap();
        assert_eq!(usage.used_percent, None);
        assert_eq!(usage.resets_at_epoch, None);
    }

    #[test]
    fn user_id_is_masked_to_last_four_digits() {
        assert_eq!(
            identity_from_user_id(&coding_plan_response())
                .and_then(|identity| identity.hint())
                .as_deref(),
            Some("UID ***8477")
        );
    }

    #[test]
    fn token_plan_subscription_reads_spec_code_and_status() {
        let value = json!({
            "data": { "DataV2": { "data": { "data": {
                "instanceCode": "sfm_tokenplansolo_public_cn-xxx",
                "specCode": "standard",
                "remainingDays": 30,
                "startTime": 1_787_365_170_000_i64,
                "endTime": 1_795_363_200_000_i64,
                "autoRenewFlag": false,
                "status": "VALID"
            }}}}
        });
        let subscription = parse_token_plan_subscription(&value).unwrap();
        assert!(subscription.active);
        assert_eq!(subscription.spec_code.as_deref(), Some("standard"));
        assert_eq!(subscription.status.as_deref(), Some("VALID"));
        assert_eq!(subscription.remaining_days, Some(30));
        assert_eq!(subscription.end_time_epoch, Some(1_795_363_200));
    }

    #[test]
    fn token_plan_subscription_without_spec_code_is_not_active() {
        let value = json!({ "data": { "DataV2": { "data": { "data": {
            "status": "EXPIRED"
        }}}}});
        assert!(parse_token_plan_subscription(&value).is_none());
    }

    #[test]
    fn login_info_prefers_nickname_and_falls_back_to_masked_uid() {
        let named = json!({ "data": { "DataV2": { "data": { "data": {
            "aliyun": { "uid": "1679446554858477", "nickName": "rainbow217" }
        }}}}});
        let identity = parse_identity(&named).unwrap();
        assert_eq!(identity.hint().as_deref(), Some("rainbow217"));

        let numeric = json!({ "data": { "DataV2": { "data": { "data": {
            "aliyun": { "uid": "1679446554858477", "nickName": "1679446554858477" }
        }}}}});
        let identity = parse_identity(&numeric).unwrap();
        assert_eq!(identity.hint().as_deref(), Some("UID ***8477"));
    }

    #[test]
    fn gateway_payload_contains_expected_action_metadata() {
        let params = request_params(TOKEN_PLAN_API, json!({}));
        let value: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(value["Api"], TOKEN_PLAN_API);
        assert_eq!(value["V"], "1.0");
        assert_eq!(value["Data"]["cornerstoneParam"]["protocol"], "V2");
        assert_eq!(value["Data"]["cornerstoneParam"]["console"], "ONE_CONSOLE");
    }

    #[test]
    fn token_is_extracted_from_query_form_json_and_multipart() {
        assert_eq!(
            query_param("state=abc&access_token=tok", "access_token").as_deref(),
            Some("tok")
        );
        assert_eq!(
            extract_body_token("application/x-www-form-urlencoded", b"access_token=tok"),
            Some("tok".to_string())
        );
        assert_eq!(
            extract_body_token("application/json", br#"{"data":{"accessToken":"tok"}}"#),
            Some("tok".to_string())
        );
        let multipart = "Content-Disposition: form-data; name=\"access_token\"\r\n\r\ntok\r\n--";
        assert_eq!(
            extract_body_token("multipart/form-data; boundary=x", multipart.as_bytes()),
            Some("tok".to_string())
        );
    }

    #[test]
    fn credentials_round_trip_through_private_directory() {
        let dir = std::env::temp_dir().join(format!(
            "token-refill-bailian-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        save_credentials(&dir, "  sk-test  ").unwrap();
        let credentials = load_credentials(&dir).unwrap();
        assert_eq!(credentials.access_token, "sk-test");
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
            "token-refill-bailian-missing-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let error = load_credentials(&dir).unwrap_err();
        assert!(error.is_auth_error());
    }

    #[test]
    fn not_logged_in_error_is_classified_as_auth_expired() {
        assert!(is_not_logged_in("NotLogined", ""));
        assert!(is_not_logged_in("", "Console session expired"));
        assert!(!is_not_logged_in("InvalidParameter", "bad request"));
    }

    #[test]
    fn console_login_url_uses_loopback_and_random_state() {
        let login = begin_console_login().unwrap();
        assert!(login
            .auth_url
            .starts_with("https://bailian.console.aliyun.com/console-login?notice=127.0.0.1:"));
        assert!(login.auth_url.contains("?state="));
        assert_eq!(login.state.len(), 32);
    }

    fn login_port(auth_url: &str) -> u16 {
        let rest = auth_url.split("127.0.0.1:").nth(1).expect("loopback port");
        rest.split('?').next().unwrap().parse().unwrap()
    }

    #[test]
    fn callback_server_accepts_matching_state_and_returns_token() {
        let login = begin_console_login().unwrap();
        let port = login_port(&login.auth_url);
        let state = login.state.clone();
        let cancel = AtomicBool::new(false);
        let server =
            std::thread::spawn(move || login.wait_for_token(&cancel, Duration::from_secs(5)));

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let request = format!(
            "GET /?state={state}&access_token=console-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        assert_eq!(server.join().unwrap().unwrap(), "console-token".to_string());
    }

    #[test]
    fn callback_server_rejects_wrong_state_and_keeps_waiting() {
        let login = begin_console_login().unwrap();
        let port = login_port(&login.auth_url);
        let state = login.state.clone();
        let cancel = AtomicBool::new(false);
        let server =
            std::thread::spawn(move || login.wait_for_token(&cancel, Duration::from_secs(5)));

        let mut bad = TcpStream::connect(("127.0.0.1", port)).unwrap();
        bad.write_all(
            b"GET /?state=wrong&access_token=stolen HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
        bad.flush().unwrap();

        let mut good = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = br#"{"access_token":"tok-2"}"#;
        let request = format!(
            "POST /?state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        good.write_all(request.as_bytes()).unwrap();
        good.write_all(body).unwrap();
        good.flush().unwrap();

        assert_eq!(server.join().unwrap().unwrap(), "tok-2".to_string());
    }

    #[test]
    fn console_call_posts_form_payload_with_bearer_token() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&buffer[..index]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .or_else(|| line.strip_prefix("Content-Length: "))
                        })
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buffer.len() >= index + 4 + length {
                        break;
                    }
                }
            }
            let body = r#"{"successResponse":true,"data":{"success":true}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
            String::from_utf8_lossy(&buffer).into_owned()
        });

        let value = call_console_at(
            &format!("http://127.0.0.1:{port}"),
            "console-token",
            TOKEN_PLAN_API,
            json!({}),
        )
        .unwrap();
        assert_eq!(value["successResponse"], true);

        let request = server.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(request.starts_with(
            "POST /cli/api.json?action=BroadScopeAspnGateway&product=sfm_bailian&api="
        ));
        assert!(lower.contains("authorization: bearer console-token"));
        assert!(lower.contains("content-type: application/x-www-form-urlencoded"));
        assert!(request.contains("params="));
        assert!(request.contains("region=cn-beijing"));
    }
}
