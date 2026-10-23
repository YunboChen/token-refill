//! Sub2API：`GET /v1/usage` 按密钥状态返回三种口径。
//!
//! - `mode: quota_limited`：密钥总额度（USD）与可选的 5h/1d/7d 限速窗口；
//! - `mode: unrestricted` + `subscription`：订阅套餐的日/周/月用量与上限；
//! - `mode: unrestricted` + `balance`：账户钱包余额。
//!
//! 订阅口径下 `remaining` 为 -1 表示没有配置任何周期上限（无限额度）。
//! 日/月窗口的重置边界平台没有给出，保留为缺失而不是推测。

use serde::Deserialize;
use serde_json::Value;

use super::{
    now_epoch, parse_epoch, remaining_percent, round_money, Endpoint, KeyQuota, KeyQuotaProvider,
    KeyQuotaWindow, QuotaMode, QuotaProvider,
};

pub struct Sub2ApiProvider {
    endpoint: Endpoint,
}

impl Sub2ApiProvider {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        Ok(Self {
            endpoint: Endpoint::new(base_url, api_key)?,
        })
    }
}

impl KeyQuotaProvider for Sub2ApiProvider {
    fn provider(&self) -> QuotaProvider {
        QuotaProvider::Sub2Api
    }

    fn fetch_quota(&self) -> Result<KeyQuota, String> {
        let payload = self.endpoint.get_json_authenticated("/v1/usage")?;
        parse_usage(&payload, now_epoch())
    }
}

