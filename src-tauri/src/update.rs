//! 版本检查：查询 GitHub Releases 的最新正式版本，与当前构建版本比较。
//!
//! 只做“检查 + 跳转下载页”，不下载也不替换正在运行的程序；自动更新需要签名
//! 密钥与安装器集成，未纳入本次范围。查询在 Windows、macOS 上走同一套逻辑。

use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};

/// 发布仓库；与 release workflow 创建 Release 的仓库一致。
pub const RELEASES_REPO: &str = "YunboChen/token-refill";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
}

/// GitHub Releases API 中本功能用到的字段；其余忽略。
#[derive(Debug, Deserialize)]
struct ReleasePayload {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// 去掉 tag 的 `v` 前缀，交给 semver 解析。
fn normalize_tag(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

fn parse_version(raw: &str) -> Option<Version> {
    Version::parse(normalize_tag(raw)).ok()
}

/// 只有两端都能解析成 semver 时才比较；格式不认识的 tag 不宣称有新版本。
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn status_from_release(payload: ReleasePayload, current: &str) -> UpdateStatus {
    let latest = normalize_tag(&payload.tag_name).to_string();
    let update_available =
        !payload.draft && !payload.prerelease && is_newer(&payload.tag_name, current);
    UpdateStatus {
        current_version: current.to_string(),
        latest_version: Some(latest),
        update_available,
        release_url: payload.html_url,
        release_notes: payload.body,
        published_at: payload.published_at,
    }
}

fn map_request_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => "发布仓库暂无正式版本。".to_string(),
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub 接口访问受限（可能是频率限制），请稍后再试。".to_string()
        }
        other => format!("检查更新失败：{other}"),
    }
}

/// 读取最新正式 Release 并与当前版本比较。
pub fn check_for_update() -> Result<UpdateStatus, String> {
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest");
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("TokenRefill/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent: ureq::Agent = config.into();

    let mut response = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(map_request_error)?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("读取版本信息失败：{err}"))?;
    let payload: ReleasePayload = serde_json::from_str(&body)
        .map_err(|_| "版本信息格式无法识别，请稍后再试。".to_string())?;

    Ok(status_from_release(payload, env!("CARGO_PKG_VERSION")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(tag: &str) -> ReleasePayload {
        ReleasePayload {
            tag_name: tag.to_string(),
            html_url: Some(format!(
                "https://github.com/{RELEASES_REPO}/releases/tag/{tag}"
            )),
            body: Some("release notes".to_string()),
            published_at: Some("2026-10-23T00:00:00Z".to_string()),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn newer_tag_with_prefix_marks_update_available() {
        let status = status_from_release(payload("v0.2.0"), "0.1.0");
        assert!(status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("0.2.0"));
        assert_eq!(status.current_version, "0.1.0");
    }

    #[test]
    fn same_or_older_version_does_not_offer_update() {
        assert!(!status_from_release(payload("v0.1.0"), "0.1.0").update_available);
        assert!(!status_from_release(payload("v0.0.9"), "0.1.0").update_available);
    }

    #[test]
    fn prerelease_and_draft_never_offer_update() {
        let mut prerelease = payload("v0.3.0");
        prerelease.prerelease = true;
        assert!(!status_from_release(prerelease, "0.1.0").update_available);

        let mut draft = payload("v0.3.0");
        draft.draft = true;
        assert!(!status_from_release(draft, "0.1.0").update_available);
    }

    #[test]
    fn unparsable_tag_is_not_treated_as_newer() {
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("v0.2.0", "dev-build"));
        let status = status_from_release(payload("nightly"), "0.1.0");
        assert!(!status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("nightly"));
    }

    #[test]
    fn tag_prefix_and_whitespace_are_normalized() {
        assert_eq!(normalize_tag(" v1.2.3 "), "1.2.3");
        assert!(is_newer("V1.10.0", "1.9.0"));
    }
}
