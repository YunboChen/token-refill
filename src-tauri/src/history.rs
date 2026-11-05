//! 本机额度走势：按账号、套餐、分组与窗口记录成功查询的脱敏数值。
//!
//! 历史单独存放在应用数据目录的 `quota-history.json`，不进入 `state.json`：
//! 后者每次变化都会整体广播给主窗口与菜单栏面板，历史只在打开统计页时按需查询。
//! 文件里只有时间戳、剩余比例/金额、已知重置时间与窗口单位，不含账户名、
//! 邮箱、服务地址或任何凭证。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::state::QuotaSnapshot;

const SCHEMA_VERSION: u32 = 1;
/// 只保留最近 7 天；更早的样本在写入与启动时裁剪。
pub const RETENTION_SECS: i64 = 7 * 24 * 60 * 60;
/// 最近 24 小时保留每次成功查询，更早的样本按 15 分钟一个点降采样，
/// 避免 5 分钟刷新把单个窗口撑到两千个点。
const RAW_SECS: i64 = 24 * 60 * 60;
const COARSE_BUCKET_SECS: i64 = 15 * 60;
/// 单个窗口的样本上限；7 天降采样后通常只有几百个点，这里只做防御。
const MAX_SAMPLES: usize = 2_000;

/// 一次成功查询中的一条窗口样本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySample {
    /// 查询时间；同一时间只保留一条，重复命令不会产生重复点。
    pub at: i64,
    /// 剩余比例（百分数）。窗口按比例计量时使用。
    #[serde(default)]
    pub remaining_percent: Option<i64>,
    /// 剩余绝对量。按金额/次数计量时使用。
    #[serde(default)]
    pub remaining_amount: Option<f64>,
    /// 平台给出的重置时间；变化即为新周期，前端据此分段。
    #[serde(default)]
    pub resets_at_epoch: Option<i64>,
}

/// 一个窗口的历史序列。因为周期长度可能随套餐变化，这里也保存最近一次
/// 查询到的单位与时长，供前端选择坐标轴与标签。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowHistory {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub duration_mins: Option<i64>,
    #[serde(default)]
    pub samples: Vec<HistorySample>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductHistory {
    #[serde(default)]
    pub windows: HashMap<String, WindowHistory>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHistory {
    #[serde(default)]
    pub products: HashMap<String, ProductHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryFile {
    schema_version: u32,
    #[serde(default)]
    accounts: HashMap<String, AccountHistory>,
}

impl Default for HistoryFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            accounts: HashMap::new(),
        }
    }
}

/// 写入历史的一条窗口样本；由 [`snapshot_windows`] 从状态快照生成。
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSample {
    /// 分组与窗口组成的稳定键，例如 `default/primary`。
    pub key: String,
    pub unit: String,
    pub duration_mins: Option<i64>,
    pub remaining_percent: Option<i64>,
    pub remaining_amount: Option<f64>,
    pub resets_at_epoch: Option<i64>,
}

/// 统计页使用的窗口视图；只含展示所需字段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowHistoryView {
    pub key: String,
    pub unit: String,
    pub duration_mins: Option<i64>,
    pub samples: Vec<HistorySample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductHistoryView {
    pub product_id: String,
    pub windows: Vec<WindowHistoryView>,
}

/// 额度走势的落盘存储；写入是尽力而为，失败只记录到 stderr，
/// 不影响额度状态本身的保存与展示。
pub struct QuotaHistoryStore {
    path: PathBuf,
    inner: Mutex<HistoryFile>,
    /// 文件由更高版本写入：本次运行只读，不覆盖对方的数据。
    read_only: bool,
}

impl QuotaHistoryStore {
    pub fn load(path: PathBuf, now: i64) -> Self {
        let (mut file, read_only) = load_history(&path, now);
        prune_file(&mut file, now);
        Self {
            path,
            inner: Mutex::new(file),
            read_only,
        }
    }

    /// 记录一次成功查询。`checked_at` 相同的重复调用只更新样本，不追加。
    /// 返回是否有变化（没有变化时不写盘）。
    pub fn record(
        &self,
        account_id: &str,
        product_id: &str,
        checked_at: i64,
        windows: Vec<WindowSample>,
        now: i64,
    ) -> bool {
        let mut guard = lock(&self.inner);
        let mut next = guard.clone();
        let mut changed = false;
        for window in windows {
            changed |= record_window(&mut next, account_id, product_id, checked_at, window, now);
        }
        if !changed {
            return false;
        }
        prune_file(&mut next, now);
        if !self.read_only {
            if let Err(err) = write_history(&self.path, &next) {
                eprintln!("保存额度走势失败：{err}");
            }
        }
        *guard = next;
        true
    }