#[derive(Debug, Default, Deserialize)]
struct UsagePayload {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default, rename = "isValid")]
    is_valid: Option<bool>,
    #[serde(default, rename = "planName")]
    plan_name: Option<String>,
    #[serde(default)]
    quota: Option<QuotaBlock>,
    #[serde(default)]
    remaining: Option<f64>,
    #[serde(default)]
    balance: Option<f64>,
    #[serde(default)]
    subscription: Option<Subscription>,
    #[serde(default, rename = "rate_limits")]
    rate_limits: Vec<RateLimit>,
    #[serde(default)]
    expires_at: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct QuotaBlock {
    #[serde(default)]
    limit: Option<f64>,
    #[serde(default)]
    used: Option<f64>,
    #[serde(default)]
    remaining: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct RateLimit {
    #[serde(default)]
    window: String,
    #[serde(default)]
    limit: Option<f64>,
    #[serde(default)]
    used: Option<f64>,
    #[serde(default)]
    remaining: Option<f64>,
    #[serde(default, rename = "reset_at")]
    reset_at: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Subscription {
    #[serde(default)]
    daily_usage_usd: Option<f64>,
    #[serde(default)]
    weekly_usage_usd: Option<f64>,
    #[serde(default)]
    monthly_usage_usd: Option<f64>,
    #[serde(default)]
    daily_limit_usd: Option<f64>,
    #[serde(default)]
    weekly_limit_usd: Option<f64>,
    #[serde(default)]
    monthly_limit_usd: Option<f64>,
    #[serde(default)]
    weekly_window_start: Option<Value>,
    #[serde(default)]
    expires_at: Option<Value>,
}

fn parse_usage(payload: &Value, checked_at_epoch: i64) -> Result<KeyQuota, String> {
    let payload: UsagePayload = serde_json::from_value(payload.clone())
        .map_err(|_| "Sub2API 返回的额度数据无法识别。".to_string())?;

    let mode = match payload.mode.as_deref() {
        Some("quota_limited") => QuotaMode::Key,
        Some("unrestricted") if payload.subscription.is_some() => QuotaMode::Subscription,
        Some("unrestricted") if payload.balance.is_some() => QuotaMode::Wallet,
        Some("unrestricted") => QuotaMode::Unrestricted,
        // 未见过的 mode 按字段推断，避免服务端升级后整条查询失败。
        _ if payload.quota.is_some() => QuotaMode::Key,
        _ if payload.subscription.is_some() => QuotaMode::Subscription,
        _ if payload.balance.is_some() => QuotaMode::Wallet,
        _ => {
            return Err("Sub2API 未返回可识别的额度数据。".to_string());
        }
    };

    let (limit_usd, used_usd, remaining_usd, unlimited, windows, expires_at_epoch) = match mode {
        QuotaMode::Key => {
            let quota = payload.quota.unwrap_or_default();
            let limit = quota.limit.map(round_money);
            let used = quota.used.map(round_money);
            let remaining = quota.remaining.or(payload.remaining).map(round_money);
            (
                limit,
                used,
                remaining,
                limit.is_none(),
                rate_limit_windows(payload.rate_limits),
                parse_epoch_value(payload.expires_at),
            )
        }
        QuotaMode::Subscription => {
            let subscription = payload.subscription.unwrap_or_default();
            let raw_remaining = payload.remaining;
            let unlimited = raw_remaining.map(|value| value < 0.0).unwrap_or(true);
            let remaining = raw_remaining.filter(|value| *value >= 0.0).map(round_money);
            let expires = parse_epoch_value(payload.expires_at)
                .or_else(|| parse_epoch_value(subscription.expires_at.clone()));
            (
                None,
                None,
                remaining,
                unlimited,
                subscription_windows(&subscription),
                expires,
            )
        }
        QuotaMode::Wallet => {
            let remaining = payload.remaining.or(payload.balance).map(round_money);
            (
                None,
                None,
                remaining,
                false,
                Vec::new(),
                parse_epoch_value(payload.expires_at),
            )
        }
        QuotaMode::Unrestricted => {
            let raw_remaining = payload.remaining;
            let unlimited = raw_remaining.map(|value| value < 0.0).unwrap_or(true);
            let remaining = raw_remaining.filter(|value| *value >= 0.0).map(round_money);
            (
                None,
                None,
                remaining,
                unlimited,
                Vec::new(),
                parse_epoch_value(payload.expires_at),
            )
        }
    };

    Ok(KeyQuota {
        provider: QuotaProvider::Sub2Api,
        mode,
        key_name: None,
        plan_name: payload.plan_name,
        valid: payload.is_valid.unwrap_or(true),
        unlimited,
        limit_usd,
        used_usd,
        remaining_usd,
        account: None,
        expires_at_epoch,
        windows,
        checked_at_epoch,
    })
}

fn parse_epoch_value(value: Option<Value>) -> Option<i64> {
    value.as_ref().and_then(parse_epoch)
}

fn rate_limit_windows(limits: Vec<RateLimit>) -> Vec<KeyQuotaWindow> {
    limits
        .into_iter()
        .filter(|limit| !limit.window.is_empty())
        .map(|limit| {
            let limit_usd = limit.limit.map(round_money);
            let used_usd = limit.used.map(round_money);
            let remaining_usd = limit.remaining.map(round_money).or_else(|| {
                limit_usd.map(|total| round_money((total - used_usd.unwrap_or(0.0)).max(0.0)))
            });
            KeyQuotaWindow {
                duration_mins: window_duration_mins(&limit.window),
                window_id: limit.window,
                limit_usd,
                used_usd,
                remaining_usd,
                remaining_percent: remaining_percent(limit_usd, remaining_usd),
                resets_at_epoch: limit.reset_at.as_ref().and_then(parse_epoch),
            }
        })
        .collect()
}

fn subscription_windows(subscription: &Subscription) -> Vec<KeyQuotaWindow> {
    let windows = [
        (
            "daily",
            1_440,
            subscription.daily_limit_usd,
            subscription.daily_usage_usd,
            None,
        ),
        (
            "weekly",
            10_080,
            subscription.weekly_limit_usd,
            subscription.weekly_usage_usd,
            subscription.weekly_window_start.as_ref(),
        ),
        (
            "monthly",
            43_200,
            subscription.monthly_limit_usd,
            subscription.monthly_usage_usd,
            None,
        ),
    ];

    windows
        .into_iter()
        .filter(|(_, _, limit, used, _)| limit.is_some() || used.is_some())
        .map(|(window_id, duration_mins, limit, used, window_start)| {
            let limit_usd = limit.map(round_money);
            let used_usd = used.map(round_money);
            let remaining_usd = limit_usd
                .map(|total| round_money((total - used_usd.unwrap_or(0.0)).clamp(0.0, total)));
            // 只有周窗口给出了起点，按 7 天补出重置时间；日/月边界不推测。
            let resets_at_epoch = window_start
                .and_then(parse_epoch)
                .map(|start| start + 7 * 86_400);
            KeyQuotaWindow {
                window_id: window_id.to_string(),
                duration_mins: Some(duration_mins),
                limit_usd,
                used_usd,
                remaining_usd,
                remaining_percent: remaining_percent(limit_usd, remaining_usd),
                resets_at_epoch,
            }
        })
        .collect()
}

fn window_duration_mins(window: &str) -> Option<i64> {
    match window {
        "5h" => Some(300),
        "1d" | "daily" => Some(1_440),
        "7d" | "weekly" => Some(10_080),
        "monthly" => Some(43_200),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> Result<KeyQuota, String> {
        parse_usage(&serde_json::from_str(body).unwrap(), 1_790_000_000)
    }

    #[test]
    fn quota_limited_key_reads_total_and_rate_windows() {
        let quota = parse(
            r#"{
                "mode": "quota_limited",
                "isValid": true,
                "status": "active",
                "quota": { "limit": 20, "used": 7.5, "remaining": 12.5, "unit": "USD" },
                "remaining": 12.5,
                "unit": "USD",
                "rate_limits": [
                    {"window": "5h", "limit": 5, "used": 1.25, "remaining": 3.75, "reset_at": "2026-10-23T08:00:00Z"},
                    {"window": "7d", "limit": 10, "used": 4, "remaining": 6, "reset_at": "2026-10-27T00:00:00Z"}
                ],
                "expires_at": "2026-12-31T00:00:00Z"
            }"#,
        )
        .unwrap();

        assert_eq!(quota.provider, QuotaProvider::Sub2Api);
        assert_eq!(quota.mode, QuotaMode::Key);
        assert!(quota.valid);
        assert!(!quota.unlimited);
        assert_eq!(quota.limit_usd, Some(20.0));
        assert_eq!(quota.used_usd, Some(7.5));
        assert_eq!(quota.remaining_usd, Some(12.5));
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[0].window_id, "5h");
        assert_eq!(quota.windows[0].duration_mins, Some(300));
        assert_eq!(quota.windows[0].remaining_percent, Some(75));
        assert!(quota.windows[0].resets_at_epoch.is_some());
        assert_eq!(quota.windows[1].window_id, "7d");
        assert_eq!(quota.windows[1].remaining_percent, Some(60));
        assert!(quota.expires_at_epoch.is_some());
    }

    #[test]
    fn rate_limits_without_total_quota_are_not_unlimited_windows() {
        let quota = parse(
            r#"{"mode":"quota_limited","isValid":true,
                "rate_limits":[{"window":"5h","limit":2,"used":0.5}]}"#,
        )
        .unwrap();

        assert_eq!(quota.mode, QuotaMode::Key);
        assert!(quota.unlimited);
        assert_eq!(quota.limit_usd, None);
        assert_eq!(quota.windows[0].remaining_usd, Some(1.5));
        assert_eq!(quota.windows[0].remaining_percent, Some(75));
    }

    #[test]
    fn subscription_windows_report_percentages_and_weekly_reset() {
        let quota = parse(
            r#"{
                "mode": "unrestricted",
                "isValid": true,
                "planName": "Pro",
                "unit": "USD",
                "remaining": 8.25,
                "subscription": {
                    "daily_usage_usd": 2, "daily_limit_usd": 10,
                    "weekly_usage_usd": 12, "weekly_limit_usd": 20,
                    "monthly_usage_usd": 31.75, "monthly_limit_usd": 40,
                    "weekly_window_start": "2026-10-19T00:00:00Z"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(quota.mode, QuotaMode::Subscription);
        assert_eq!(quota.plan_name.as_deref(), Some("Pro"));
        assert!(!quota.unlimited);
        assert_eq!(quota.remaining_usd, Some(8.25));
        assert_eq!(quota.windows.len(), 3);
        assert_eq!(quota.windows[0].window_id, "daily");
        assert_eq!(quota.windows[0].remaining_percent, Some(80));
        assert_eq!(quota.windows[1].window_id, "weekly");
        assert_eq!(quota.windows[1].remaining_percent, Some(40));
        assert_eq!(
            quota.windows[1].resets_at_epoch,
            Some(1_792_972_800) // 2026-10-26T00:00:00Z
        );
        assert_eq!(quota.windows[2].remaining_percent, Some(21)); // 8.25 / 40 = 20.625 → 21
    }

    #[test]
    fn subscription_without_limits_is_unlimited() {
        let quota = parse(
            r#"{"mode":"unrestricted","isValid":true,"planName":"Team","remaining":-1,
                "subscription":{"daily_usage_usd":3,"weekly_usage_usd":4,"monthly_usage_usd":5}}"#,
        )
        .unwrap();

        assert_eq!(quota.mode, QuotaMode::Subscription);
        assert!(quota.unlimited);
        assert_eq!(quota.remaining_usd, None);
        assert_eq!(quota.windows.len(), 3);
        assert!(quota
            .windows
            .iter()
            .all(|window| window.limit_usd.is_none()));
    }

    #[test]
    fn wallet_mode_reads_balance() {
        let quota = parse(
            r#"{"mode":"unrestricted","isValid":false,"planName":"钱包余额",
                "remaining":3.5,"balance":3.5,"unit":"USD"}"#,
        )
        .unwrap();

        assert_eq!(quota.mode, QuotaMode::Wallet);
        assert!(!quota.valid);
        assert_eq!(quota.remaining_usd, Some(3.5));
        assert!(quota.windows.is_empty());
    }

    #[test]
    fn empty_payload_is_an_error() {
        assert!(parse(r#"{"mode":"something_new","isValid":true}"#).is_err());
    }

    #[test]
    fn window_duration_is_mapped_for_known_ids() {
        assert_eq!(window_duration_mins("5h"), Some(300));
        assert_eq!(window_duration_mins("monthly"), Some(43_200));
        assert_eq!(window_duration_mins("custom"), None);
    }
}
