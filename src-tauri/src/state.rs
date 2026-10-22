//! 应用状态与刷新协调。
//!
//! 前端只通过这里的脱敏状态读取账户、额度快照与查询状态；凭证（当前阶段尚未引入）
//! 不允许进入该结构。查询任务、缓存保留与自动刷新都在 Rust 侧完成，窗口关闭不影响。
//!
//! 持久化使用带版本号的 JSON，单写入入口按“克隆 → 修改 → 落盘 → 提交内存 →
//! 广播事件”的顺序执行，避免写入失败却对外报告成功。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::codex::{self, CodexUsage, UsageError};

pub const SCHEMA_VERSION: u32 = 1;
pub const STATE_EVENT: &str = "app-state-changed";
pub const DEFAULT_ACCOUNT_ID: &str = "codex-local";
pub const DEFAULT_PRODUCT_ID: &str = "codex-subscription";

/// 与前端设置页的可选项保持一致；后端在这里再次校验，不信任前端传入值。
pub const REFRESH_OPTIONS: [i64; 5] = [1, 5, 15, 30, 60];
const LOW_QUOTA_MIN: i64 = 5;
const LOW_QUOTA_MAX: i64 = 50;
const SCHEDULER_TICK: Duration = Duration::from_secs(5);

// ---------- 脱敏状态模型 ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub refresh_minutes: i64,
    pub low_quota_enabled: bool,
    pub low_quota_threshold: i64,
    pub auth_expired_enabled: bool,
    pub reset_notice_enabled: bool,
    pub auto_launch_enabled: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        // 默认值来自已确认需求：5 分钟刷新、低额度 20% 且默认开启、
        // 授权失效提醒默认开启、重置提醒与自启默认关闭。
        Self {
            refresh_minutes: 5,
            low_quota_enabled: true,
            low_quota_threshold: 20,
            auth_expired_enabled: true,
            reset_notice_enabled: false,
            auto_launch_enabled: false,
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if !REFRESH_OPTIONS.contains(&self.refresh_minutes) {
            return Err(format!("不支持的刷新间隔：{} 分钟", self.refresh_minutes));
        }
        if !(LOW_QUOTA_MIN..=LOW_QUOTA_MAX).contains(&self.low_quota_threshold) {
            return Err(format!(
                "低额度阈值需在 {LOW_QUOTA_MIN}% 到 {LOW_QUOTA_MAX}% 之间"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthState {
    Unknown,
    Ok,
    NeedsReauth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryState {
    pub refreshing: bool,
    pub last_attempt_epoch: Option<i64>,
    pub last_error: Option<QueryError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// 本地稳定标识，不依赖平台是否提供窗口名称。
    pub window_id: String,
    /// 平台返回的原始窗口标识（Codex 为 primary / secondary）。
    pub source_window_id: String,
    pub unit: String,
    /// 协议口径。缺失表示平台未提供，不能用 0 代替。
    pub used_percent: Option<i64>,
    /// 派生展示值。缺失表示无法可靠计算。
    pub remaining_percent: Option<i64>,
    pub window_duration_mins: Option<i64>,
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaGroup {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub plan_type: Option<String>,
    pub windows: Vec<QuotaWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub checked_at_epoch: i64,
    pub groups: Vec<QuotaGroup>,
    /// 必需字段缺失等能力缺口，明确记录而不是静默补零。
    pub capability_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductState {
    pub product_id: String,
    pub kind: String,
    pub display_name: String,
    pub plan_tier: Option<String>,
    pub snapshot: Option<QuotaSnapshot>,
    pub query: QueryState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub account_id: String,
    pub platform: String,
    pub display_name: String,
    pub auth_state: AuthState,
    pub products: Vec<ProductState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    /// 每次状态变更递增；前端据此丢弃乱序事件。
    pub revision: u64,
    pub accounts: Vec<AccountState>,
    pub preferences: Preferences,
}

pub fn default_state() -> PersistedState {
    PersistedState {
        schema_version: SCHEMA_VERSION,
        revision: 0,
        accounts: vec![AccountState {
            account_id: DEFAULT_ACCOUNT_ID.to_string(),
            platform: "codex".to_string(),
            display_name: "本机 Codex".to_string(),
            auth_state: AuthState::Unknown,
            products: vec![ProductState {
                product_id: DEFAULT_PRODUCT_ID.to_string(),
                kind: "codex-subscription".to_string(),
                display_name: "Codex 订阅".to_string(),
                plan_tier: None,
                snapshot: None,
                query: QueryState::default(),
            }],
        }],
        preferences: Preferences::default(),
    }
}

// ---------- 纯状态变换 ----------

fn epoch_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn account_mut<'a>(
    state: &'a mut PersistedState,
    account_id: &str,
) -> Option<&'a mut AccountState> {
    state
        .accounts
        .iter_mut()
        .find(|account| account.account_id == account_id)
}

fn snapshot_from_usage(usage: &CodexUsage) -> QuotaSnapshot {
    let mut windows_total = 0;
    let mut missing_duration = false;
    let mut missing_reset = false;

    let groups = usage
        .buckets
        .iter()
        .map(|bucket| {
            let windows = bucket
                .windows
                .iter()
                .enumerate()
                .map(|(index, window)| {
                    windows_total += 1;
                    missing_duration |= window.window_duration_mins.is_none();
                    missing_reset |= window.resets_at_epoch.is_none();
                    QuotaWindow {
                        window_id: format!("{}-{index}", window.role),
                        source_window_id: window.role.clone(),
                        unit: "percent".to_string(),
                        used_percent: Some(window.used_percent),
                        remaining_percent: Some(window.remaining_percent),
                        window_duration_mins: window.window_duration_mins,
                        resets_at_epoch: window.resets_at_epoch,
                    }
                })
                .collect::<Vec<_>>();
            QuotaGroup {
                limit_id: bucket.limit_id.clone(),
                limit_name: bucket.limit_name.clone(),
                plan_type: bucket.plan_type.clone(),
                windows,
            }
        })
        .collect::<Vec<_>>();

    let mut capability_gaps = Vec::new();
    if windows_total == 0 {
        capability_gaps.push("平台未返回额度窗口".to_string());
    }
    if missing_duration {
        capability_gaps.push("部分额度窗口未提供持续时间".to_string());
    }
    if missing_reset {
        capability_gaps.push("部分额度窗口未提供重置时间".to_string());
    }

    QuotaSnapshot {
        checked_at_epoch: usage.checked_at_epoch,
        groups,
        capability_gaps,
    }
}

fn is_codex_product(product: &ProductState) -> bool {
    product.kind == "codex-subscription"
}

fn mark_refreshing(state: &mut PersistedState, account_id: &str, now: i64) {
    if let Some(account) = account_mut(state, account_id) {
        for product in account.products.iter_mut().filter(|p| is_codex_product(p)) {
            product.query.refreshing = true;
            product.query.last_attempt_epoch = Some(now);
        }
    }
}

fn apply_success(state: &mut PersistedState, account_id: &str, usage: CodexUsage) {
    let Some(account) = account_mut(state, account_id) else {
        // 账户在请求期间被移除：不接受旧任务回写。
        return;
    };
    let snapshot = snapshot_from_usage(&usage);
    let plan_tier = snapshot
        .groups
        .iter()
        .find_map(|group| group.plan_type.clone());
    let checked_at = snapshot.checked_at_epoch;

    account.auth_state = AuthState::Ok;
    for product in account.products.iter_mut().filter(|p| is_codex_product(p)) {
        product.plan_tier = plan_tier.clone();
        product.snapshot = Some(snapshot.clone());
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(checked_at);
        product.query.last_error = None;
    }
}

fn apply_error(state: &mut PersistedState, account_id: &str, error: UsageError, now: i64) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    if error.code == "auth-expired" {
        account.auth_state = AuthState::NeedsReauth;
    }
    for product in account.products.iter_mut().filter(|p| is_codex_product(p)) {
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(now);
        // 失败只更新查询状态，保留上一次成功快照。
        product.query.last_error = Some(QueryError {
            code: error.code.clone(),
            message: error.message.clone(),
        });
    }
}

fn is_account_due(account: &AccountState, refresh_minutes: i64, now: i64) -> bool {
    if account.platform != "codex" {
        return false;
    }
    account
        .products
        .iter()
        .filter(|p| is_codex_product(p))
        .any(|product| {
            if product.query.refreshing {
                return false;
            }
            // 首次没有快照时优先补齐数据，即使上次尝试时间很近。
            if product.snapshot.is_none() {
                return true;
            }
            match product.query.last_attempt_epoch {
                None => true,
                Some(last) => now >= last.saturating_add(refresh_minutes.saturating_mul(60)),
            }
        })
}

// ---------- 文件持久化 ----------

fn write_state(path: &Path, state: &PersistedState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建应用数据目录失败：{err}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|err| format!("序列化应用状态失败：{err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|err| format!("写入应用状态失败：{err}"))?;
    // 状态含账户与用量信息，限制为仅当前用户可读写（macOS/Linux）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置应用状态权限失败：{err}"))?;
    }
    fs::rename(&tmp, path).map_err(|err| format!("替换应用状态失败：{err}"))?;
    Ok(())
}

fn backup_unreadable(path: &Path) {
    let backup = path.with_extension(format!("json.corrupt-{}", epoch_now()));
    let _ = fs::rename(path, backup);
}

fn load_state(path: &Path) -> PersistedState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        // 文件不存在或暂时不可读时都从默认状态启动；
        // 后续成功写入前不会覆盖原文件。
        Err(_) => return default_state(),
    };

    match serde_json::from_str::<PersistedState>(&raw) {
        Ok(state) if state.schema_version == SCHEMA_VERSION => state,
        Ok(_) | Err(_) => {
            // 版本不兼容或内容损坏：先备份原文件，避免静默覆盖后无法恢复。
            backup_unreadable(path);
            default_state()
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------- 共享状态与刷新协调 ----------

pub struct StateStore {
    inner: Mutex<PersistedState>,
    path: PathBuf,
    app: AppHandle,
    in_flight: Mutex<HashSet<String>>,
    shutdown: AtomicBool,
}

impl StateStore {
    pub fn load(app: AppHandle, path: PathBuf) -> Self {
        let state = load_state(&path);
        Self {
            inner: Mutex::new(state),
            path,
            app,
            in_flight: Mutex::new(HashSet::new()),
            shutdown: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> PersistedState {
        lock(&self.inner).clone()
    }

    fn emit(&self, state: &PersistedState) {
        let _ = self.app.emit(STATE_EVENT, state);
    }

    /// 严格更新：落盘失败时不提交内存，也不广播，调用方必须报告失败。
    fn update_strict<F>(&self, change: F) -> Result<PersistedState, String>
    where
        F: FnOnce(&mut PersistedState),
    {
        let mut guard = lock(&self.inner);
        let mut next = guard.clone();
        change(&mut next);
        next.revision = next.revision.saturating_add(1);
        write_state(&self.path, &next)?;
        *guard = next.clone();
        drop(guard);
        self.emit(&next);
        Ok(next)
    }

    /// 刷新结果更新：即使落盘失败也要展示最新查询状态，失败会记录到 stderr。
    fn update_soft<F>(&self, change: F) -> PersistedState
    where
        F: FnOnce(&mut PersistedState),
    {
        let mut guard = lock(&self.inner);
        let mut next = guard.clone();
        change(&mut next);
        next.revision = next.revision.saturating_add(1);
        if let Err(err) = write_state(&self.path, &next) {
            eprintln!("保存应用状态失败：{err}");
        }
        *guard = next.clone();
        drop(guard);
        self.emit(&next);
        next
    }

    pub fn update_preferences(&self, preferences: Preferences) -> Result<PersistedState, String> {
        preferences.validate()?;
        self.update_strict(|state| state.preferences = preferences)
    }

    /// 手动刷新入口。`account_id` 为空时刷新全部 Codex 账户；
    /// 同一账户已有在途请求时直接复用，不重复启动查询。
    pub fn request_refresh(self: &Arc<Self>, account_id: Option<&str>) -> Result<(), String> {
        let now = epoch_now();
        let state = self.snapshot();
        let targets: Vec<String> = match account_id {
            Some(id) => {
                if !state
                    .accounts
                    .iter()
                    .any(|account| account.account_id == id)
                {
                    return Err(format!("未知账户：{id}"));
                }
                vec![id.to_string()]
            }
            None => state
                .accounts
                .iter()
                .filter(|account| account.platform == "codex")
                .map(|account| account.account_id.clone())
                .collect(),
        };

        for target in targets {
            {
                let mut in_flight = lock(&self.in_flight);
                if !in_flight.insert(target.clone()) {
                    continue;
                }
            }

            self.update_soft(|state| mark_refreshing(state, &target, now));
            spawn_refresh(Arc::clone(self), target);
        }
        Ok(())
    }

    fn due_accounts(&self, now: i64) -> Vec<String> {
        let state = self.snapshot();
        let minutes = state.preferences.refresh_minutes;
        state
            .accounts
            .iter()
            .filter(|account| is_account_due(account, minutes, now))
            .map(|account| account.account_id.clone())
            .collect()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

fn spawn_refresh(store: Arc<StateStore>, account_id: String) {
    // 查询在独立线程执行，命令立即返回；UI 通过事件收到 refreshing 与结果。
    thread::spawn(move || {
        let result = codex::read_usage();
        let finished_at = epoch_now();
        match result {
            Ok(usage) => {
                store.update_soft(|state| apply_success(state, &account_id, usage));
            }
            Err(error) => {
                store.update_soft(|state| apply_error(state, &account_id, error, finished_at));
            }
        }
        let mut in_flight = lock(&store.in_flight);
        in_flight.remove(&account_id);
    });
}

pub fn start_scheduler(store: Arc<StateStore>) {
    thread::spawn(move || loop {
        if store.shutdown.load(Ordering::SeqCst) {
            break;
        }
        // 启动时立即检查一次：没有快照会马上补数据，有快照则按间隔等待。
        for account_id in store.due_accounts(epoch_now()) {
            let _ = store.request_refresh(Some(&account_id));
        }
        thread::sleep(SCHEDULER_TICK);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::{CodexLimitBucket, CodexWindow};

    fn synthetic_usage() -> CodexUsage {
        CodexUsage {
            checked_at_epoch: 1_790_000_000,
            buckets: vec![CodexLimitBucket {
                limit_id: Some("codex".to_string()),
                limit_name: None,
                plan_type: Some("plus".to_string()),
                windows: vec![
                    CodexWindow {
                        role: "primary".to_string(),
                        used_percent: 4,
                        remaining_percent: 96,
                        window_duration_mins: Some(300),
                        resets_at_epoch: Some(1_790_010_000),
                    },
                    CodexWindow {
                        role: "secondary".to_string(),
                        used_percent: 13,
                        remaining_percent: 87,
                        window_duration_mins: Some(10080),
                        resets_at_epoch: Some(1_790_500_000),
                    },
                ],
            }],
        }
    }

    fn temp_state_path(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("token-refill-state-{}-{label}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("state.json")
    }

    #[test]
    fn default_state_has_one_local_codex_product() {
        let state = default_state();
        assert_eq!(state.schema_version, SCHEMA_VERSION);
        assert_eq!(state.accounts.len(), 1);
        assert_eq!(state.accounts[0].account_id, DEFAULT_ACCOUNT_ID);
        assert_eq!(state.accounts[0].products[0].product_id, DEFAULT_PRODUCT_ID);
        assert!(state.accounts[0].products[0].snapshot.is_none());
    }

    #[test]
    fn success_updates_snapshot_and_clears_error() {
        let mut state = default_state();
        state.accounts[0].products[0].query.last_error = Some(QueryError {
            code: "timeout".to_string(),
            message: "previous failure".to_string(),
        });

        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage());

        let account = &state.accounts[0];
        let product = &account.products[0];
        assert_eq!(account.auth_state, AuthState::Ok);
        assert_eq!(product.plan_tier.as_deref(), Some("plus"));
        assert_eq!(product.query.last_error, None);
        assert!(!product.query.refreshing);
        let snapshot = product.snapshot.as_ref().expect("snapshot");
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].windows.len(), 2);
        assert_eq!(snapshot.groups[0].windows[0].remaining_percent, Some(96));
        assert_eq!(
            snapshot.groups[0].windows[1].window_duration_mins,
            Some(10080)
        );
        assert!(snapshot.capability_gaps.is_empty());
    }

    #[test]
    fn failure_keeps_last_snapshot_and_records_error() {
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage());
        let snapshot_before = state.accounts[0].products[0]
            .snapshot
            .clone()
            .expect("snapshot");

        apply_error(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "timeout".to_string(),
                message: "查询超时".to_string(),
            },
            1_790_100_000,
        );

        let product = &state.accounts[0].products[0];
        assert_eq!(product.snapshot.as_ref(), Some(&snapshot_before));
        assert!(!product.query.refreshing);
        assert_eq!(
            product.query.last_error.as_ref().map(|e| e.code.as_str()),
            Some("timeout")
        );
        assert_eq!(product.query.last_attempt_epoch, Some(1_790_100_000));
    }

    #[test]
    fn auth_error_marks_account_for_reauth() {
        let mut state = default_state();
        apply_error(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "auth-expired".to_string(),
                message: "需要重新登录".to_string(),
            },
            1_790_100_000,
        );
        assert_eq!(state.accounts[0].auth_state, AuthState::NeedsReauth);
    }

    #[test]
    fn missing_window_fields_are_recorded_as_capability_gaps() {
        let mut state = default_state();
        let usage = CodexUsage {
            checked_at_epoch: 1,
            buckets: vec![CodexLimitBucket {
                limit_id: None,
                limit_name: None,
                plan_type: None,
                windows: vec![CodexWindow {
                    role: "primary".to_string(),
                    used_percent: 0,
                    remaining_percent: 100,
                    window_duration_mins: None,
                    resets_at_epoch: None,
                }],
            }],
        };
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage);
        let snapshot = state.accounts[0].products[0].snapshot.as_ref().unwrap();
        assert_eq!(snapshot.groups[0].windows[0].window_duration_mins, None);
        assert_eq!(snapshot.groups[0].windows[0].resets_at_epoch, None);
        assert_eq!(snapshot.capability_gaps.len(), 2);
    }

    #[test]
    fn codex_refresh_does_not_touch_other_products() {
        let mut state = default_state();
        let mut other = state.accounts[0].products[0].clone();
        other.product_id = "bailian-token-plan".to_string();
        other.kind = "bailian-token-plan".to_string();
        other.display_name = "百炼 Token Plan".to_string();
        state.accounts[0].products.push(other);

        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage());

        let products = &state.accounts[0].products;
        assert!(products[0].snapshot.is_some());
        assert!(products[1].snapshot.is_none());
        assert!(products[1].plan_tier.is_none());
    }

    #[test]
    fn preferences_are_validated_before_saving() {
        let mut prefs = Preferences::default();
        assert!(prefs.validate().is_ok());

        prefs.refresh_minutes = 2;
        assert!(prefs.validate().is_err());

        prefs.refresh_minutes = 5;
        prefs.low_quota_threshold = 0;
        assert!(prefs.validate().is_err());

        prefs.low_quota_threshold = 100;
        assert!(prefs.validate().is_err());
    }

    #[test]
    fn due_accounts_follow_interval_and_first_snapshot() {
        let now = 1_790_000_000;

        // 没有快照时立即到期，即使刚尝试过。
        let mut state = default_state();
        state.accounts[0].products[0].query.last_attempt_epoch = Some(now);
        assert!(is_account_due(&state.accounts[0], 5, now));

        // 有快照后按刷新间隔判断。
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage());
        state.accounts[0].products[0].query.last_attempt_epoch = Some(now);
        assert!(!is_account_due(&state.accounts[0], 5, now));
        assert!(!is_account_due(&state.accounts[0], 5, now + 299));
        assert!(is_account_due(&state.accounts[0], 5, now + 300));

        // 在途刷新不重复调度。
        state.accounts[0].products[0].query.refreshing = true;
        assert!(!is_account_due(&state.accounts[0], 5, now + 600));
    }

    #[test]
    fn state_round_trips_through_disk() {
        let path = temp_state_path("round-trip");
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage());
        state.preferences.refresh_minutes = 15;

        write_state(&path, &state).expect("write state");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("state metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        let loaded = load_state(&path);

        assert_eq!(loaded, state);
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn corrupt_state_is_backed_up_instead_of_silently_overwritten() {
        let path = temp_state_path("corrupt");
        fs::write(&path, "{ not json").expect("write corrupt state");

        let loaded = load_state(&path);

        assert_eq!(loaded, default_state());
        let backup_exists = fs::read_dir(path.parent().unwrap())
            .expect("read temp dir")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(backup_exists);
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