    /// 移除账户时清理对应历史；没有记录时不写盘。
    pub fn clear_account(&self, account_id: &str) -> bool {
        let mut guard = lock(&self.inner);
        if guard.accounts.remove(account_id).is_none() {
            return false;
        }
        if !self.read_only {
            if let Err(err) = write_history(&self.path, &guard) {
                eprintln!("保存额度走势失败：{err}");
            }
        }
        true
    }

    /// 统计页读取单个账户的全部套餐与窗口；样本按时间升序返回。
    pub fn account_view(&self, account_id: &str) -> Vec<ProductHistoryView> {
        let guard = lock(&self.inner);
        let Some(account) = guard.accounts.get(account_id) else {
            return Vec::new();
        };
        let mut products: Vec<ProductHistoryView> = account
            .products
            .iter()
            .filter(|(_, product)| !product.windows.is_empty())
            .map(|(product_id, product)| {
                let mut windows: Vec<WindowHistoryView> = product
                    .windows
                    .iter()
                    .filter(|(_, series)| !series.samples.is_empty())
                    .map(|(key, series)| WindowHistoryView {
                        key: key.clone(),
                        unit: series.unit.clone(),
                        duration_mins: series.duration_mins,
                        samples: series.samples.clone(),
                    })
                    .collect();
                windows.sort_by(|left, right| left.key.cmp(&right.key));
                ProductHistoryView {
                    product_id: product_id.clone(),
                    windows,
                }
            })
            .collect();
        products.sort_by(|left, right| left.product_id.cmp(&right.product_id));
        products
    }
}

/// 从一次成功查询的额度快照提取可记录的窗口。
///
/// 分组键优先使用平台给出的 `limit_id`，其次 `limit_name`；都没有时按组序号
/// 生成，保证同一账号同一行情景下键稳定。比例与绝对量都缺失的窗口跳过，
/// 不用 0 或推算值代替。
pub fn snapshot_windows(snapshot: &QuotaSnapshot) -> Vec<WindowSample> {
    let mut windows = Vec::new();
    for (index, group) in snapshot.groups.iter().enumerate() {
        let group_key = group
            .limit_id
            .clone()
            .or_else(|| group.limit_name.clone())
            .unwrap_or_else(|| format!("group-{index}"));
        for window in &group.windows {
            if window.remaining_percent.is_none() && window.remaining_amount.is_none() {
                continue;
            }
            windows.push(WindowSample {
                key: format!("{group_key}/{}", window.window_id),
                unit: window.unit.clone(),
                duration_mins: window.window_duration_mins,
                remaining_percent: window.remaining_percent,
                remaining_amount: window.remaining_amount,
                resets_at_epoch: window.resets_at_epoch,
            });
        }
    }
    windows
}

fn record_window(
    file: &mut HistoryFile,
    account_id: &str,
    product_id: &str,
    checked_at: i64,
    window: WindowSample,
    now: i64,
) -> bool {
    let account = file.accounts.entry(account_id.to_string()).or_default();
    let product = account.products.entry(product_id.to_string()).or_default();
    let series = product
        .windows
        .entry(window.key)
        .or_insert_with(|| WindowHistory {
            unit: window.unit.clone(),
            duration_mins: window.duration_mins,
            samples: Vec::new(),
        });

    let mut changed = false;
    if series.unit != window.unit {
        series.unit = window.unit.clone();
        changed = true;
    }
    if series.duration_mins != window.duration_mins {
        series.duration_mins = window.duration_mins;
        changed = true;
    }

    let sample = HistorySample {
        at: checked_at,
        remaining_percent: window.remaining_percent,
        remaining_amount: window.remaining_amount,
        resets_at_epoch: window.resets_at_epoch,
    };
    match series.samples.last_mut() {
        // 同一查询时间重复到达：覆盖而不是追加。
        Some(last) if last.at == checked_at => {
            if *last != sample {
                *last = sample;
                changed = true;
            }
        }
        // 乱序的旧样本直接忽略，避免序列倒退。
        Some(last) if last.at > checked_at => {}
        _ => {
            series.samples.push(sample);
            changed = true;
        }
    }

    let before = series.samples.len();
    prune_samples(&mut series.samples, now);
    if series.samples.len() != before {
        changed = true;
    }
    changed
}

/// 裁剪过期样本并降采样；公开给测试与启动加载共用。
pub fn prune_samples(samples: &mut Vec<HistorySample>, now: i64) {
    let cutoff = now.saturating_sub(RETENTION_SECS);
    samples.retain(|sample| sample.at >= cutoff);
    compact_samples(samples, now);
    if samples.len() > MAX_SAMPLES {
        samples.drain(..samples.len() - MAX_SAMPLES);
    }
}

