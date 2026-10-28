//! 中转站密钥的会话与本地存储。
//!
//! 用户手动填入的服务地址、API Key 与可选的 New API 访问令牌保存在
//! `relay-profiles/<relayId>/credentials.json`：Unix 使用 0700 目录 + 0600 文件，
//! Windows 沿用 `%APPDATA%` 的按用户 ACL。凭证只供 Rust 侧查询使用，不进入
//! 前端状态、事件或错误消息。
//!
//! 从 cc-switch、Claude Code / Codex 本地配置扫描到的密钥不复制到应用目录：
//! 用户勾选导入后，只在 `relay-profiles/<relayId>/source.json` 记录来源引用
//! （来源、provider id、地址等，不含密钥），下次启动时重新读取源配置取出密钥。
//! 扫描只列出候选，用户勾选后才查询额度并登记。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::discovery;
use crate::quota::{self, DetectedQuota, KeyQuota, QuotaProvider};

/// 会话内最多保留的条目，避免误粘贴大量密钥后长期占用内存。
const MAX_ENTRIES: usize = 20;
const CREDENTIALS_FILE: &str = "credentials.json";
const SOURCE_FILE: &str = "source.json";
const SNAPSHOT_FILE: &str = "snapshot.json";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 中转站条目变化后广播给所有窗口（主窗口与菜单栏面板）。
/// `revision` 用于丢弃乱序到达的旧快照，避免窗口回退到旧数据。
pub const RELAY_ACCOUNTS_EVENT: &str = "relay-accounts-changed";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayAccountsEvent {
    pub revision: u64,
    pub accounts: Vec<RelayAccount>,
}

/// 密钥来源；手动输入与本地配置自动读取共用同一份注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelayOrigin {
    Manual,
    CcSwitch,
    ClaudeCode,
    Codex,
}

impl RelayOrigin {
    /// 只有手动输入的条目会把密钥写入应用目录。扫描到的密钥属于源工具的配置，
    /// 复制一份到应用目录既无必要，也会扩大密钥的存在范围；这些条目只保存引用。
    fn is_persisted(self) -> bool {
        matches!(self, RelayOrigin::Manual)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayAccount {
    pub id: String,
    pub base_url: String,
    /// 扫描到的 provider 名称（cc-switch）；手动或当前配置文件条目为空。
    pub label: Option<String>,
    pub origin: RelayOrigin,
    pub provider: QuotaProvider,
    pub quota: Option<KeyQuota>,
    pub checked_at_epoch: Option<i64>,
    /// 最近一次刷新失败的原因；保留上一次成功的额度。
    pub last_error: Option<String>,
    /// 密钥是否已写入应用目录（仅手动填写的条目）。
    pub persisted: bool,
    /// 是否已在应用目录保存来源引用；下次启动会重新读取源配置。
    pub reference_saved: bool,
    /// 是否已配置 New API 访问令牌（只暴露存在性，不返回令牌本身）。
    pub has_account_token: bool,
    /// 旧版 New API 单令牌使用的 `New-Api-User` 用户 ID；不是密钥。
    pub new_api_user_id: Option<String>,
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

/// 扫描结果中的一条候选：只含展示信息，密钥保留在 Rust 内存里。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayCandidate {
    pub id: String,
    pub label: Option<String>,
    /// cc-switch 的 `app_type`（`claude` / `codex`）；当前配置文件没有这一项。
    pub app: Option<String>,
    pub origin: RelayOrigin,
    pub base_url: String,
    /// 同一地址与密钥已经在账户列表中，导入时无需重复查询。
    pub already_added: bool,
}

/// 写入一条中转站凭证所需的全部信息；参数较多，集中成一个结构便于阅读。
struct UpsertInput<'a> {
    origin: RelayOrigin,
    base_url: &'a str,
    api_key: &'a str,
    account_token: Option<&'a str>,
    new_api_user_id: Option<&'a str>,
    label: Option<&'a str>,
    source_ref: Option<StoredSourceRef>,
    detected: DetectedQuota,
}

/// 扫描导入的条目不保存密钥，只保存来源引用；下次启动按引用重新读取源配置。
/// 文件里可能含用户主动填写的 New API 访问令牌，因此同样按 0600 写入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSourceRef {
    origin: RelayOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app: Option<String>,
    /// cc-switch 的 provider 行 id，或 Codex `[model_providers.<id>]` 的表名。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_id: Option<String>,
    /// 导入时的原始地址；没有 provider id 时用它匹配当前配置。
    base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    provider: QuotaProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    new_api_user_id: Option<String>,
}

impl StoredSourceRef {
    fn matches(&self, candidate: &discovery::ConfigCandidate) -> bool {
        if self.origin != candidate.origin || self.app != candidate.app {
            return false;
        }
        match self.provider_id.as_deref() {
            Some(provider_id) => candidate.source_id.as_deref() == Some(provider_id),
            // Claude Code 的当前配置没有 provider id，按地址匹配；地址换了就不再
            // 把旧账户套到新配置上。
            None => candidate.base_url.trim() == self.base_url.trim(),
        }
    }
}

