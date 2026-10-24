//! New API：`GET /api/usage/token/` 返回当前 Bearer 密钥的授予/已用/可用额度。
//!
//! 平台额度以内部 quota 为计量单位，`/api/status` 的 `data.quota_per_unit`
//! 表示 1 美元对应的 quota 数；取不到时按上游默认的 500000 换算。
//!
//! 账户口径有两种来源：
//! 1. 密钥自身：`user_quota`，需要 New API ≥ v1.0.0-rc.45，并且该密钥开启了
//!    `allow_user_quota_query`（面板里叫「允许查询用户额度」）；
//! 2. 访问令牌：`GET /api/user/self` 与 `/api/subscription/self`。新版
//!    scoped PAT 需要 New API ≥ v1.0.0-rc.41 且具备 `profile:read`（订阅
//!    还需要 `wallet:read`）；旧版单令牌还需要 `New-Api-User: <用户 ID>`。
//!    密钥已返回账户额度时仍会尝试令牌（用于补充订阅），失败则静默回退。
//!
//! 二开站点常添加 `quota_scope`、`display` 等扩展字段，也可能把数字序列化成
//! 字符串或使用 `paid_balance` / `balance` 等别名；解析按可缺省、可别名、
//! 未知即忽略处理，不把扩展字段当成上游协议承诺。

use serde::Deserialize;
use serde_json::Value;

use super::{
    default_quota_per_unit, flexible, now_epoch, quota_display_unit_from_status,
    quota_per_unit_from_status, remaining_percent, round_money, validate_account_token,
    validate_new_api_user_id, AccountQuota, AccountSource, CredentialKind, Endpoint, KeyQuota,
    KeyQuotaProvider, KeyQuotaWindow, QuotaMode, QuotaProvider,
};

const ACCOUNT_HINT: &str = "站点未返回账户额度。需要 New API v1.0.0-rc.45 及以上并在密钥上开启「允许查询用户额度」，或填写访问令牌；旧版单令牌还需同时填写用户 ID。";

pub struct NewApiProvider {
    endpoint: Endpoint,
    account_endpoint: Option<Endpoint>,
    account_user_id: Option<String>,
}

impl NewApiProvider {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        Self::new_with_account_token(base_url, api_key, None, None)
    }

    /// `account_token` 是可选的身份令牌，只用于账户级接口；密钥仍用于额度接口。
    /// 旧版 New API 的单令牌还需要 `New-Api-User` 头，对应 `account_user_id`。
    pub fn new_with_account_token(
        base_url: &str,
        api_key: &str,
        account_token: Option<&str>,
        account_user_id: Option<&str>,
    ) -> Result<Self, String> {
        let account_endpoint = account_token
            .map(validate_account_token)
            .transpose()?
            .map(|token| Endpoint::new(base_url, &token))
            .transpose()?;
        Ok(Self {
            endpoint: Endpoint::new(base_url, api_key)?,
            account_endpoint,
            account_user_id: validate_new_api_user_id(account_user_id)?,
        })
    }
}

impl KeyQuotaProvider for NewApiProvider {
    fn provider(&self) -> QuotaProvider {
        QuotaProvider::NewApi
    }

    fn fetch_quota(&self) -> Result<KeyQuota, String> {
        let payload = self.endpoint.get_json_authenticated("/api/usage/token/")?;
        // 公开接口，失败时回退到上游默认换算比例，不影响额度查询本身。
        let status = self.endpoint.get_json("/api/status").ok();
        let quota_per_unit = status
            .as_ref()
            .and_then(quota_per_unit_from_status)
            .unwrap_or_else(default_quota_per_unit);
        let currency = status.as_ref().and_then(quota_display_unit_from_status);

        let mut quota = parse_usage(&payload, quota_per_unit, currency.as_deref(), now_epoch())?;
        self.apply_account_data(&mut quota, quota_per_unit);
        Ok(quota)
    }
}