/// 最近 24 小时保留原始采样；更早的按 15 分钟桶保留桶内最后一个点。
fn compact_samples(samples: &mut Vec<HistorySample>, now: i64) {
    let raw_cutoff = now.saturating_sub(RAW_SECS);
    let mut compacted = Vec::with_capacity(samples.len());
    let mut last_bucket: Option<i64> = None;
    for sample in samples.drain(..) {
        if sample.at >= raw_cutoff {
            compacted.push(sample);
            last_bucket = None;
            continue;
        }
        let bucket = sample.at.div_euclid(COARSE_BUCKET_SECS);
        if last_bucket == Some(bucket) {
            if let Some(last) = compacted.last_mut() {
                *last = sample;
            }
        } else {
            compacted.push(sample);
            last_bucket = Some(bucket);
        }
    }
    *samples = compacted;
}

fn prune_file(file: &mut HistoryFile, now: i64) {
    for account in file.accounts.values_mut() {
        for product in account.products.values_mut() {
            product.windows.retain(|_, series| {
                prune_samples(&mut series.samples, now);
                !series.samples.is_empty()
            });
        }
        account
            .products
            .retain(|_, product| !product.windows.is_empty());
    }
    file.accounts
        .retain(|_, account| !account.products.is_empty());
}

fn load_history(path: &Path, now: i64) -> (HistoryFile, bool) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        // 文件不存在或不可读：从空历史开始，后续成功写入前不覆盖原文件。
        Err(_) => return (HistoryFile::default(), false),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => {
            backup_unreadable(path, now);
            return (HistoryFile::default(), false);
        }
    };
    let file_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok());
    if file_version.is_some_and(|version| version > SCHEMA_VERSION) {
        // 旧代码忽略不认识的字段；解析不了时保持空历史，但不改写对方文件。
        let file = serde_json::from_value::<HistoryFile>(value).unwrap_or_default();
        return (file, true);
    }
    match serde_json::from_value::<HistoryFile>(value) {
        Ok(file) if file.schema_version == SCHEMA_VERSION => (file, false),
        _ => {
            backup_unreadable(path, now);
            (HistoryFile::default(), false)
        }
    }
}

fn write_history(path: &Path, file: &HistoryFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建应用数据目录失败：{err}"))?;
    }
    let bytes = serde_json::to_vec(file).map_err(|err| format!("序列化额度走势失败：{err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|err| format!("写入额度走势失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置额度走势权限失败：{err}"))?;
    }
    fs::rename(&tmp, path).map_err(|err| format!("替换额度走势失败：{err}"))
}

