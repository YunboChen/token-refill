//! 第三方中转平台的密钥额度查询。
//!
//! 两类平台都只凭 API Key 查询当前密钥的实时额度：
//! - New API：`GET /api/usage/token/`，并用 `/api/status` 的 `quota_per_unit`
//!   把平台内部的 quota 单位换算成美元；
//! - Sub2API：`GET /v1/usage`，按 `mode` 区分密钥额度、订阅窗口与钱包余额。
//!
//! 密钥只保存在 Rust 侧，不进入前端状态、日志或错误消息。请求一律使用
//! HTTPS；仅本机回环地址允许 HTTP，用于自部署调试。

pub mod newapi;
pub mod sub2api;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use url::{Host, Url};

use newapi::NewApiProvider;
use sub2api::Sub2ApiProvider;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;

/// New API 内部额度的默认换算比例：1 美元 = 500000 quota。
/// 服务端可通过 `/api/status` 返回实际值，默认值只在取不到时兜底。
const DEFAULT_QUOTA_PER_UNIT: f64 = 500_000.0;

/// 额度来源平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuotaProvider {
    NewApi,
    Sub2Api,
}

/// 额度口径。平台没有提供额度数据时不要用 0 顶替，用 `Unrestricted` 或
/// `Option` 字段表达缺失。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuotaMode {
    /// 密钥自身的总额度。
    Key,
    /// 订阅套餐的日/周/月额度。
    Subscription,
    /// 账户钱包余额。
    Wallet,
    /// 返回了余额字段，但无法判断是订阅还是钱包。
    Unrestricted,
}

/// 密钥的实时额度快照。金额统一为美元。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyQuota {
    pub provider: QuotaProvider,
    pub mode: QuotaMode,
    pub key_name: Option<String>,
    pub plan_name: Option<String>,
    /// 密钥状态是否可用；平台未返回时为 true。
    pub valid: bool,
    /// 总额度不受限制（仍可能受窗口限速或订阅周期限制）。
    pub unlimited: bool,
    pub limit_usd: Option<f64>,
    pub used_usd: Option<f64>,
    pub remaining_usd: Option<f64>,
    /// New API 在密钥允许时额外返回的账户额度。
    pub account: Option<AccountQuota>,
    pub expires_at_epoch: Option<i64>,
    pub windows: Vec<KeyQuotaWindow>,
    pub checked_at_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuota {
    pub used_usd: Option<f64>,
    pub remaining_usd: Option<f64>,
}

/// 限速窗口或订阅周期。`limit_usd` 缺失表示平台没有配置该周期上限。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyQuotaWindow {
    /// 平台给出的窗口标识：5h / 1d / 7d / daily / weekly / monthly。
    pub window_id: String,
    pub duration_mins: Option<i64>,
    pub limit_usd: Option<f64>,
    pub used_usd: Option<f64>,
    pub remaining_usd: Option<f64>,
    pub remaining_percent: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

pub trait KeyQuotaProvider {
    fn provider(&self) -> QuotaProvider;
    fn fetch_quota(&self) -> Result<KeyQuota, String>;
}

impl QuotaProvider {
    pub fn label(self) -> &'static str {
        match self {
            QuotaProvider::NewApi => "New API",
            QuotaProvider::Sub2Api => "Sub2API",
        }
    }
}

/// 自动识别结果：平台类型与对应的实时额度。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedQuota {
    pub provider: QuotaProvider,
    pub quota: KeyQuota,
}

/// 同一个 Base URL 下依次尝试两种额度接口，以实际响应判断上游类型，
/// 不要求用户先选择平台。两种都失败时把各自的错误一并返回。
pub fn detect_and_fetch(base_url: &str, api_key: &str) -> Result<DetectedQuota, String> {
    let base_url = normalize_base_url(base_url)?;
    let api_key = validate_api_key(api_key)?;

    let mut failures = Vec::new();
    for provider in [QuotaProvider::NewApi, QuotaProvider::Sub2Api] {
        let result = match provider {
            QuotaProvider::NewApi => {
                NewApiProvider::new(&base_url, &api_key).and_then(|client| client.fetch_quota())
            }
            QuotaProvider::Sub2Api => {
                Sub2ApiProvider::new(&base_url, &api_key).and_then(|client| client.fetch_quota())
            }
        };
        match result {
            Ok(quota) => return Ok(DetectedQuota { provider, quota }),
            Err(message) => failures.push(format!("{}：{message}", provider.label())),
        }
    }

    Err(format!(
        "无法识别上游类型或密钥无效（{}）。",
        failures.join("；")
    ))
}

/// 校验后的服务地址与密钥。不实现 `Debug`，避免密钥被意外打印。
#[derive(Clone)]
pub(crate) struct Endpoint {
    base_url: String,
    api_key: String,
}

impl Endpoint {
    pub(crate) fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        Ok(Self {
            base_url: normalize_base_url(base_url)?,
            api_key: validate_api_key(api_key)?,
        })
    }

    /// 不带密钥的公开接口，例如 New API 的 `/api/status`。
    pub(crate) fn get_json(&self, path: &str) -> Result<Value, String> {
        self.request(path, false)
    }

    /// 带 `Authorization: Bearer` 的接口。
    pub(crate) fn get_json_authenticated(&self, path: &str) -> Result<Value, String> {
        self.request(path, true)
    }

    fn request(&self, path: &str, authenticated: bool) -> Result<Value, String> {
        let url = format!("{}{path}", self.base_url);
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = crate::proxy::ureq_proxy_for(&url) {
            config = config.proxy(Some(proxy));
        }
        let agent: ureq::Agent = config.build().into();

        let mut request = agent.get(&url).header("Accept", "application/json");
        if authenticated {
            request = request.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let mut response = request.call().map_err(map_request_error)?;
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|_| "读取额度响应失败。".to_string())?;
        serde_json::from_str(&body).map_err(|_| "额度响应格式无法识别。".to_string())
    }
}

