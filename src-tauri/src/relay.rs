//! 中转站密钥的会话级注册表。
//!
//! 用户在界面里填入服务地址与 API Key 后，由 Rust 自动识别 New API / Sub2API
//! 并缓存查询结果。密钥只存在于本进程内存：不写 `state.json`，不进入前端状态，
//! 退出应用即清除。系统级凭证存储落地前，这是不把密钥落盘的最小方案。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;

use crate::discovery;
use crate::quota::{self, DetectedQuota, KeyQuota, QuotaProvider};

/// 会话内最多保留的条目，避免误粘贴大量密钥后长期占用内存。
const MAX_ENTRIES: usize = 20;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 密钥来源；手动输入与本地配置自动读取共用同一份会话注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelayOrigin {
    Manual,
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayAccount {
    pub id: String,
    pub base_url: String,
    pub origin: RelayOrigin,
    pub provider: QuotaProvider,
    pub quota: Option<KeyQuota>,
    pub checked_at_epoch: Option<i64>,
    /// 最近一次刷新失败的原因；保留上一次成功的额度。
    pub last_error: Option<String>,
}

/// 扫描到、但没能识别为 New API / Sub2API 的本地配置。不含密钥。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedConfig {
    pub origin: RelayOrigin,
    pub base_url: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub accounts: Vec<RelayAccount>,
    pub skipped: Vec<SkippedConfig>,
}

struct RelayEntry {
    id: String,
    base_url: String,
    origin: RelayOrigin,
    api_key: String,
    provider: QuotaProvider,
    quota: Option<KeyQuota>,
    checked_at_epoch: Option<i64>,
    last_error: Option<String>,
}

impl RelayEntry {
    fn view(&self) -> RelayAccount {
        RelayAccount {
            id: self.id.clone(),
            base_url: self.base_url.clone(),
            origin: self.origin,
            provider: self.provider,
            quota: self.quota.clone(),
            checked_at_epoch: self.checked_at_epoch,
            last_error: self.last_error.clone(),
        }
    }
}

fn skipped_config(candidate: &discovery::ConfigCandidate, reason: String) -> SkippedConfig {
    SkippedConfig {
        origin: candidate.origin,
        base_url: quota::normalize_base_url(&candidate.base_url)
            .unwrap_or_else(|_| candidate.base_url.clone()),
        reason,
    }
}

#[derive(Default)]
pub struct RelayStore {
    entries: Mutex<Vec<RelayEntry>>,
}

impl RelayStore {
    fn lock(&self) -> MutexGuard<'_, Vec<RelayEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 自动识别上游并查询；同一服务地址 + 同一密钥复用原条目。
    pub fn query(&self, base_url: &str, api_key: &str) -> Result<RelayAccount, String> {
        let detected = quota::detect_and_fetch(base_url, api_key)?;
        self.upsert(RelayOrigin::Manual, base_url, api_key, detected)
    }

    /// 扫描 Claude Code / Codex 配置并自动识别；失败的配置只报告来源与原因。
    pub fn discover(&self) -> DiscoveryReport {
        let mut skipped = Vec::new();
        for candidate in discovery::discover_candidates() {
            match quota::detect_and_fetch(&candidate.base_url, &candidate.api_key) {
                Ok(detected) => {
                    if let Err(reason) = self.upsert(
                        candidate.origin,
                        &candidate.base_url,
                        &candidate.api_key,
                        detected,
                    ) {
                        skipped.push(skipped_config(&candidate, reason));
                    }
                }
                Err(reason) => skipped.push(skipped_config(&candidate, reason)),
            }
        }
        DiscoveryReport {
            accounts: self.list(),
            skipped,
        }
    }

    fn upsert(
        &self,
        origin: RelayOrigin,
        base_url: &str,
        api_key: &str,
        detected: DetectedQuota,
    ) -> Result<RelayAccount, String> {
        let base_url = quota::normalize_base_url(base_url)?;
        let api_key = quota::validate_api_key(api_key)?;
        let checked_at_epoch = Some(detected.quota.checked_at_epoch);

        let mut entries = self.lock();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.base_url == base_url && entry.api_key == api_key)
        {
            entry.provider = detected.provider;
            entry.quota = Some(detected.quota);
            entry.checked_at_epoch = checked_at_epoch;
            entry.last_error = None;
            return Ok(entry.view());
        }

        if entries.len() >= MAX_ENTRIES {
            return Err(format!(
                "本次运行最多保存 {MAX_ENTRIES} 个中转站密钥，请先移除不用的条目。"
            ));
        }

        let entry = RelayEntry {
            id: format!("relay-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            base_url,
            origin,
            api_key,
            provider: detected.provider,
            quota: Some(detected.quota),
            checked_at_epoch,
            last_error: None,
        };
        let view = entry.view();
        entries.push(entry);
        Ok(view)
    }

    /// 使用已保存的密钥重新识别并查询；失败时保留旧额度并记录错误。
    pub fn refresh(&self, id: &str) -> Result<RelayAccount, String> {
        let (base_url, api_key) = {
            let entries = self.lock();
            let entry = entries
                .iter()
                .find(|entry| entry.id == id)
                .ok_or_else(|| "找不到这条中转站账户，请重新添加。".to_string())?;
            (entry.base_url.clone(), entry.api_key.clone())
        };

        let result = quota::detect_and_fetch(&base_url, &api_key);

        let mut entries = self.lock();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| "找不到这条中转站账户，请重新添加。".to_string())?;
        match result {
            Ok(detected) => {
                entry.provider = detected.provider;
                entry.checked_at_epoch = Some(detected.quota.checked_at_epoch);
                entry.quota = Some(detected.quota);
                entry.last_error = None;
            }
            Err(error) => entry.last_error = Some(error),
        }
        Ok(entry.view())
    }

    pub fn list(&self) -> Vec<RelayAccount> {
        self.lock().iter().map(RelayEntry::view).collect()
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        if entries.len() == before {
            return Err("找不到这条中转站账户。".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn serve(steps: Vec<(&'static str, u16, &'static str)>) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定回环端口");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        thread::spawn(move || {
            for (path, status, body) in steps {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]);
                assert!(request.starts_with(&format!("GET {path} ")), "{request}");
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        base
    }

    #[test]
    fn query_detects_sub2api_and_reuses_the_entry() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let store = RelayStore::default();

        let first = store.query(&base, "sk-test").unwrap();
        assert_eq!(first.provider, QuotaProvider::Sub2Api);
        assert_eq!(first.quota.as_ref().unwrap().remaining_usd, Some(4.0));

        let second = store.query(&base, "sk-test").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn refresh_failure_keeps_the_previous_quota() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
            ("/api/usage/token/", 401, "{}"),
            ("/v1/usage", 401, "{}"),
        ]);
        let store = RelayStore::default();
        let account = store.query(&base, "sk-test").unwrap();

        let refreshed = store.refresh(&account.id).unwrap();
        assert!(refreshed.last_error.is_some());
        assert_eq!(refreshed.quota.as_ref().unwrap().remaining_usd, Some(4.0));
    }

    #[test]
    fn remove_drops_the_entry_and_secret() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let store = RelayStore::default();
        let account = store.query(&base, "sk-test").unwrap();

        store.remove(&account.id).unwrap();
        assert!(store.list().is_empty());
        assert!(store.refresh(&account.id).is_err());
    }
}
