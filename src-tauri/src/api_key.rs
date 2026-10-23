//! API Key 凭证的本地存储。
//!
//! 每个账户在自己的应用私有目录里保存一份 `credentials.json`：Unix 使用
//! 0700 目录 + 0600 文件，Windows 沿用 `%APPDATA%` 的按用户 ACL。
//! 密钥只供 Rust 侧查询使用，不进入前端状态、事件或错误消息。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CREDENTIALS_FILE: &str = "credentials.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredApiKey {
    pub platform: String,
    pub api_key: String,
}

pub fn validate(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("请先填写 API Key。".to_string());
    }
    if key.len() < 8 || key.len() > 512 {
        return Err("API Key 长度不正确。".to_string());
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("API Key 不能包含空白或控制字符。".to_string());
    }
    Ok(key.to_string())
}

pub fn credentials_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(CREDENTIALS_FILE)
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("创建账户目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置账户目录权限失败：{err}"))?;
    }
    Ok(())
}

/// 先写临时文件再原子替换，避免进程中断留下半截 JSON。
pub fn save(profile_dir: &Path, platform: &str, api_key: &str) -> Result<(), String> {
    ensure_private_dir(profile_dir)?;
    let path = credentials_path(profile_dir);
    let tmp = path.with_extension("json.tmp");
    let payload = StoredApiKey {
        platform: platform.to_string(),
        api_key: api_key.to_string(),
    };
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|err| format!("序列化账户凭证失败：{err}"))?;
    fs::write(&tmp, bytes).map_err(|err| format!("写入账户凭证失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置账户凭证权限失败：{err}"))?;
    }
    fs::rename(&tmp, &path).map_err(|err| format!("保存账户凭证失败：{err}"))?;
    Ok(())
}

pub fn load(profile_dir: &Path) -> Result<StoredApiKey, String> {
    let path = credentials_path(profile_dir);
    let raw = fs::read_to_string(&path).map_err(|_| "未找到该账户的 API Key。".to_string())?;
    serde_json::from_str::<StoredApiKey>(&raw).map_err(|_| "账户凭证文件无法读取。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!(
            "token-refill-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn credentials_round_trip_through_private_file() {
        let dir = temp_dir("api-key");
        save(&dir, "opencode-go", "sk-test-key-123").unwrap();
        let stored = load(&dir).unwrap();
        assert_eq!(stored.platform, "opencode-go");
        assert_eq!(stored.api_key, "sk-test-key-123");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_keys_are_rejected_before_use() {
        assert!(validate("  ").is_err());
        assert!(validate("short").is_err());
        assert!(validate("has space in key").is_err());
        assert_eq!(
            validate("  sk-valid-key-123  ").unwrap(),
            "sk-valid-key-123"
        );
    }

    #[test]
    fn missing_file_is_a_readable_error() {
        let dir = temp_dir("api-key-missing");
        assert!(load(&dir).is_err());
    }
}