fn backup_unreadable(path: &Path, now: i64) {
    let backup = path.with_extension(format!("json.corrupt-{now}"));
    let _ = fs::rename(path, backup);
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::state::{QuotaGroup, QuotaWindow};

    fn sample(at: i64, percent: i64, resets_at: Option<i64>) -> HistorySample {
        HistorySample {
            at,
            remaining_percent: Some(percent),
            remaining_amount: None,
            resets_at_epoch: resets_at,
        }
    }

    fn temp_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!(
            "token-refill-history-{label}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    fn window_sample(key: &str, percent: i64) -> WindowSample {
        WindowSample {
            key: key.to_string(),
            unit: "percent".to_string(),
            duration_mins: Some(300),
            remaining_percent: Some(percent),
            remaining_amount: None,
            resets_at_epoch: Some(1_800_000_000),
        }
    }

    fn quota_window(window_id: &str, percent: Option<i64>, amount: Option<f64>) -> QuotaWindow {
        QuotaWindow {
            window_id: window_id.to_string(),
            source_window_id: window_id.to_string(),
            window_name: None,
            unit: if percent.is_some() {
                "percent".to_string()
            } else {
                "calls".to_string()
            },
            used_percent: percent.map(|value| 100 - value),
            remaining_percent: percent,
            used_amount: None,
            total_amount: None,
            remaining_amount: amount,
            window_duration_mins: Some(300),
            resets_at_epoch: Some(1_800_000_000),
            expires_at_epoch: None,
        }
    }

    #[test]
    fn snapshot_windows_keeps_only_comparable_values() {
        let snapshot = QuotaSnapshot {
            checked_at_epoch: 1_800_000_000,
            groups: vec![
                QuotaGroup {
                    limit_id: Some("default".to_string()),
                    limit_name: None,
                    plan_type: None,
                    windows: vec![
                        quota_window("primary", Some(42), None),
                        quota_window("unknown", None, None),
                        quota_window("balance", None, Some(3.5)),
                    ],
                },
                QuotaGroup {
                    limit_id: None,
                    limit_name: None,
                    plan_type: None,
                    windows: vec![quota_window("secondary", Some(70), None)],
                },
            ],
            capability_gaps: Vec::new(),
        };

        let windows = snapshot_windows(&snapshot);
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].key, "default/primary");
        assert_eq!(windows[0].remaining_percent, Some(42));
        assert_eq!(windows[1].key, "default/balance");
        assert_eq!(windows[1].remaining_amount, Some(3.5));
        // 没有分组标识时按序号生成稳定键，不依赖名称。
        assert_eq!(windows[2].key, "group-1/secondary");
    }

    #[test]
    fn same_timestamp_replaces_instead_of_appending() {
        let path = temp_path("dedupe");
        let store = QuotaHistoryStore::load(path.clone(), 1_800_000_000);
        assert!(store.record(
            "account-1",
            "codex-plus",
            1_800_000_000,
            vec![window_sample("default/primary", 40)],
            1_800_000_000,
        ));
        // 同一查询时间重复到达：覆盖而不是追加。
        assert!(store.record(
            "account-1",
            "codex-plus",
            1_800_000_000,
            vec![window_sample("default/primary", 35)],
            1_800_000_000,
        ));
        let products = store.account_view("account-1");
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].windows.len(), 1);
        assert_eq!(products[0].windows[0].samples.len(), 1);
        assert_eq!(
            products[0].windows[0].samples[0].remaining_percent,
            Some(35)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prunes_samples_older_than_seven_days() {
        let now = 1_800_000_000;
        let mut samples = vec![
            sample(now - RETENTION_SECS - 60, 10, None),
            sample(now - 24 * 60 * 60, 20, None),
            sample(now, 30, None),
        ];
        prune_samples(&mut samples, now);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].at, now - 24 * 60 * 60);
        assert_eq!(samples[1].at, now);
    }

    #[test]
    fn compacts_old_samples_to_quarter_hour_buckets() {
        let now = 1_800_000_000;
        let cutoff = now - RAW_SECS;
        // 桶起点对齐到 900 秒，保证两条样本落在同一个桶里。
        let bucket_start = (cutoff - 3_600).div_euclid(COARSE_BUCKET_SECS) * COARSE_BUCKET_SECS;
        let mut samples = vec![
            sample(bucket_start, 10, None),
            sample(bucket_start + 60, 11, None),
            sample(bucket_start + COARSE_BUCKET_SECS, 12, None),
            sample(now - 60, 13, None),
        ];
        prune_samples(&mut samples, now);
        // 旧样本每个 15 分钟桶只留最后一个；24 小时内的原始样本保留。
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].at, bucket_start + 60);
        assert_eq!(samples[1].at, bucket_start + COARSE_BUCKET_SECS);
        assert_eq!(samples[2].at, now - 60);
    }

    #[test]
    fn clear_account_removes_history() {
        let path = temp_path("clear");
        let store = QuotaHistoryStore::load(path.clone(), 1_800_000_000);
        store.record(
            "account-1",
            "codex-plus",
            1_800_000_000,
            vec![window_sample("default/primary", 40)],
            1_800_000_000,
        );
        assert!(store.clear_account("account-1"));
        assert!(store.account_view("account-1").is_empty());
        // 再次清理没有变化，不写盘。
        assert!(!store.clear_account("account-1"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn higher_schema_file_is_read_only() {
        let path = temp_path("newer");
        fs::write(&path, r#"{"schemaVersion":99,"accounts":{}}"#).unwrap();
        let store = QuotaHistoryStore::load(path.clone(), 1_800_000_000);
        assert!(store.record(
            "account-1",
            "codex-plus",
            1_800_000_000,
            vec![window_sample("default/primary", 40)],
            1_800_000_000,
        ));
        // 内存里有样本，但更高版本的文件保持原样。
        assert_eq!(store.account_view("account-1").len(), 1);
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw, r#"{"schemaVersion":99,"accounts":{}}"#);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_restarts_empty() {
        let path = temp_path("corrupt");
        fs::write(&path, "not json").unwrap();
        let now = 1_800_000_000;
        let store = QuotaHistoryStore::load(path.clone(), now);
        assert!(store.account_view("account-1").is_empty());
        let backup = path.with_extension(format!("json.corrupt-{now}"));
        assert!(backup.exists());
        assert!(!path.exists());
        let _ = fs::remove_file(backup);
    }
}
