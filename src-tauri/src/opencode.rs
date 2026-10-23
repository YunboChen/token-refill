//! OpenCode Go 订阅额度。
//!
//! 官方 usage API：`GET https://opencode.ai/zen/go/v1/usage`，用 Go API Key
//! 作为 Bearer 令牌。返回 rolling（5 小时）/ weekly / monthly 三个窗口的
//! 已用百分比与重置时间；百分比已经是账户维度的汇总值，不需要再按模型累加。

use std::time::Duration;

use serde_json::Value;

use crate::quota::{now_epoch, parse_epoch};
use crate::usage_api::{get_json, ProviderError};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// 窗口定义：API 字段、展示名与固定周期。订阅月按购买日滚动，不写死天数。
const WINDOWS: [(&str, &str, Option<i64>); 3] = [
    ("rolling", "5 小时窗口", Some(5 * 60)),
    ("weekly", "1 周窗口", Some(7 * 24 * 60)),
    ("monthly", "订阅月", None),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpencodeUsage {
    pub checked_at_epoch: i64,
    pub windows: Vec<OpencodeWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpencodeWindow {
    pub source_id: &'static str,
    pub name: &'static str,
    pub window_duration_mins: Option<i64>,
    pub used_percent: i64,
    pub resets_at_epoch: Option<i64>,
}

pub fn read_usage(api_key: &str) -> Result<OpencodeUsage, ProviderError> {
    let payload =
        get_json(USAGE_URL, api_key, REQUEST_TIMEOUT).map_err(|error| match error.status {
            Some(401) => ProviderError::new(
                "auth-expired",
                "OpenCode Go API Key 无效或已失效，请重新填写。",
            ),
            Some(403) => {
                ProviderError::new("not-subscribed", "该 API Key 没有开通 OpenCode Go 订阅。")
            }
            Some(429) => ProviderError::new("rate-limited", "OpenCode 查询过于频繁，请稍后重试。"),
            Some(status) if status >= 500 => {
                ProviderError::new("upstream", "OpenCode 服务暂时不可用，请稍后重试。")
            }
            Some(_) => ProviderError::new("http", error.message),
            None => ProviderError::new("network", error.message),
        })?;

    parse_usage(&payload, now_epoch())
}

fn parse_usage(payload: &Value, checked_at_epoch: i64) -> Result<OpencodeUsage, ProviderError> {
    let usage = payload
        .get("usage")
        .and_then(Value::as_object)
        .ok_or_else(|| ProviderError::new("decode", "OpenCode 未返回额度数据。"))?;

    let mut windows = Vec::new();
    for (source_id, name, duration) in WINDOWS {
        let Some(window) = usage.get(source_id) else {
            continue;
        };
        // `rate-limited` 表示窗口已打满；百分比字段本身可能仍停在旧值。
        let rate_limited = window
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("rate-limited"));
        let percent = window
            .get("percent")
            .and_then(Value::as_f64)
            .map(|percent| percent.round().clamp(0.0, 100.0) as i64)
            .unwrap_or(0);
        windows.push(OpencodeWindow {
            source_id,
            name,
            window_duration_mins: duration,
            used_percent: if rate_limited { 100 } else { percent },
            resets_at_epoch: window.get("resetsAt").and_then(parse_epoch),
        });
    }

    if windows.is_empty() {
        return Err(ProviderError::new(
            "decode",
            "OpenCode 未返回可识别的额度窗口。",
        ));
    }

    Ok(OpencodeUsage {
        checked_at_epoch,
        windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_windows_and_reset_times() {
        let payload = serde_json::json!({
            "usage": {
                "rolling": { "status": "ok", "percent": 12, "resetsAt": "2026-10-11T13:01:09.000Z" },
                "weekly": { "status": "ok", "percent": 46, "resetsAt": "2026-10-12T00:00:00.000Z" },
                "monthly": { "status": "ok", "percent": 82, "resetsAt": "2026-10-14T21:34:50.000Z" }
            }
        });

        let usage = parse_usage(&payload, 1_790_000_000).unwrap();
        assert_eq!(usage.windows.len(), 3);
        assert_eq!(usage.windows[0].used_percent, 12);
        assert_eq!(usage.windows[0].window_duration_mins, Some(300));
        assert_eq!(usage.windows[1].used_percent, 46);
        assert_eq!(usage.windows[2].window_duration_mins, None);
        assert_eq!(usage.windows[2].resets_at_epoch, Some(1_792_013_690));
    }

    #[test]
    fn rate_limited_pins_window_to_full() {
        let payload = serde_json::json!({
            "usage": {
                "rolling": { "status": "rate-limited", "percent": 37, "resetsAt": "2026-10-11T13:01:09.000Z" }
            }
        });

        let usage = parse_usage(&payload, 1).unwrap();
        assert_eq!(usage.windows[0].used_percent, 100);
    }

    #[test]
    fn empty_usage_is_a_decode_error() {
        let error = parse_usage(&serde_json::json!({ "usage": {} }), 1).unwrap_err();
        assert_eq!(error.code, "decode");
    }
}
