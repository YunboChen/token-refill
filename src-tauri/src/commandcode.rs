//! Command Code 订阅额度。
//!
//! 使用 Provider API Key（`user_…`）访问 CLI 同款 `/alpha` 账单接口：
//! - `GET /alpha/billing/credits`：月度剩余额度与 5 小时 / 周滚动窗口；
//! - `GET /alpha/usage/summary`：本账期已用额度（可选）；
//! - `GET /alpha/billing/subscriptions`：套餐与账期结束时间（可选）。
//!
//! 只有 credits 是必需数据；另外两个接口失败时仍展示已有窗口，并记录能力缺口。

use std::time::Duration;

use serde_json::Value;

use crate::quota::{now_epoch, parse_epoch};
use crate::usage_api::{get_json, HttpError, ProviderError};

const API_BASE: &str = "https://api.commandcode.ai/alpha";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub struct CommandCodeUsage {
    pub checked_at_epoch: i64,
    pub plan_tier: Option<String>,
    pub monthly: MonthlyCredits,
    pub five_hour: Option<RollingWindow>,
    pub weekly: Option<RollingWindow>,
    pub capability_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RollingWindow {
    pub used: f64,
    pub cap: f64,
    pub used_percent: i64,
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyCredits {
    /// 月度赠额剩余；可能包含单独购买的余额。
    pub remaining: f64,
    pub used: Option<f64>,
    pub total: Option<f64>,
    pub used_percent: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

pub fn read_usage(api_key: &str) -> Result<CommandCodeUsage, ProviderError> {
    let checked_at_epoch = now_epoch();
    let credits = get_json(
        &format!("{API_BASE}/billing/credits"),
        api_key,
        REQUEST_TIMEOUT,
    )
    .map_err(map_credits_error)?;

    // 两个辅助接口都只影响展示完整度，失败不应丢掉已经拿到的滚动窗口。
    let summary = get_json(
        &format!("{API_BASE}/usage/summary"),
        api_key,
        REQUEST_TIMEOUT,
    )
    .ok();
    let subscription = get_json(
        &format!("{API_BASE}/billing/subscriptions"),
        api_key,
        REQUEST_TIMEOUT,
    )
    .ok();

    parse_usage(
        &credits,
        summary.as_ref(),
        subscription.as_ref(),
        checked_at_epoch,
    )
}

fn map_credits_error(error: HttpError) -> ProviderError {
    match error.status {
        Some(401 | 403) => ProviderError::new(
            "auth-expired",
            "Command Code API Key 无效或已失效，请重新填写。",
        ),
        Some(404) => ProviderError::new(
            "upstream",
            "Command Code 额度接口不可用，请确认 Provider API Key。",
        ),
        Some(429) => ProviderError::new("rate-limited", "Command Code 查询过于频繁，请稍后重试。"),
        Some(status) if status >= 500 => {
            ProviderError::new("upstream", "Command Code 服务暂时不可用，请稍后重试。")
        }
        Some(_) => ProviderError::new("http", error.message),
        None => ProviderError::new("network", error.message),
    }
}

fn parse_usage(
    credits: &Value,
    summary: Option<&Value>,
    subscription: Option<&Value>,
    checked_at_epoch: i64,
) -> Result<CommandCodeUsage, ProviderError> {
    let credits_object = credits
        .get("credits")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::new("decode", "Command Code 未返回额度数据。"))?;
    let monthly_credits = number(credits_object.get("monthlyCredits"))
        .ok_or_else(|| ProviderError::new("decode", "Command Code 未返回月度剩余额度。"))?;
    let purchased = number(credits_object.get("purchasedCredits")).unwrap_or(0.0);
    let remaining = (monthly_credits + purchased).max(0.0);

    let window_limits = credits
        .get("windowLimits")
        .or_else(|| credits_object.get("windowLimits"));
    let five_hour = window_limits.and_then(|limits| parse_rolling(limits.get("fiveHour")));
    let weekly = window_limits.and_then(|limits| parse_rolling(limits.get("weekly")));

    let subscription = subscription.and_then(parse_subscription);
    let plan_tier = subscription
        .as_ref()
        .and_then(|sub| plan_label(&sub.plan_id))
        .or_else(|| infer_plan(five_hour.as_ref(), weekly.as_ref()));
    let resets_at_epoch = subscription.as_ref().and_then(|sub| sub.period_end);

    let used = summary
        .and_then(|value| number(value.get("totalMonthlyCredits")))
        .filter(|value| *value > 0.0);
    let total = used.map(|used| used + remaining);
    let used_percent = match (used, total) {
        (Some(used), Some(total)) if total > 0.0 => {
            Some(((used * 100.0) / total).round().clamp(0.0, 100.0) as i64)
        }
        _ => None,
    };

    let mut capability_gaps = Vec::new();
    if used.is_none() {
        capability_gaps.push("平台未返回本月已用额度".to_string());
    }
    if five_hour.is_none() {
        capability_gaps.push("平台未返回 5 小时滚动窗口".to_string());
    }
    if weekly.is_none() {
        capability_gaps.push("平台未返回每周滚动窗口".to_string());
    }

    Ok(CommandCodeUsage {
        checked_at_epoch,
        plan_tier,
        monthly: MonthlyCredits {
            remaining,
            used,
            total,
            used_percent,
            resets_at_epoch,
        },
        five_hour,
        weekly,
        capability_gaps,
    })
}

struct Subscription {
    plan_id: String,
    period_end: Option<i64>,
}

fn parse_subscription(payload: &Value) -> Option<Subscription> {
    if payload.get("success").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let data = payload.get("data")?.as_object()?;
    let plan_id = data.get("planId")?.as_str()?.trim().to_string();
    if plan_id.is_empty() {
        return None;
    }
    Some(Subscription {
        plan_id,
        period_end: data.get("currentPeriodEnd").and_then(parse_epoch),
    })
}

fn parse_rolling(value: Option<&Value>) -> Option<RollingWindow> {
    let value = value?.as_object()?;
    let cap = number(value.get("cap"))?;
    if cap <= 0.0 {
        return None;
    }
    let used = number(value.get("used")).unwrap_or(0.0).max(0.0);
    Some(RollingWindow {
        used,
        cap,
        used_percent: ((used / cap) * 100.0).round().clamp(0.0, 100.0) as i64,
        resets_at_epoch: value.get("resetAt").and_then(parse_epoch),
    })
}

/// 上游字段可能是数字，也可能被序列化成字符串。
fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => text.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

fn plan_label(plan_id: &str) -> Option<String> {
    let label = match plan_id {
        "individual-go" | "individual-go-v1" => "Go",
        "individual-pro" => "Pro",
        "individual-max" => "Max",
        "individual-ultra" => "Ultra",
        other => other,
    };
    Some(label.to_string())
}

/// 订阅接口不可用时，用滚动窗口上限反推套餐；数值来自公开套餐配置。
fn infer_plan(five_hour: Option<&RollingWindow>, weekly: Option<&RollingWindow>) -> Option<String> {
    let five = five_hour?.cap;
    let week = weekly?.cap;
    let matches = |value: f64, target: f64| (value - target).abs() < 0.01;
    let label = if matches(five, 3.0) && matches(week, 6.0) {
        "Go"
    } else if matches(five, 16.0) && matches(week, 40.0) {
        "Pro"
    } else if matches(five, 45.0) && matches(week, 90.0) {
        "Max 10×"
    } else if matches(five, 90.0) && matches(week, 180.0) {
        "Max 20×"
    } else if matches(five, 14.0) && matches(week, 35.0) {
        "GOAT"
    } else {
        return None;
    };
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits() -> Value {
        serde_json::json!({
            "credits": {
                "monthlyCredits": 8.5,
                "purchasedCredits": 0.0,
                "premiumMonthlyCredits": 0.0,
                "opensourceMonthlyCredits": 8.5
            },
            "windowLimits": {
                "fiveHour": { "used": 2.5, "cap": 10.0, "resetAt": "2026-10-11T13:00:00Z" },
                "weekly": { "used": 4.0, "cap": 40.0, "resetAt": "2026-10-12T00:00:00Z" }
            }
        })
    }

    fn summary() -> Value {
        serde_json::json!({ "totalMonthlyCredits": 11.5 })
    }

    fn subscription() -> Value {
        serde_json::json!({
            "success": true,
            "data": {
                "planId": "individual-go",
                "status": "active",
                "currentPeriodEnd": "2026-11-06T07:28:50.000Z"
            }
        })
    }

    #[test]
    fn derives_rolling_and_monthly_windows() {
        let usage = parse_usage(&credits(), Some(&summary()), Some(&subscription()), 1).unwrap();

        let five = usage.five_hour.unwrap();
        assert_eq!(five.used_percent, 25);
        assert_eq!(usage.weekly.unwrap().used_percent, 10);
        assert_eq!(usage.monthly.used, Some(11.5));
        assert_eq!(usage.monthly.total, Some(20.0));
        assert_eq!(usage.monthly.used_percent, Some(58));
        assert_eq!(usage.plan_tier.as_deref(), Some("Go"));
        assert!(usage.capability_gaps.is_empty());
    }

    #[test]
    fn keeps_credits_when_summary_is_unavailable() {
        let usage = parse_usage(&credits(), None, None, 1).unwrap();

        assert_eq!(usage.monthly.remaining, 8.5);
        assert_eq!(usage.monthly.used, None);
        assert_eq!(usage.monthly.used_percent, None);
        // 10/40 不在已知套餐上限里，订阅接口缺失时保持未知。
        assert!(usage.plan_tier.is_none());
        assert!(usage
            .capability_gaps
            .iter()
            .any(|gap| gap.contains("本月已用")));
    }

    #[test]
    fn accepts_nested_window_limits_and_string_numbers() {
        let payload = serde_json::json!({
            "credits": {
                "monthlyCredits": "7.25",
                "windowLimits": {
                    "fiveHour": { "used": "2", "cap": "8", "resetAt": 1_780_200_000 }
                }
            }
        });

        let usage = parse_usage(&payload, None, None, 1).unwrap();
        assert_eq!(usage.monthly.remaining, 7.25);
        assert_eq!(usage.five_hour.unwrap().used_percent, 25);
    }

    #[test]
    fn missing_monthly_credits_is_a_decode_error() {
        let error = parse_usage(&serde_json::json!({ "credits": {} }), None, None, 1).unwrap_err();
        assert_eq!(error.code, "decode");
    }
}
