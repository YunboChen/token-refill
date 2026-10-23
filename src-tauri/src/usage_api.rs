//! 第三方订阅额度接口的共用 HTTP 与错误类型。
//!
//! 只在 Rust 侧发起请求；API Key 不进入错误消息、日志或前端状态。
//! 响应正文不写入错误，避免把上游返回的账户数据带出去。

use std::time::Duration;

use serde_json::Value;

const MAX_RESPONSE_BYTES: u64 = 512 * 1024;

/// 统一的查询错误。`code` 供状态层判断是否需要重新授权。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
}

impl ProviderError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    /// 只有明确的凭证问题才把账户标记为需要重新授权；网络与限流不算。
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self.code.as_str(),
            "auth-expired" | "invalid-key" | "not-logged-in" | "credentials-missing"
        )
    }
}

/// 传输层错误；保留 HTTP 状态码供各平台映射成自己的提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub status: Option<u16>,
    pub message: String,
}

impl HttpError {
    fn network(_error: ureq::Error) -> Self {
        Self {
            status: None,
            message: "无法连接到额度服务，请检查网络后重试。".to_string(),
        }
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// 发起带 Bearer 认证的 JSON GET 请求。
pub fn get_json(url: &str, api_key: &str, timeout: Duration) -> Result<Value, HttpError> {
    let request = agent(timeout)
        .get(url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {api_key}"));

    let mut response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => {
            return Err(HttpError {
                status: Some(status),
                message: format!("额度接口返回 HTTP {status}。"),
            })
        }
        Err(error) => return Err(HttpError::network(error)),
    };

    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|_| HttpError {
            status: None,
            message: "读取额度响应失败。".to_string(),
        })?;

    serde_json::from_str(&body).map_err(|_| HttpError {
        status: None,
        message: "额度响应格式无法识别。".to_string(),
    })
}