/// 启动时从磁盘恢复账户的两种形态：手动凭证，或只含来源的引用。
enum LoadedCredential {
    Inline(StoredRelayCredential),
    Reference(StoredSourceRef),
}

/// 扫描后暂存在内存中的候选，等待用户勾选导入。
#[derive(Clone)]
struct PendingCandidate {
    id: String,
    base_url: String,
    api_key: String,
    origin: RelayOrigin,
    app: Option<String>,
    source_id: Option<String>,
    label: Option<String>,
}

struct RelayEntry {
    id: String,
    base_url: String,
    label: Option<String>,
    origin: RelayOrigin,
    api_key: String,
    account_token: Option<String>,
    account_user_id: Option<String>,
    /// 扫描导入的条目的来源引用；手动条目为空。
    source_ref: Option<StoredSourceRef>,
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
            label: self.label.clone(),
            origin: self.origin,
            provider: self.provider,
            quota: self.quota.clone(),
            checked_at_epoch: self.checked_at_epoch,
            last_error: self.last_error.clone(),
            persisted: self.origin.is_persisted(),
            reference_saved: self.source_ref.is_some(),
            has_account_token: self.account_token.is_some(),
            new_api_user_id: self.account_user_id.clone(),
        }
    }

    /// 账户是否会跨重启保留：手动凭证或来源引用任一存在即可。
    fn is_retained(&self) -> bool {
        self.origin.is_persisted() || self.source_ref.is_some()
    }

    fn stored(&self) -> StoredRelayCredential {
        StoredRelayCredential {
            base_url: self.base_url.clone(),
            origin: self.origin,
            provider: self.provider,
            api_key: self.api_key.clone(),
            account_token: self.account_token.clone(),
            new_api_user_id: self.account_user_id.clone(),
        }
    }
}

/// 最近一次成功查询的非敏感快照。凭证与额度分文件保存：凭证写失败会阻止
/// 添加条目，快照写失败只影响重启后的首屏，不应该让一次成功的查询失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    quota: KeyQuota,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRelayCredential {
    base_url: String,
    origin: RelayOrigin,
    provider: QuotaProvider,
    api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    new_api_user_id: Option<String>,
}

pub struct RelayStore {
    entries: Mutex<Vec<RelayEntry>>,
    /// 本次运行最近一次扫描的候选，等待用户勾选；密钥不离开 Rust 内存。
    pending: Mutex<Vec<PendingCandidate>>,
    /// 条目修订号；每次变更都在持有条目锁时递增，供跨窗口事件排序。
    revision: AtomicU64,
    /// `None` 表示纯内存模式，供测试与未配置数据目录的场景使用。
    dir: Option<PathBuf>,
}

impl Default for RelayStore {
    fn default() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            pending: Mutex::new(Vec::new()),
            revision: AtomicU64::new(0),
            dir: None,
        }
    }
}

impl RelayStore {
    /// 从应用数据目录加载手动保存的中转站凭证。
    pub fn load(dir: PathBuf) -> Self {
        let store = Self {
            entries: Mutex::new(Vec::new()),
            pending: Mutex::new(Vec::new()),
            revision: AtomicU64::new(0),
            dir: Some(dir),
        };
        store.load_from_disk();
        store
    }