impl NewApiProvider {
    /// 访问令牌优先；没有令牌时沿用密钥返回的 `user_quota`。
    /// 令牌查询失败不回滚密钥额度，只把原因记在 `account_error` 上。
    fn apply_account_data(&self, quota: &mut KeyQuota, quota_per_unit: f64) {
        let key_account = quota.account.take();
        let Some(endpoint) = &self.account_endpoint else {
            quota.account = key_account;
            if quota.account.is_none() {
                quota.account_error = Some(ACCOUNT_HINT.to_string());
            }
            return;
        };

        let user_id = self.account_user_id.as_deref();
        match fetch_account(endpoint, quota_per_unit, user_id) {
            Ok(mut account) => {
                account.windows = fetch_subscription_windows(endpoint, quota_per_unit, user_id);
                quota.account = Some(account);
                quota.account_error = None;
            }
            Err(error) => {
                quota.account = key_account;
                // 密钥已经给出账户额度时，令牌失败不再覆盖成错误提示。
                quota.account_error = if quota.account.is_none() {
                    Some(error)
                } else {
                    None
                };
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenUsageEnvelope {
    #[serde(default)]
    data: Option<TokenUsage>,
}

#[derive(Debug, Deserialize)]
struct TokenUsage {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    total_granted: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    total_used: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    total_available: Option<f64>,
    #[serde(default, deserialize_with = "flexible::bool_default")]
    unlimited_quota: bool,
    #[serde(default, deserialize_with = "flexible::opt_i64")]
    expires_at: Option<i64>,
    #[serde(default)]
    user_quota: Option<UserQuota>,
    /// 部分站点（例如按账户共享额度的中转站）用该字段标记密钥使用的是账户额度。
    /// 上游没有这个字段；也兼容 user/shared 等常见二开取值。
    #[serde(default)]
    quota_scope: Option<String>,
    /// 部分站点提供的展示用换算结果，单位已是币种单位而不是内部 quota。
    /// 上游没有这个字段；字段名兼容 remaining/remain/available 等写法。
    #[serde(default)]
    display: Option<DisplayBlock>,
}

#[derive(Debug, Default, Deserialize)]
struct DisplayBlock {
    #[serde(
        default,
        alias = "remain",
        alias = "available",
        deserialize_with = "flexible::opt_f64"
    )]
    remaining: Option<f64>,
    #[serde(
        default,
        alias = "granted",
        alias = "quota",
        deserialize_with = "flexible::opt_f64"
    )]
    total: Option<f64>,
    #[serde(
        default,
        alias = "used_quota",
        alias = "consumed",
        deserialize_with = "flexible::opt_f64"
    )]
    used: Option<f64>,
    #[serde(default, alias = "currency", alias = "symbol")]
    unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserQuota {
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    total_available: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    total_used: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SelfEnvelope {
    #[serde(default)]
    data: Option<SelfData>,
}

#[derive(Debug, Deserialize)]
struct SelfData {
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    quota: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    used_quota: Option<f64>,
    /// 二开站点可能把余额拆成实付/赠送，或使用 balance 命名。
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    paid_balance: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    bonus_balance: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    balance: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    used: Option<f64>,
}

impl SelfData {
    fn remaining_quota(&self) -> Option<f64> {
        if let Some(quota) = self.quota.or(self.balance) {
            return Some(quota);
        }
        // 只给了拆分余额时按实付 + 赠送还原。
        match (self.paid_balance, self.bonus_balance) {
            (Some(paid), Some(bonus)) => Some(paid + bonus),
            (paid, bonus) => paid.or(bonus),
        }
    }

    fn used_quota(&self) -> Option<f64> {
        self.used_quota.or(self.used)
    }
}

#[derive(Debug, Default, Deserialize)]
struct SubscriptionEnvelope {
    #[serde(default)]
    data: Option<SubscriptionData>,
}

#[derive(Debug, Default, Deserialize)]
struct SubscriptionData {
    #[serde(default)]
    subscriptions: Vec<SubscriptionSummary>,
}

#[derive(Debug, Default, Deserialize)]
struct SubscriptionSummary {
    #[serde(default)]
    subscription: Option<Subscription>,
}

#[derive(Debug, Default, Deserialize)]
struct Subscription {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    amount_total: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_f64")]
    amount_used: Option<f64>,
    #[serde(default, deserialize_with = "flexible::opt_i64")]
    next_reset_time: Option<i64>,
    #[serde(default)]
    status: Option<String>,
}

impl Subscription {
    fn numeric_id(&self) -> Option<i64> {
        self.id.as_ref().and_then(|value| match value {
            Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64)),
            Value::String(text) => text.trim().parse::<i64>().ok(),
            _ => None,
        })
    }
}