/// 只接受 HTTPS；本机回环地址允许 HTTP，便于调试自部署服务。
pub(crate) fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请先填写服务地址。".to_string());
    }

    let url = Url::parse(trimmed).map_err(|_| "服务地址格式不正确。".to_string())?;
    if url.host().is_none() {
        return Err("服务地址缺少主机名。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("服务地址不能包含账号信息。".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("服务地址不能包含查询参数或片段。".to_string());
    }
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        "http" => return Err("额度查询只允许 HTTPS 地址（本机回环地址除外）。".to_string()),
        _ => return Err("服务地址必须以 https:// 开头。".to_string()),
    }

    // 客户端配置通常把 Base URL 写成 `https://host/v1`；额度接口在站点根路径，
    // 去掉末尾的 `/v1` 后再拼接，避免出现 `/v1/v1/usage`。
    let base = url.as_str().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .filter(|value| !value.is_empty())
        .unwrap_or(base);
    Ok(base.to_string())
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

pub(crate) fn validate_api_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("请先填写 API Key。".to_string());
    }
    if key.len() > 512 || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("API Key 格式不正确。".to_string());
    }
    Ok(key.to_string())
}

/// 错误信息只描述状态，不回显响应正文，避免把密钥或账户数据带出去。
fn map_request_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(401 | 403) => "密钥无效、已禁用或无权查询额度。".to_string(),
        ureq::Error::StatusCode(404) => "未找到额度接口，请确认服务地址与平台类型。".to_string(),
        ureq::Error::StatusCode(429) => "查询过于频繁，请稍后重试。".to_string(),
        ureq::Error::StatusCode(code) if code >= 500 => {
            "服务端暂时不可用，请稍后重试。".to_string()
        }
        ureq::Error::StatusCode(code) => format!("查询额度失败（HTTP {code}）。"),
        other => format!("无法连接到额度服务：{other}"),
    }
}

pub(crate) fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// 金额保留 6 位小数，避免浮点尾数进入界面与状态文件。
pub(crate) fn round_money(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let rounded = (value * 1_000_000.0).round() / 1_000_000.0;
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// 接受秒/毫秒时间戳与 RFC 3339 字符串（Go 的 `time.Time` 默认序列化形式）；
/// 0、负数与无效值都视为未提供。
pub(crate) fn parse_epoch(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            let raw = number
                .as_i64()
                .or_else(|| number.as_f64().map(|v| v as i64))?;
            if raw <= 0 {
                None
            } else if raw > 9_999_999_999 {
                Some(raw / 1000)
            } else {
                Some(raw)
            }
        }
        Value::String(text) => OffsetDateTime::parse(text.trim(), &Rfc3339)
            .ok()
            .map(|time| time.unix_timestamp()),
        _ => None,
    }
}

pub(crate) fn remaining_percent(limit: Option<f64>, remaining: Option<f64>) -> Option<i64> {
    let limit = limit.filter(|value| *value > 0.0)?;
    let remaining = remaining?;
    Some(((remaining / limit) * 100.0).round().clamp(0.0, 100.0) as i64)
}

pub(crate) fn quota_per_unit_from_status(status: &Value) -> Option<f64> {
    status
        .pointer("/data/quota_per_unit")
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0)
}

pub(crate) fn default_quota_per_unit() -> f64 {
    DEFAULT_QUOTA_PER_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_drops_client_v1_suffix() {
        assert_eq!(
            normalize_base_url("https://relay.example.com/v1").unwrap(),
            "https://relay.example.com"
        );
        assert_eq!(
            normalize_base_url("https://relay.example.com/v1/").unwrap(),
            "https://relay.example.com"
        );
        assert_eq!(
            normalize_base_url("https://relay.example.com/proxy/v1").unwrap(),
            "https://relay.example.com/proxy"
        );
        assert_eq!(
            normalize_base_url("https://relay.example.com").unwrap(),
            "https://relay.example.com"
        );
    }

    #[test]
    fn loopback_http_allowed_but_remote_http_rejected() {
        assert!(normalize_base_url("http://127.0.0.1:8080/v1").is_ok());
        assert!(normalize_base_url("http://localhost:8080").is_ok());
        assert!(normalize_base_url("http://relay.example.com").is_err());
    }

    #[test]
    fn timestamps_accept_seconds_milliseconds_and_rfc3339() {
        assert_eq!(
            parse_epoch(&Value::from(1_790_000_000)),
            Some(1_790_000_000)
        );
        assert_eq!(
            parse_epoch(&Value::from(1_790_000_000_000_i64)),
            Some(1_790_000_000)
        );
        assert_eq!(
            parse_epoch(&Value::from("2026-10-23T08:00:00Z")),
            Some(1_792_742_400)
        );
        assert_eq!(parse_epoch(&Value::from(0)), None);
        assert_eq!(parse_epoch(&Value::from("not a time")), None);
    }
}