    fn lock(&self) -> MutexGuard<'_, Vec<RelayEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 记录一次条目变更。调用方需持有条目锁，保证修订号与列表快照一一对应。
    fn bump_revision(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    /// 条目列表与对应修订号；两者在同一把锁内读取，事件消费方可以按修订号丢弃旧快照。
    pub fn view_with_revision(&self) -> RelayAccountsEvent {
        let entries = self.lock();
        RelayAccountsEvent {
            revision: self.revision.load(Ordering::SeqCst),
            accounts: entries.iter().map(RelayEntry::view).collect(),
        }
    }

    /// 读取每个账户目录；损坏或权限不符的条目直接跳过并记录，不影响启动。
    fn load_from_disk(&self) {
        self.load_from_disk_with(discovery::discover_candidates);
    }

    /// 只有目录里存在来源引用时才读取外部配置，普通启动不会触发扫描。
    /// `discover` 单独传入，便于测试注入候选。
    fn load_from_disk_with(&self, mut discover: impl FnMut() -> Vec<discovery::ConfigCandidate>) {
        let Some(root) = &self.dir else {
            return;
        };
        let Ok(read_dir) = fs::read_dir(root) else {
            return;
        };

        let mut discovered: Option<Vec<discovery::ConfigCandidate>> = None;
        let mut entries = self.lock();
        for dir_entry in read_dir.flatten() {
            let path = dir_entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_valid_relay_id(id) || entries.iter().any(|entry| entry.id == id) {
                continue;
            }

            let loaded = match read_credentials(&path) {
                Ok(stored) => LoadedCredential::Inline(stored),
                Err(_) if path.join(SOURCE_FILE).is_file() => match read_source_ref(&path) {
                    Ok(reference) => LoadedCredential::Reference(reference),
                    Err(_) => {
                        eprintln!("跳过无法读取的中转站来源引用：{id}");
                        continue;
                    }
                },
                Err(_) => {
                    eprintln!("跳过无法读取的中转站凭证目录：{id}");
                    continue;
                }
            };

            let (
                base_url,
                api_key,
                origin,
                label,
                account_token,
                account_user_id,
                provider,
                source_ref,
            ) = match loaded {
                LoadedCredential::Inline(stored) => {
                    let Ok(base_url) = quota::normalize_base_url(&stored.base_url) else {
                        eprintln!("跳过服务地址无效的中转站凭证目录：{id}");
                        continue;
                    };
                    let Ok(api_key) = quota::validate_api_key(&stored.api_key) else {
                        eprintln!("跳过密钥无效的中转站凭证目录：{id}");
                        continue;
                    };
                    (
                        base_url,
                        api_key,
                        stored.origin,
                        None,
                        stored.account_token,
                        stored.new_api_user_id,
                        stored.provider,
                        None,
                    )
                }
                LoadedCredential::Reference(reference) => {
                    if discovered.is_none() {
                        discovered = Some(discover());
                    }
                    let Some(candidate) = discovered
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .find(|candidate| reference.matches(candidate))
                    else {
                        eprintln!("跳过来源已不可用的中转站账户：{id}");
                        continue;
                    };
                    let Ok(base_url) = quota::normalize_base_url(&candidate.base_url) else {
                        eprintln!("跳过来源地址无效的中转站账户：{id}");
                        continue;
                    };
                    let Ok(api_key) = quota::validate_api_key(&candidate.api_key) else {
                        eprintln!("跳过来源密钥无效的中转站账户：{id}");
                        continue;
                    };
                    let label = candidate.label.clone().or_else(|| reference.label.clone());
                    (
                        base_url,
                        api_key,
                        reference.origin,
                        label,
                        reference.account_token.clone(),
                        reference.new_api_user_id.clone(),
                        reference.provider,
                        Some(reference),
                    )
                }
            };

            let duplicate = entries
                .iter()
                .any(|entry| entry.base_url == base_url && entry.api_key == api_key);
            if duplicate {
                continue;
            }

            let mut quota = read_snapshot(&path).map(|snapshot| snapshot.quota);
            // 上次查询的账户错误不随快照恢复，等下一次刷新重新判断。
            if let Some(snapshot) = quota.as_mut() {
                snapshot.account_error = None;
            }
            let checked_at_epoch = quota.as_ref().map(|quota| quota.checked_at_epoch);
            entries.push(RelayEntry {
                id: id.to_string(),
                base_url,
                label,
                origin,
                api_key,
                account_token,
                account_user_id,
                source_ref,
                provider,
                quota,
                checked_at_epoch,
                last_error: None,
            });
        }
    }

    /// 自动识别上游并查询；同一服务地址 + 同一密钥复用原条目。
    /// `account_token` 与 `new_api_user_id` 只对 New API 生效。
    pub fn query(
        &self,
        base_url: &str,
        api_key: &str,
        account_token: Option<&str>,
        new_api_user_id: Option<&str>,
    ) -> Result<RelayAccount, String> {
        let detected = quota::detect_and_fetch(base_url, api_key, account_token, new_api_user_id)?;
        self.upsert(UpsertInput {
            origin: RelayOrigin::Manual,
            base_url,
            api_key,
            account_token,
            new_api_user_id,
            label: None,
            source_ref: None,
            detected,
        })
    }

    /// 扫描 cc-switch、Claude Code / Codex 的本地配置，只列出候选，不发起网络请求。
    /// 密钥留在内存中，等用户勾选后由 [`Self::import`] 使用。
    pub fn scan(&self) -> Vec<RelayCandidate> {
        self.scan_candidates(discovery::discover_candidates())
    }

    fn scan_candidates(&self, candidates: Vec<discovery::ConfigCandidate>) -> Vec<RelayCandidate> {
        let mut pending = Vec::with_capacity(candidates.len());
        let mut views = Vec::with_capacity(candidates.len());

        for (index, candidate) in candidates.into_iter().enumerate() {
            let id = format!("candidate-{}", index + 1);
            let normalized = quota::normalize_base_url(&candidate.base_url);
            let already_added = normalized
                .as_ref()
                .map(|base_url| self.contains(base_url, &candidate.api_key))
                .unwrap_or(false);
            views.push(RelayCandidate {
                id: id.clone(),
                label: candidate.label.clone(),
                app: candidate.app.clone(),
                origin: candidate.origin,
                base_url: normalized.unwrap_or(candidate.base_url.clone()),
                already_added,
            });
            pending.push(PendingCandidate {
                id,
                base_url: candidate.base_url,
                api_key: candidate.api_key,
                origin: candidate.origin,
                app: candidate.app,
                source_id: candidate.source_id,
                label: candidate.label,
            });
        }

        let mut pending_slot = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending_slot = pending;
        views
    }

    /// 导入用户勾选的候选：识别上游并查询额度。未识别的候选只报告来源与原因，
    /// 已保存的条目继续沿用上次成功快照。
    pub fn import(&self, ids: &[String]) -> DiscoveryReport {
        let selected: Vec<PendingCandidate> = {
            let pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut seen = HashSet::new();
            ids.iter()
                .filter(|id| seen.insert(id.as_str()))
                .filter_map(|id| {
                    pending
                        .iter()
                        .find(|candidate| &candidate.id == id)
                        .cloned()
                })
                .collect()
        };

        let mut skipped = Vec::new();
        for candidate in selected {
            let base_url = quota::normalize_base_url(&candidate.base_url)
                .unwrap_or_else(|_| candidate.base_url.clone());
            match quota::detect_and_fetch(&candidate.base_url, &candidate.api_key, None, None) {
                Ok(detected) => {
                    let source_ref = StoredSourceRef {
                        origin: candidate.origin,
                        app: candidate.app.clone(),
                        provider_id: candidate.source_id.clone(),
                        base_url: candidate.base_url.clone(),
                        label: candidate.label.clone(),
                        provider: detected.provider,
                        account_token: None,
                        new_api_user_id: None,
                    };
                    if let Err(reason) = self.upsert(UpsertInput {
                        origin: candidate.origin,
                        base_url: &candidate.base_url,
                        api_key: &candidate.api_key,
                        account_token: None,
                        new_api_user_id: None,
                        label: candidate.label.as_deref(),
                        source_ref: Some(source_ref),
                        detected,
                    }) {
                        skipped.push(SkippedConfig {
                            origin: candidate.origin,
                            base_url,
                            reason,
                        });
                    }
                }
                Err(reason) => skipped.push(SkippedConfig {
                    origin: candidate.origin,
                    base_url,
                    reason,
                }),
            }
        }

        DiscoveryReport {
            accounts: self.list(),
            skipped,
        }
    }

    fn upsert(&self, input: UpsertInput<'_>) -> Result<RelayAccount, String> {
        let UpsertInput {
            origin,
            base_url,
            api_key,
            account_token,
            new_api_user_id,
            label,
            mut source_ref,
            detected,
        } = input;
        let base_url = quota::normalize_base_url(base_url)?;
        let api_key = quota::validate_api_key(api_key)?;
        // 访问令牌与用户 ID 只对 New API 有意义，Sub2API 不保存这两项。
        let (account_token, account_user_id) = if detected.provider == QuotaProvider::NewApi {
            (
                account_token
                    .map(quota::validate_account_token)
                    .transpose()?,
                quota::validate_new_api_user_id(new_api_user_id)?,
            )
        } else {
            (None, None)
        };
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
            if let Some(label) = label {
                entry.label = Some(label.to_string());
            }
            if account_token.is_some() {
                entry.account_token = account_token;
            }
            if account_user_id.is_some() {
                entry.account_user_id = account_user_id;
            }
            if origin.is_persisted() {
                // 手动条目即使已被本地配置扫描到，也继续保留在应用目录里。
                entry.origin = RelayOrigin::Manual;
                entry.source_ref = None;
            } else if let Some(mut reference) = source_ref.take() {
                reference.provider = detected.provider;
                reference.account_token = entry.account_token.clone();
                reference.new_api_user_id = entry.account_user_id.clone();
                entry.source_ref = Some(reference);
            }
            if entry.origin.is_persisted() {
                self.persist_entry(entry)?;
            } else if entry.source_ref.is_some() {
                self.persist_source_ref(entry)?;
            }
            if entry.is_retained() {
                self.persist_snapshot(entry);
            }
            self.bump_revision();
            return Ok(entry.view());
        }

        if entries.len() >= MAX_ENTRIES {
            return Err(format!(
                "本次运行最多保存 {MAX_ENTRIES} 个中转站密钥，请先移除不用的条目。"
            ));
        }

        let id = next_id(&entries);
        let entry = RelayEntry {
            id,
            base_url,
            label: label.map(str::to_string),
            origin,
            api_key,
            account_token: account_token.clone(),
            account_user_id: account_user_id.clone(),
            source_ref: source_ref.map(|mut reference| {
                reference.provider = detected.provider;
                reference.account_token = account_token;
                reference.new_api_user_id = account_user_id;
                reference
            }),
            provider: detected.provider,
            quota: Some(detected.quota),
            checked_at_epoch,
            last_error: None,
        };
        if entry.origin.is_persisted() {
            self.persist_entry(&entry)?;
        } else if entry.source_ref.is_some() {
            self.persist_source_ref(&entry)?;
        }
        if entry.is_retained() {
            self.persist_snapshot(&entry);
        }
        let view = entry.view();
        entries.push(entry);
        self.bump_revision();
        Ok(view)
    }