fn to_usd(quota: Option<f64>, quota_per_unit: f64) -> Option<f64> {
    quota
        .filter(|value| value.is_finite())
        .map(|value| round_money(value / quota_per_unit))
}

fn normalize_quota_per_unit(quota_per_unit: f64) -> f64 {
    if quota_per_unit > 0.0 {
        quota_per_unit
    } else {
        default_quota_per_unit()
    }
}

/// 账户总额 = 剩余 + 已用；任何一侧缺失时不猜测。
fn total_from_parts(remaining: Option<f64>, used: Option<f64>, quota_per_unit: f64) -> Option<f64> {
    match (remaining, used) {
        (Some(remaining), Some(used)) if remaining.is_finite() && used.is_finite() => {
            Some(round_money((remaining + used) / quota_per_unit))
        }
        _ => None,
    }
}

fn parse_usage(
    payload: &Value,
    quota_per_unit: f64,
    status_currency: Option<&str>,
    checked_at_epoch: i64,
) -> Result<KeyQuota, String> {
    let envelope: TokenUsageEnvelope = serde_json::from_value(payload.clone())
        .map_err(|_| "New API 返回的额度数据无法识别。".to_string())?;
    let data = envelope
        .data
        .ok_or_else(|| "New API 未返回额度数据（密钥可能无效或已禁用）。".to_string())?;

    let TokenUsage {
        name,
        total_granted,
        total_used,
        total_available,
        unlimited_quota,
        expires_at,
        user_quota,
        quota_scope,
        display,
    } = data;

    let quota_per_unit = normalize_quota_per_unit(quota_per_unit);
    let currency = display
        .as_ref()
        .and_then(|display| display.unit.as_deref())
        .or(status_currency)
        .and_then(normalize_currency);

    // `quota_scope` 与 `display` 都不是上游字段，是二开站点常见的扩展。
    // 明确的 token/key 口径优先；account/user/shared 视为账户口径；没有
    // 该字段时，只有“无限额密钥 + display 金额”才推断为账户口径。
    // 只有出现“余额/总额”才推断为账户口径；仅给 used 不足以判断。
    let display_has_amounts = display
        .as_ref()
        .map(|display| display.remaining.is_some() || display.total.is_some())
        .unwrap_or(false);
    let scope = quota_scope.as_deref().unwrap_or("").trim();
    let account_scope = if scope.eq_ignore_ascii_case("token") || scope.eq_ignore_ascii_case("key")
    {
        false
    } else if scope.eq_ignore_ascii_case("account")
        || scope.eq_ignore_ascii_case("user")
        || scope.eq_ignore_ascii_case("shared")
    {
        true
    } else {
        unlimited_quota && display_has_amounts
    };

    let user_account = user_quota.and_then(|quota| {
        let account = AccountQuota {
            total_usd: total_from_parts(quota.total_available, quota.total_used, quota_per_unit),
            used_usd: to_usd(quota.total_used, quota_per_unit),
            remaining_usd: to_usd(quota.total_available, quota_per_unit),
            source: AccountSource::Key,
            windows: Vec::new(),
        };
        (!account_is_empty(&account)).then_some(account)
    });
    let account = user_account.or_else(|| {
        if account_scope {
            account_from_display(
                display.as_ref(),
                total_used,
                total_available,
                quota_per_unit,
            )
        } else {
            None
        }
    });

    Ok(KeyQuota {
        provider: QuotaProvider::NewApi,
        mode: QuotaMode::Key,
        key_name: name,
        plan_name: None,
        currency,
        valid: true,
        unlimited: unlimited_quota,
        // 账户口径下 total_* 属于账户，不再重复当作密钥额度展示。
        // 二开站点可能只给 display、不给 total_*，此时用 display 兜底。
        limit_usd: if unlimited_quota || account_scope {
            None
        } else {
            to_usd(total_granted, quota_per_unit).or_else(|| {
                display
                    .as_ref()
                    .and_then(|display| display.total)
                    .map(round_money)
            })
        },
        used_usd: if account_scope {
            None
        } else {
            to_usd(total_used, quota_per_unit).or_else(|| {
                display
                    .as_ref()
                    .and_then(|display| display.used)
                    .map(round_money)
            })
        },
        remaining_usd: if unlimited_quota || account_scope {
            None
        } else {
            to_usd(total_available, quota_per_unit).or_else(|| {
                display
                    .as_ref()
                    .and_then(|display| display.remaining)
                    .map(round_money)
            })
        },
        account,
        account_error: None,
        expires_at_epoch: expires_at.filter(|value| *value > 0),
        windows: Vec::new(),
        checked_at_epoch,
    })
}

