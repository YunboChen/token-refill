//! New API：`GET /api/usage/token/` 返回当前 Bearer 密钥的授予/已用/可用额度。
//!
//! 平台额度以内部 quota 为计量单位，`/api/status` 的 `data.quota_per_unit`
//! 表示 1 美元对应的 quota 数；取不到时按上游默认的 500000 换算。

use serde::Deserialize;
use serde_json::Value;

use super::{
    default_quota_per_unit, now_epoch, quota_per_unit_from_status, round_money, AccountQuota,
    Endpoint, KeyQuota, KeyQuotaProvider, QuotaMode, QuotaProvider,
};

pub struct NewApiProvider {
    endpoint: Endpoint,
}

impl NewApiProvider {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, String> {
        Ok(Self {
            endpoint: Endpoint::new(base_url, api_key)?,
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
        let quota_per_unit = self
            .endpoint
            .get_json("/api/status")
            .ok()
            .and_then(|status| quota_per_unit_from_status(&status))
            .unwrap_or_else(default_quota_per_unit);

        parse_usage(&payload, quota_per_unit, now_epoch())
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
    #[serde(default)]
    total_granted: Option<f64>,
    #[serde(default)]
    total_used: Option<f64>,
    #[serde(default)]
    total_available: Option<f64>,
    #[serde(default)]
    unlimited_quota: bool,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    user_quota: Option<UserQuota>,
}

#[derive(Debug, Deserialize)]
struct UserQuota {
    #[serde(default)]
    total_available: Option<f64>,
    #[serde(default)]
    total_used: Option<f64>,
}

fn to_usd(quota: Option<f64>, quota_per_unit: f64) -> Option<f64> {
    quota
        .filter(|value| value.is_finite())
        .map(|value| round_money(value / quota_per_unit))
}

fn parse_usage(
    payload: &Value,
    quota_per_unit: f64,
    checked_at_epoch: i64,
) -> Result<KeyQuota, String> {
    let envelope: TokenUsageEnvelope = serde_json::from_value(payload.clone())
        .map_err(|_| "New API 返回的额度数据无法识别。".to_string())?;
    let data = envelope
        .data
        .ok_or_else(|| "New API 未返回额度数据（密钥可能无效或已禁用）。".to_string())?;

    let quota_per_unit = if quota_per_unit > 0.0 {
        quota_per_unit
    } else {
        default_quota_per_unit()
    };
    let unlimited = data.unlimited_quota;

    Ok(KeyQuota {
        provider: QuotaProvider::NewApi,
        mode: QuotaMode::Key,
        key_name: data.name,
        plan_name: None,
        valid: true,
        unlimited,
        // 无限额度时授予值与可用值都没有上限含义，只保留已用量。
        limit_usd: if unlimited {
            None
        } else {
            to_usd(data.total_granted, quota_per_unit)
        },
        used_usd: to_usd(data.total_used, quota_per_unit),
        remaining_usd: if unlimited {
            None
        } else {
            to_usd(data.total_available, quota_per_unit)
        },
        account: data.user_quota.map(|quota| AccountQuota {
            used_usd: to_usd(quota.total_used, quota_per_unit),
            remaining_usd: to_usd(quota.total_available, quota_per_unit),
        }),
        expires_at_epoch: data.expires_at.filter(|value| *value > 0),
        windows: Vec::new(),
        checked_at_epoch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str, checked_at: i64) -> Result<KeyQuota, String> {
        parse_usage(&serde_json::from_str(body).unwrap(), 500_000.0, checked_at)
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
        let account = quota.account.unwrap();
        assert_eq!(account.remaining_usd, Some(0.9));
        assert_eq!(account.used_usd, Some(0.1));
        assert_eq!(quota.checked_at_epoch, 1_790_000_000);
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
}