    /// 使用已保存的密钥重新识别并查询；失败时保留旧额度并记录错误。
    pub fn refresh(&self, id: &str) -> Result<RelayAccount, String> {
        let (base_url, api_key, account_token, account_user_id) = {
            let entries = self.lock();
            let entry = entries
                .iter()
                .find(|entry| entry.id == id)
                .ok_or_else(|| "找不到这条中转站账户，请重新添加。".to_string())?;
            (
                entry.base_url.clone(),
                entry.api_key.clone(),
                entry.account_token.clone(),
                entry.account_user_id.clone(),
            )
        };

        let result = quota::detect_and_fetch(
            &base_url,
            &api_key,
            account_token.as_deref(),
            account_user_id.as_deref(),
        );

        let mut entries = self.lock();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| "找不到这条中转站账户，请重新添加。".to_string())?;
        match result {
            Ok(detected) => {
                let provider_changed = entry.provider != detected.provider;
                entry.provider = detected.provider;
                entry.checked_at_epoch = Some(detected.quota.checked_at_epoch);
                entry.quota = Some(detected.quota);
                entry.last_error = None;
                if provider_changed {
                    if let Some(reference) = entry.source_ref.as_mut() {
                        reference.provider = detected.provider;
                    }
                    if entry.source_ref.is_some() {
                        if let Err(error) = self.persist_source_ref(entry) {
                            eprintln!("更新中转站来源引用失败：{error}");
                        }
                    }
                }
                if entry.is_retained() {
                    self.persist_snapshot(entry);
                }
            }
            Err(error) => entry.last_error = Some(error),
        }
        self.bump_revision();
        Ok(entry.view())
    }

    /// 为已添加的 New API 账户补充或替换访问令牌，并立即重新查询。
    /// 访问令牌是用户主动填写的，手动条目写入 credentials.json，扫描导入的条目
    /// 写入 source.json；两者都不含 API Key。
    pub fn set_account_token(
        &self,
        id: &str,
        account_token: &str,
        new_api_user_id: Option<&str>,
    ) -> Result<RelayAccount, String> {
        let account_token = quota::validate_account_token(account_token)?;
        let new_api_user_id = quota::validate_new_api_user_id(new_api_user_id)?;
        {
            let mut entries = self.lock();
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| "找不到这条中转站账户，请重新添加。".to_string())?;
            if entry.provider != QuotaProvider::NewApi {
                return Err("访问令牌只用于 New API 账户。".to_string());
            }
            entry.account_token = Some(account_token);
            if new_api_user_id.is_some() {
                entry.account_user_id = new_api_user_id;
            }
            // 后续 refresh 还会递增一次；这里先记录令牌本身的变更。
            self.bump_revision();
            let token = entry.account_token.clone();
            let user_id = entry.account_user_id.clone();
            if let Some(reference) = entry.source_ref.as_mut() {
                reference.account_token = token;
                reference.new_api_user_id = user_id;
            }
            if entry.source_ref.is_some() {
                self.persist_source_ref(entry)?;
            } else if entry.origin.is_persisted() {
                self.persist_entry(entry)?;
            }
        }
        self.refresh(id)
    }

    pub fn list(&self) -> Vec<RelayAccount> {
        self.lock().iter().map(RelayEntry::view).collect()
    }

    /// 服务地址应已规范化；密钥按校验后的形式比较。
    fn contains(&self, base_url: &str, api_key: &str) -> bool {
        let api_key = api_key.trim();
        self.lock()
            .iter()
            .any(|entry| entry.base_url == base_url && entry.api_key == api_key)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut entries = self.lock();
        let Some(index) = entries.iter().position(|entry| entry.id == id) else {
            return Err("找不到这条中转站账户。".to_string());
        };
        // 先删目录再删内存条目：磁盘删除失败时条目仍然可见，用户可以重试。
        if let Some(dir) = self.entry_dir(id) {
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|err| format!("删除中转站凭证目录失败：{err}"))?;
            }
        }
        entries.remove(index);
        self.bump_revision();
        Ok(())
    }

    fn entry_dir(&self, id: &str) -> Option<PathBuf> {
        if !is_valid_relay_id(id) {
            return None;
        }
        self.dir.as_ref().map(|root| root.join(id))
    }

    fn persist_entry(&self, entry: &RelayEntry) -> Result<(), String> {
        let Some(dir) = self.entry_dir(&entry.id) else {
            return Ok(());
        };
        write_credentials(&dir, &entry.stored())?;
        // 同一条目改成手动凭证后，不应再残留旧的来源引用。
        let _ = fs::remove_file(dir.join(SOURCE_FILE));
        Ok(())
    }

    /// 保存来源引用（不含密钥）。下次启动按引用重新读取源配置。
    fn persist_source_ref(&self, entry: &RelayEntry) -> Result<(), String> {
        let (Some(dir), Some(reference)) = (self.entry_dir(&entry.id), entry.source_ref.as_ref())
        else {
            return Ok(());
        };
        write_json_private(&dir, SOURCE_FILE, reference, "中转站来源引用")?;
        // 来源引用与手动凭证互斥，避免下次启动优先读到旧密钥。
        let _ = fs::remove_file(dir.join(CREDENTIALS_FILE));
        Ok(())
    }

    /// 快照写失败不影响本次查询结果，只在下次启动时缺少首屏数据。
    fn persist_snapshot(&self, entry: &RelayEntry) {
        let (Some(dir), Some(quota)) = (self.entry_dir(&entry.id), entry.quota.as_ref()) else {
            return;
        };
        if let Err(error) = write_snapshot(&dir, quota) {
            eprintln!("保存中转站额度快照失败：{error}");
        }
    }
}