/// 站点给出的 `display` 已经是币种单位；缺失的字段只在同一组数值内推导。
fn account_from_display(
    display: Option<&DisplayBlock>,
    total_used: Option<f64>,
    total_available: Option<f64>,
    quota_per_unit: f64,
) -> Option<AccountQuota> {
    let account = if let Some(display) = display {
        let mut remaining = finite(display.remaining);
        let mut used = finite(display.used);
        let mut total = finite(display.total);
        if total.is_none() {
            total = sum_amounts(remaining, used);
        }
        if remaining.is_none() {
            remaining = subtract_amounts(total, used);
        }
        if used.is_none() {
            used = subtract_amounts(total, remaining);
        }
        AccountQuota {
            total_usd: total.map(round_money),
            used_usd: used.map(round_money),
            remaining_usd: remaining.map(round_money),
            source: AccountSource::Key,
            windows: Vec::new(),
        }
    } else {
        AccountQuota {
            total_usd: total_from_parts(total_available, total_used, quota_per_unit),
            used_usd: to_usd(total_used, quota_per_unit),
            remaining_usd: to_usd(total_available, quota_per_unit),
            source: AccountSource::Key,
            windows: Vec::new(),
        }
    };
    (!account_is_empty(&account)).then_some(account)
}

fn account_is_empty(account: &AccountQuota) -> bool {
    account.total_usd.is_none() && account.used_usd.is_none() && account.remaining_usd.is_none()
}

fn normalize_currency(unit: &str) -> Option<String> {
    let unit = unit.trim();
    if unit.is_empty() {
        return None;
    }
    let upper = unit.to_ascii_uppercase();
    Some(match upper.as_str() {
        "¥" | "￥" | "RMB" => "CNY".to_string(),
        "US$" => "USD".to_string(),
        _ => upper,
    })
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn sum_amounts(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (finite(left), finite(right)) {
        (Some(left), Some(right)) => Some(round_money(left + right)),
        _ => None,
    }
}

fn subtract_amounts(total: Option<f64>, part: Option<f64>) -> Option<f64> {
    match (finite(total), finite(part)) {
        (Some(total), Some(part)) => Some(round_money((total - part).max(0.0))),
        _ => None,
    }
}

fn fetch_account(
    endpoint: &Endpoint,
    quota_per_unit: f64,
    user_id: Option<&str>,
) -> Result<AccountQuota, String> {
    let payload = endpoint
        .get_json_authenticated_with("/api/user/self", CredentialKind::AccessToken, user_id)
        .map_err(|error| format!("账户额度查询失败：{error}"))?;
    parse_account_self(&payload, quota_per_unit)
}

fn parse_account_self(payload: &Value, quota_per_unit: f64) -> Result<AccountQuota, String> {
    let envelope: SelfEnvelope = serde_json::from_value(payload.clone())
        .map_err(|_| "New API 返回的账户额度无法识别。".to_string())?;
    let data = envelope
        .data
        .ok_or_else(|| "New API 未返回账户额度（访问令牌可能无权读取账户信息）。".to_string())?;
    let remaining_quota = data.remaining_quota();
    let used_quota = data.used_quota();
    if remaining_quota.is_none() && used_quota.is_none() {
        return Err(
            "New API 未返回账户额度数据（访问令牌可能缺少 profile:read 权限）。".to_string(),
        );
    }

    let quota_per_unit = normalize_quota_per_unit(quota_per_unit);
    Ok(AccountQuota {
        total_usd: total_from_parts(remaining_quota, used_quota, quota_per_unit),
        used_usd: to_usd(used_quota, quota_per_unit),
        remaining_usd: to_usd(remaining_quota, quota_per_unit),
        source: AccountSource::AccessToken,
        windows: Vec::new(),
    })
}

/// 订阅接口在旧版本可能不存在；失败时只返回空窗口，不影响账户余额。
fn fetch_subscription_windows(
    endpoint: &Endpoint,
    quota_per_unit: f64,
    user_id: Option<&str>,
) -> Vec<KeyQuotaWindow> {
    endpoint
        .get_json_authenticated_with(
            "/api/subscription/self",
            CredentialKind::AccessToken,
            user_id,
        )
        .ok()
        .map(|payload| parse_subscriptions(&payload, quota_per_unit))
        .unwrap_or_default()
}

fn parse_subscriptions(payload: &Value, quota_per_unit: f64) -> Vec<KeyQuotaWindow> {
    let Ok(envelope) = serde_json::from_value::<SubscriptionEnvelope>(payload.clone()) else {
        return Vec::new();
    };
    let quota_per_unit = normalize_quota_per_unit(quota_per_unit);
    envelope
        .data
        .unwrap_or_default()
        .subscriptions
        .into_iter()
        .filter_map(|summary| summary.subscription)
        .filter(|subscription| subscription.status.as_deref() != Some("expired"))
        .enumerate()
        .filter_map(|(index, subscription)| {
            // 没有总额度的订阅无法给出比例，也不冒充无限额度。
            let limit_usd = to_usd(subscription.amount_total, quota_per_unit)?;
            let used_usd = to_usd(subscription.amount_used, quota_per_unit);
            let remaining_usd = used_usd.map(|used| round_money((limit_usd - used).max(0.0)));
            let window_id = subscription
                .numeric_id()
                .map(|id| format!("订阅 {id}"))
                .unwrap_or_else(|| format!("订阅 {}", index + 1));
            Some(KeyQuotaWindow {
                window_id,
                duration_mins: None,
                limit_usd: Some(limit_usd),
                used_usd,
                remaining_usd,
                remaining_percent: remaining_percent(Some(limit_usd), remaining_usd),
                resets_at_epoch: subscription.next_reset_time.filter(|&time| time > 0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(body: &str, checked_at: i64) -> Result<KeyQuota, String> {
        parse_usage(
            &serde_json::from_str(body).unwrap(),
            500_000.0,
            None,
            checked_at,
        )
    }

    #[test]
    fn reads_granted_used_and_available_quota_in_usd() {
        let quota = parse(
            r#"{
                "code": true,
                "message": "ok",
                "data": {
                    "object": "token_usage",
                    "name": "Default Token",
                    "total_granted": 1000000,
                    "total_used": 125000,
                    "total_available": 875000,
                    "unlimited_quota": false,
                    "expires_at": 1790000000,
                    "user_quota": { "total_available": 450000, "total_used": 50000 }
                }
            }"#,
            1_790_000_000,
        )
        .unwrap();

        assert_eq!(quota.provider, QuotaProvider::NewApi);
        assert_eq!(quota.mode, QuotaMode::Key);
        assert_eq!(quota.key_name.as_deref(), Some("Default Token"));
        assert!(!quota.unlimited);
        assert_eq!(quota.limit_usd, Some(2.0));
        assert_eq!(quota.used_usd, Some(0.25));
        assert_eq!(quota.remaining_usd, Some(1.75));
        assert_eq!(quota.expires_at_epoch, Some(1_790_000_000));
        assert!(quota.windows.is_empty());
        assert_eq!(quota.account_error, None);
        let account = quota.account.unwrap();
        assert_eq!(account.remaining_usd, Some(0.9));
        assert_eq!(account.used_usd, Some(0.1));
        assert_eq!(account.total_usd, Some(1.0));
        assert_eq!(account.source, AccountSource::Key);
        assert!(account.windows.is_empty());
        assert_eq!(quota.checked_at_epoch, 1_790_000_000);
    }

    #[test]
    fn account_scope_key_reports_account_balance_even_when_unlimited() {
        let quota = parse(
            r#"{
                "code": true,
                "data": {
                    "display": { "remaining": 3.0, "total": 8, "unit": "CNY", "used": 5.0 },
                    "expires_at": 0,
                    "name": "测试",
                    "object": "token_usage",
                    "quota_scope": "account",
                    "total_available": 1500000,
                    "total_granted": 4000000,
                    "total_used": 2500000,
                    "unlimited_quota": true
                },
                "message": "ok"
            }"#,
            1,
        )
        .unwrap();

        assert!(quota.unlimited);
        assert_eq!(quota.limit_usd, None);
        assert_eq!(quota.remaining_usd, None);
        assert_eq!(quota.used_usd, None);
        assert_eq!(quota.currency.as_deref(), Some("CNY"));
        assert_eq!(quota.account_error, None);
        let account = quota.account.expect("账户额度");
        assert_eq!(account.remaining_usd, Some(3.0));
        assert_eq!(account.used_usd, Some(5.0));
        assert_eq!(account.total_usd, Some(8.0));
        assert_eq!(account.source, AccountSource::Key);
    }

    #[test]
    fn account_scope_without_display_uses_quota_units_and_status_currency() {
        let payload: Value = serde_json::from_str(
            r#"{"data":{"quota_scope":"account","total_available":1500000,
                "total_granted":4000000,"total_used":2500000,"unlimited_quota":true}}"#,
        )
        .unwrap();
        let quota = parse_usage(&payload, 500_000.0, Some("CNY"), 1).unwrap();

        assert_eq!(quota.currency.as_deref(), Some("CNY"));
        let account = quota.account.expect("账户额度");
        assert_eq!(account.remaining_usd, Some(3.0));
        assert_eq!(account.used_usd, Some(5.0));
        assert_eq!(account.total_usd, Some(8.0));
    }

    #[test]
    fn unlimited_key_has_no_limit_or_remaining() {
        let quota = parse(
            r#"{"data":{"name":"无限","total_granted":0,"total_used":0,
                "total_available":0,"unlimited_quota":true,"expires_at":0}}"#,
            1,
        )
        .unwrap();

        assert!(quota.unlimited);
        assert_eq!(quota.limit_usd, None);
        assert_eq!(quota.remaining_usd, None);
        assert_eq!(quota.expires_at_epoch, None);
        assert_eq!(quota.account, None);
    }

    #[test]
    fn error_envelope_without_data_is_reported_without_echoing_server_text() {
        let error = parse(r#"{"success":false,"message":"token not found"}"#, 1).unwrap_err();
        assert!(error.contains("未返回额度数据"));
        assert!(!error.contains("token not found"));
    }

    #[test]
    fn missing_status_uses_default_conversion() {
        let status: Value = serde_json::from_str(r#"{"success":true,"data":{}}"#).unwrap();
        assert_eq!(quota_per_unit_from_status(&status), None);
        assert_eq!(default_quota_per_unit(), 500_000.0);
    }

    #[test]
    fn status_display_type_is_normalized_to_an_uppercase_unit() {
        let status = json!({"data": {"quota_display_type": " cny "}});
        assert_eq!(
            quota_display_unit_from_status(&status),
            Some("CNY".to_string())
        );
        assert_eq!(quota_display_unit_from_status(&json!({"data": {}})), None);
    }

    #[test]
    fn unlimited_display_without_scope_is_inferred_as_account() {
        let quota = parse(
            r#"{"data":{
                "display":{"remain":"3.0","total":"8","used":"5.0","currency":"¥"},
                "unlimited_quota":"true"
            }}"#,
            1,
        )
        .unwrap();

        assert_eq!(quota.currency.as_deref(), Some("CNY"));
        let account = quota.account.expect("推断为账户额度");
        assert_eq!(account.remaining_usd, Some(3.0));
        assert_eq!(account.used_usd, Some(5.0));
        assert_eq!(account.total_usd, Some(8.0));
    }

    #[test]
    fn explicit_token_scope_does_not_turn_display_into_account_quota() {
        let quota = parse(
            r#"{"data":{
                "quota_scope":"token",
                "display":{"remaining":5,"total":10,"used":5,"unit":"USD"},
                "unlimited_quota":true
            }}"#,
            1,
        )
        .unwrap();

        assert!(quota.account.is_none());
        assert_eq!(quota.currency.as_deref(), Some("USD"));
        assert_eq!(quota.used_usd, Some(5.0));
    }

    #[test]
    fn account_self_accepts_split_balance_and_string_numbers() {
        let payload = json!({
            "success": true,
            "data": {
                "paid_balance": "1500000",
                "bonus_balance": "0",
                "used": "2500000"
            }
        });

        let account = parse_account_self(&payload, 500_000.0).unwrap();

        assert_eq!(account.remaining_usd, Some(3.0));
        assert_eq!(account.used_usd, Some(5.0));
        assert_eq!(account.total_usd, Some(8.0));
    }

    #[test]
    fn currency_symbols_are_normalized_to_codes() {
        assert_eq!(normalize_currency(" ¥ ").as_deref(), Some("CNY"));
        assert_eq!(normalize_currency("rmb").as_deref(), Some("CNY"));
        assert_eq!(normalize_currency("US$").as_deref(), Some("USD"));
        assert_eq!(normalize_currency("eur").as_deref(), Some("EUR"));
        assert_eq!(normalize_currency("  "), None);
    }

    #[test]
    fn account_self_payload_is_converted_and_tagged_with_the_token_source() {
        let payload = json!({
            "success": true,
            "data": { "quota": 1_250_000, "used_quota": 750_000 }
        });

        let account = parse_account_self(&payload, 500_000.0).unwrap();

        assert_eq!(account.remaining_usd, Some(2.5));
        assert_eq!(account.used_usd, Some(1.5));
        assert_eq!(account.total_usd, Some(4.0));
        assert_eq!(account.source, AccountSource::AccessToken);
        assert_eq!(account.windows, Vec::new());
    }

    #[test]
    fn account_payload_without_quota_fields_is_rejected_for_fallback() {
        let payload = json!({"success": true, "data": {"username": "someone"}});
        assert!(parse_account_self(&payload, 500_000.0).is_err());
    }

    #[test]
    fn active_subscriptions_become_windows_and_expired_ones_are_skipped() {
        let payload = json!({
            "success": true,
            "data": {
                "subscriptions": [
                    { "subscription": {
                        "id": 7, "amount_total": 1_000_000, "amount_used": 250_000,
                        "next_reset_time": 1_793_000_000, "status": "active"
                    }},
                    { "subscription": {
                        "id": 8, "amount_total": 500_000, "amount_used": 500_000,
                        "status": "expired"
                    }},
                    { "subscription": {
                        "id": 9, "amount_used": 100_000, "status": "active"
                    }}
                ]
            }
        });

        let windows = parse_subscriptions(&payload, 500_000.0);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window_id, "订阅 7");
        assert_eq!(windows[0].limit_usd, Some(2.0));
        assert_eq!(windows[0].used_usd, Some(0.5));
        assert_eq!(windows[0].remaining_usd, Some(1.5));
        assert_eq!(windows[0].remaining_percent, Some(75));
        assert_eq!(windows[0].resets_at_epoch, Some(1_793_000_000));
    }

    #[test]
    fn subscription_payload_that_cannot_be_parsed_yields_no_windows() {
        assert!(
            parse_subscriptions(&json!({"data": {"subscriptions": "nope"}}), 500_000.0).is_empty()
        );
    }

    #[test]
    fn account_token_is_validated_before_any_request() {
        assert!(NewApiProvider::new_with_account_token(
            "https://relay.example.com",
            "sk-test-key",
            Some("  "),
            None
        )
        .is_err());
        assert!(NewApiProvider::new_with_account_token(
            "https://relay.example.com",
            "sk-test-key",
            Some("has space"),
            None
        )
        .is_err());
        assert!(NewApiProvider::new_with_account_token(
            "https://relay.example.com",
            "sk-test-key",
            Some("pat-valid-token"),
            None
        )
        .is_ok());
        assert!(NewApiProvider::new_with_account_token(
            "https://relay.example.com",
            "sk-test-key",
            Some("pat-valid-token"),
            Some("not-a-number")
        )
        .is_err());
        assert!(NewApiProvider::new_with_account_token(
            "https://relay.example.com",
            "sk-test-key",
            Some("pat-valid-token"),
            Some("20001")
        )
        .is_ok());
    }
}