/// 标识会参与目录名拼接，只允许安全字符，避免路径逃逸。
fn is_valid_relay_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn next_id(entries: &[RelayEntry]) -> String {
    loop {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let id = format!("relay-{millis}-{sequence}");
        if !entries.iter().any(|entry| entry.id == id) {
            return id;
        }
    }
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("创建中转站凭证目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置中转站凭证目录权限失败：{err}"))?;
    }
    Ok(())
}

/// 先写临时文件再原子替换，避免进程中断留下半截 JSON。
fn write_json_private(
    dir: &Path,
    file_name: &str,
    payload: &impl Serialize,
    label: &str,
) -> Result<(), String> {
    ensure_private_dir(dir)?;
    let path = dir.join(file_name);
    let tmp = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(payload).map_err(|err| format!("序列化{label}失败：{err}"))?;
    fs::write(&tmp, bytes).map_err(|err| format!("写入{label}失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置{label}权限失败：{err}"))?;
    }
    fs::rename(&tmp, &path).map_err(|err| format!("保存{label}失败：{err}"))?;
    Ok(())
}

fn write_credentials(dir: &Path, payload: &StoredRelayCredential) -> Result<(), String> {
    write_json_private(dir, CREDENTIALS_FILE, payload, "中转站凭证")
}

fn write_snapshot(dir: &Path, quota: &KeyQuota) -> Result<(), String> {
    write_json_private(
        dir,
        SNAPSHOT_FILE,
        &StoredSnapshot {
            quota: quota.clone(),
        },
        "中转站额度快照",
    )
}

fn read_credentials(dir: &Path) -> Result<StoredRelayCredential, String> {
    let raw = fs::read_to_string(dir.join(CREDENTIALS_FILE))
        .map_err(|_| "未找到中转站凭证文件。".to_string())?;
    serde_json::from_str(&raw).map_err(|_| "中转站凭证文件无法读取。".to_string())
}

fn read_source_ref(dir: &Path) -> Result<StoredSourceRef, String> {
    let raw = fs::read_to_string(dir.join(SOURCE_FILE))
        .map_err(|_| "未找到中转站来源引用。".to_string())?;
    serde_json::from_str(&raw).map_err(|_| "中转站来源引用无法读取。".to_string())
}

fn read_snapshot(dir: &Path) -> Option<StoredSnapshot> {
    let raw = fs::read_to_string(dir.join(SNAPSHOT_FILE)).ok()?;
    serde_json::from_str(&raw).ok()
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

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!(
            "token-refill-relay-{label}-{}-{nanos}",
            std::process::id()
        ))
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

        let first = store.query(&base, "sk-test", None, None).unwrap();
        assert_eq!(first.provider, QuotaProvider::Sub2Api);
        assert_eq!(first.quota.as_ref().unwrap().remaining_usd, Some(4.0));

        let second = store.query(&base, "sk-test", None, None).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn revision_advances_with_every_entry_change() {
        // 两个窗口按修订号丢弃乱序事件，因此每次变更都必须递增。
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
            ("/api/usage/token/", 401, "{}"),
            ("/v1/usage", 401, "{}"),
        ]);
        let store = RelayStore::default();
        let empty = store.view_with_revision();
        assert_eq!(empty.revision, 0);
        assert!(empty.accounts.is_empty());

        let account = store.query(&base, "sk-test", None, None).unwrap();
        let added = store.view_with_revision();
        assert_eq!(added.revision, 1);
        assert_eq!(added.accounts.len(), 1);
        assert_eq!(added.accounts[0].id, account.id);

        store.refresh(&account.id).unwrap();
        let refreshed = store.view_with_revision();
        assert_eq!(refreshed.revision, 2);
        assert!(refreshed.accounts[0].last_error.is_some());

        store.remove(&account.id).unwrap();
        let removed = store.view_with_revision();
        assert_eq!(removed.revision, 3);
        assert!(removed.accounts.is_empty());
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
        let account = store.query(&base, "sk-test", None, None).unwrap();

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
        let account = store.query(&base, "sk-test", None, None).unwrap();

        store.remove(&account.id).unwrap();
        assert!(store.list().is_empty());
        assert!(store.refresh(&account.id).is_err());
    }

    #[test]
    fn manual_credentials_survive_a_reload_and_are_removed_with_the_entry() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("persist");

        let account = {
            let store = RelayStore::load(dir.clone());
            store.query(&base, "sk-test", None, None).unwrap()
        };
        assert!(dir.join(&account.id).join(CREDENTIALS_FILE).is_file());

        let store = RelayStore::load(dir.clone());
        let loaded = store.list();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, account.id);
        assert_eq!(loaded[0].origin, RelayOrigin::Manual);
        assert!(loaded[0].persisted);
        let quota = loaded[0].quota.as_ref().expect("恢复上次成功快照");
        assert_eq!(quota.remaining_usd, Some(4.0));
        assert_eq!(loaded[0].checked_at_epoch, Some(quota.checked_at_epoch));

        store.remove(&account.id).unwrap();
        assert!(!dir.join(&account.id).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn access_token_is_persisted_with_the_credential_and_snapshot() {
        let usage = r#"{"code":true,"data":{"name":"中转","total_granted":1000000,
            "total_used":0,"total_available":1000000,"unlimited_quota":false}}"#;
        let status = r#"{"success":true,"data":{"quota_per_unit":500000}}"#;
        let self_payload = r#"{"success":true,"data":{"quota":600000,"used_quota":400000}}"#;
        let subscriptions = r#"{"success":true,"data":{"subscriptions":[]}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 200, usage),
            ("/api/status", 200, status),
            ("/api/user/self", 200, self_payload),
            ("/api/subscription/self", 200, subscriptions),
        ]);
        let dir = temp_dir("access-token");

        let account = {
            let store = RelayStore::load(dir.clone());
            store
                .query(&base, "sk-test", Some("pat-test-token"), Some("20001"))
                .unwrap()
        };
        let quota = account.quota.as_ref().unwrap();
        assert_eq!(
            quota.account.as_ref().unwrap().source,
            crate::quota::AccountSource::AccessToken
        );

        let stored = read_credentials(&dir.join(&account.id)).unwrap();
        assert_eq!(stored.account_token.as_deref(), Some("pat-test-token"));
        assert_eq!(stored.new_api_user_id.as_deref(), Some("20001"));
        let snapshot = read_snapshot(&dir.join(&account.id)).unwrap();
        assert_eq!(
            snapshot.quota.account.unwrap().source,
            crate::quota::AccountSource::AccessToken
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovered_entries_stay_in_memory_and_are_not_copied_to_the_app_dir() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("discovered");
        let store = RelayStore::load(dir.clone());
        let detected = quota::detect_and_fetch(&base, "sk-test", None, None).unwrap();
        let account = store
            .upsert(UpsertInput {
                origin: RelayOrigin::Codex,
                base_url: &base,
                api_key: "sk-test",
                account_token: None,
                new_api_user_id: None,
                label: None,
                source_ref: None,
                detected,
            })
            .unwrap();

        assert!(!account.persisted);
        assert!(!account.reference_saved);
        assert!(!dir.join(&account.id).exists());
        assert_eq!(RelayStore::load(dir.clone()).list().len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    fn config_candidate(
        origin: RelayOrigin,
        label: Option<&str>,
        base_url: &str,
        api_key: &str,
    ) -> discovery::ConfigCandidate {
        discovery::ConfigCandidate {
            origin,
            app: matches!(origin, RelayOrigin::CcSwitch).then(|| "claude".to_string()),
            source_id: matches!(origin, RelayOrigin::CcSwitch)
                .then(|| format!("cc-switch-{base_url}")),
            label: label.map(str::to_string),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
        }
    }

    #[test]
    fn scan_lists_cc_switch_candidates_without_querying() {
        let store = RelayStore::default();
        let candidates = store.scan_candidates(vec![
            config_candidate(
                RelayOrigin::CcSwitch,
                Some("千问AI平台"),
                "https://relay.example.com/v1",
                "sk-one",
            ),
            config_candidate(
                RelayOrigin::Codex,
                None,
                "https://second.example.com",
                "sk-two",
            ),
        ]);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, "candidate-1");
        assert_eq!(candidates[0].label.as_deref(), Some("千问AI平台"));
        assert_eq!(candidates[0].app.as_deref(), Some("claude"));
        assert_eq!(candidates[0].origin, RelayOrigin::CcSwitch);
        assert_eq!(candidates[0].base_url, "https://relay.example.com");
        assert!(!candidates[0].already_added);
        assert_eq!(candidates[1].id, "candidate-2");
        // 扫描只列出候选，不会创建账户。
        assert!(store.list().is_empty());
    }

    #[test]
    fn scan_marks_credentials_already_in_the_list() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let store = RelayStore::default();
        let account = store.query(&base, "sk-test", None, None).unwrap();

        let candidates = store.scan_candidates(vec![config_candidate(
            RelayOrigin::CcSwitch,
            Some("已有账户"),
            &base,
            "sk-test",
        )]);

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].already_added);
        assert_eq!(candidates[0].base_url, account.base_url);
    }

    #[test]
    fn import_queries_only_the_selected_candidates() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        // 只为第一个候选准备响应；如果导入时误查第二个，连接失败会落到 skipped。
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let store = RelayStore::default();
        let candidates = store.scan_candidates(vec![
            config_candidate(RelayOrigin::CcSwitch, Some("选中的账户"), &base, "sk-one"),
            config_candidate(RelayOrigin::ClaudeCode, None, &base, "sk-two"),
        ]);

        let report = store.import(&[candidates[0].id.clone()]);

        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(report.accounts.len(), 1);
        assert_eq!(report.accounts[0].label.as_deref(), Some("选中的账户"));
        assert_eq!(report.accounts[0].origin, RelayOrigin::CcSwitch);
        assert!(!report.accounts[0].persisted);
    }

    #[test]
    fn import_ignores_unknown_and_duplicate_ids() {
        let store = RelayStore::default();
        store.scan_candidates(vec![config_candidate(
            RelayOrigin::Codex,
            None,
            "http://127.0.0.1:1",
            "sk-one",
        )]);

        let report = store.import(&[
            "missing".to_string(),
            "candidate-1".to_string(),
            "candidate-1".to_string(),
        ]);

        // 只读内存里的候选，不会因为未知 id 或重复 id 报错；候选指向不可达
        // 地址时应落在 skipped 里，说明仍然只处理了一次。
        assert_eq!(report.skipped.len(), 1);
        assert!(report.accounts.is_empty());
    }

    fn store_loaded_with(dir: PathBuf, candidates: Vec<discovery::ConfigCandidate>) -> RelayStore {
        let store = RelayStore {
            entries: Mutex::new(Vec::new()),
            pending: Mutex::new(Vec::new()),
            revision: AtomicU64::new(0),
            dir: Some(dir),
        };
        store.load_from_disk_with(|| candidates.clone());
        store
    }

    #[test]
    fn imported_accounts_save_a_source_reference_not_the_key() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("source-ref");
        let store = RelayStore::load(dir.clone());
        let candidate =
            config_candidate(RelayOrigin::CcSwitch, Some("来源账户"), &base, "sk-source");
        let views = store.scan_candidates(vec![candidate]);
        let report = store.import(&[views[0].id.clone()]);

        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let account = &report.accounts[0];
        assert!(account.reference_saved);
        assert!(!account.persisted);
        let profile = dir.join(&account.id);
        let raw = fs::read_to_string(profile.join(SOURCE_FILE)).unwrap();
        assert!(!raw.contains("sk-source"));
        assert!(!profile.join(CREDENTIALS_FILE).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_references_restore_the_account_on_reload() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("source-reload");
        let store = RelayStore::load(dir.clone());
        let candidate =
            config_candidate(RelayOrigin::CcSwitch, Some("来源账户"), &base, "sk-source");
        let views = store.scan_candidates(vec![candidate.clone()]);
        let report = store.import(&[views[0].id.clone()]);
        let account = report.accounts[0].clone();

        let reloaded = store_loaded_with(dir.clone(), vec![candidate]);
        let list = reloaded.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, account.id);
        assert_eq!(list[0].origin, RelayOrigin::CcSwitch);
        assert_eq!(list[0].label.as_deref(), Some("来源账户"));
        assert!(list[0].reference_saved);
        assert!(!list[0].persisted);
        assert_eq!(
            list[0].quota.as_ref().unwrap().remaining_usd,
            Some(4.0),
            "额度快照应随来源账户恢复"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_reference_without_its_source_is_skipped_on_reload() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("source-missing");
        let store = RelayStore::load(dir.clone());
        let candidate =
            config_candidate(RelayOrigin::CcSwitch, Some("来源账户"), &base, "sk-source");
        let views = store.scan_candidates(vec![candidate]);
        store.import(&[views[0].id.clone()]);

        let reloaded = store_loaded_with(dir.clone(), Vec::new());
        assert!(reloaded.list().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manually_adding_a_sourced_account_switches_to_inline_credentials() {
        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("source-promote");
        let store = RelayStore::load(dir.clone());
        let candidate =
            config_candidate(RelayOrigin::CcSwitch, Some("来源账户"), &base, "sk-source");
        let views = store.scan_candidates(vec![candidate]);
        let report = store.import(&[views[0].id.clone()]);
        let id = report.accounts[0].id.clone();

        let updated = store.query(&base, "sk-source", None, None).unwrap();
        assert_eq!(updated.id, id);
        assert!(updated.persisted);
        assert!(!updated.reference_saved);
        assert!(dir.join(&id).join(CREDENTIALS_FILE).is_file());
        assert!(!dir.join(&id).join(SOURCE_FILE).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_credential_files_are_skipped_without_failing_startup() {
        let dir = temp_dir("corrupt");
        let profile = dir.join("relay-broken");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join(CREDENTIALS_FILE), "{not json").unwrap();

        let store = RelayStore::load(dir.clone());
        assert!(store.list().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_credentials_use_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let body = r#"{"mode":"quota_limited","isValid":true,
            "quota":{"limit":5,"used":1,"remaining":4}}"#;
        let base = serve(vec![
            ("/api/usage/token/", 404, "{}"),
            ("/v1/usage", 200, body),
        ]);
        let dir = temp_dir("mode");
        let store = RelayStore::load(dir.clone());
        let account = store.query(&base, "sk-test", None, None).unwrap();

        let profile = dir.join(&account.id);
        let file_mode = fs::metadata(profile.join(CREDENTIALS_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let dir_mode = fs::metadata(&profile).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);

        let _ = fs::remove_dir_all(&dir);
    }
}
