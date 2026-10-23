//! 应用状态与刷新协调。
//!
//! 前端只通过这里的脱敏状态读取账户、额度快照与查询状态；凭证（当前阶段尚未引入）
//! 不允许进入该结构。查询任务、缓存保留与自动刷新都在 Rust 侧完成，窗口关闭不影响。
//!
//! 持久化使用带版本号的 JSON，单写入入口按“克隆 → 修改 → 落盘 → 提交内存 →
//! 广播事件”的顺序执行，避免写入失败却对外报告成功。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::codex::{self, CodexIdentity, CodexUsage, UsageError};

pub const SCHEMA_VERSION: u32 = 1;
pub const STATE_EVENT: &str = "app-state-changed";
pub const LOGIN_EVENT: &str = "account-login-changed";
pub const DEFAULT_ACCOUNT_ID: &str = "codex-local";
pub const DEFAULT_PRODUCT_ID: &str = "codex-subscription";

/// 与前端设置页的可选项保持一致；后端在这里再次校验，不信任前端传入值。
pub const REFRESH_OPTIONS: [i64; 5] = [1, 5, 15, 30, 60];
const LOW_QUOTA_MIN: i64 = 5;
const LOW_QUOTA_MAX: i64 = 50;
const SCHEDULER_TICK: Duration = Duration::from_secs(5);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const DISPLAY_NAME_MAX_CHARS: usize = 60;
const PROFILES_DIR_NAME: &str = "codex-profiles";
static ACCOUNT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

/// 账户授权来源。Local 使用本机既有 Codex 登录态；Managed 使用应用独立
/// CODEX_HOME 中的官方授权记录。凭证始终不进入该结构。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccountSource {
    #[default]
    Local,
    Managed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub account_id: String,
    pub platform: String,
    pub display_name: String,
    pub auth_state: AuthState,
    #[serde(default)]
    pub source: AccountSource,
    /// 脱敏身份提示（如 j***@example.com），不包含令牌。
    #[serde(default)]
    pub identity_hint: Option<String>,
    pub products: Vec<ProductState>,
}

/// 发起授权后返回给界面的公开信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartPayload {
    pub login_id: String,
    pub account_id: String,
    pub auth_url: String,
}

/// 授权流程状态事件；只包含流程标识与脱敏结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginEvent {
    pub login_id: String,
    pub account_id: String,
    pub status: String,
    pub message: Option<String>,
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
            source: AccountSource::Local,
            identity_hint: None,
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

fn new_account_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let sequence = ACCOUNT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("codex-{millis}-{sequence}")
}

/// 账户标识会参与目录名拼接，因此只允许安全字符，避免路径逃逸。
fn is_valid_account_id(account_id: &str) -> bool {
    !account_id.is_empty()
        && account_id.len() <= 80
        && account_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 只用于界面识别，保留域名与首字符，避免完整邮箱进入常规展示。
fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((user, domain)) if !user.is_empty() && !domain.is_empty() => {
            let first = user.chars().next().unwrap_or('*');
            format!("{first}***@{domain}")
        }
        _ => "已授权账户".to_string(),
    }
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

/// 更新脱敏身份提示，并返回可用于套餐档位回退的 planType。
/// Some(None) 表示身份已确认退出登录；None 表示本次未读到身份，保留原值。
fn apply_identity(
    account: &mut AccountState,
    identity_update: &Option<Option<CodexIdentity>>,
) -> Option<String> {
    let identity = identity_update.as_ref()?;
    account.identity_hint = identity
        .as_ref()
        .and_then(|identity| identity.email.as_deref().map(mask_email));
    identity
        .as_ref()
        .and_then(|identity| identity.plan_type.clone())
}

fn apply_success(
    state: &mut PersistedState,
    account_id: &str,
    usage: CodexUsage,
    identity_update: Option<Option<CodexIdentity>>,
) {
    let Some(account) = account_mut(state, account_id) else {
        // 账户在请求期间被移除：不接受旧任务回写。
        return;
    };
    let snapshot = snapshot_from_usage(&usage);
    let identity_plan = apply_identity(account, &identity_update);
    let plan_tier = snapshot
        .groups
        .iter()
        .find_map(|group| group.plan_type.clone())
        .or(identity_plan);
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

fn apply_error(
    state: &mut PersistedState,
    account_id: &str,
    error: UsageError,
    now: i64,
    identity_update: Option<Option<CodexIdentity>>,
) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    let identity_plan = apply_identity(account, &identity_update);
    if error.code == "auth-expired" {
        account.auth_state = AuthState::NeedsReauth;
    }
    for product in account.products.iter_mut().filter(|p| is_codex_product(p)) {
        if let Some(plan) = identity_plan.clone() {
            product.plan_tier = Some(plan);
        }
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(now);
        // 失败只更新查询状态，保留上一次成功快照。
        product.query.last_error = Some(QueryError {
            code: error.code.clone(),
            message: error.message.clone(),
        });
    }
}

/// 授权成功后的状态写入。新账户追加到列表；重新授权更新原账户，
/// 不重复创建记录。额度读取失败时保留已授权状态并记录查询错误。
fn apply_login_success(
    state: &mut PersistedState,
    account_id: &str,
    requested_name: Option<&str>,
    identity_hint: Option<String>,
    plan_tier: Option<String>,
    snapshot: Option<QuotaSnapshot>,
    query_error: Option<QueryError>,
    now: i64,
) {
    let checked_at = snapshot
        .as_ref()
        .map(|snapshot| snapshot.checked_at_epoch)
        .unwrap_or(now);
    let requested_name = requested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);

    if let Some(account) = account_mut(state, account_id) {
        account.auth_state = AuthState::Ok;
        account.source = AccountSource::Managed;
        if let Some(hint) = identity_hint {
            account.identity_hint = Some(hint);
        }
        for product in account.products.iter_mut().filter(|p| is_codex_product(p)) {
            if let Some(plan_tier) = plan_tier.clone() {
                product.plan_tier = Some(plan_tier);
            }
            if let Some(snapshot) = snapshot.clone() {
                product.snapshot = Some(snapshot);
            }
            product.query.refreshing = false;
            product.query.last_attempt_epoch = Some(checked_at);
            product.query.last_error = query_error.clone();
        }
        return;
    }

    let display_name = requested_name
        .or_else(|| identity_hint.clone())
        .unwrap_or_else(|| "Codex 账户".to_string());
    state.accounts.push(AccountState {
        account_id: account_id.to_string(),
        platform: "codex".to_string(),
        display_name,
        auth_state: AuthState::Ok,
        source: AccountSource::Managed,
        identity_hint,
        products: vec![ProductState {
            product_id: DEFAULT_PRODUCT_ID.to_string(),
            kind: "codex-subscription".to_string(),
            display_name: "Codex 订阅".to_string(),
            plan_tier,
            snapshot,
            query: QueryState {
                refreshing: false,
                last_attempt_epoch: Some(checked_at),
                last_error: query_error,
            },
        }],
    });
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
            // 首次没有快照时立即补数据；但首次失败后按刷新间隔退避，
            // 不能每 5 秒重试一次（那会让界面一直停留在“刷新中”）。
            if product.snapshot.is_none() && product.query.last_error.is_none() {
                return true;
            }
            match product.query.last_attempt_epoch {
                None => true,
                Some(last) => now >= last.saturating_add(refresh_minutes.saturating_mul(60)),
            }
        })
}

// ---------- 文件持久化 ----------

/// 授权目录由 Codex CLI 写入登录令牌，限制为仅当前用户可访问。
/// Unix 显式设 0700；Windows 的应用数据目录位于 `%APPDATA%\<identifier>`，
/// 默认 ACL 只授予当前用户、SYSTEM 与 Administrators，不再叠加 POSIX 权限位。
fn ensure_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("创建账户授权目录失败：{err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("设置账户授权目录权限失败：{err}"))?;
    }
    Ok(())
}

fn write_state(path: &Path, state: &PersistedState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建应用数据目录失败：{err}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|err| format!("序列化应用状态失败：{err}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|err| format!("写入应用状态失败：{err}"))?;
    // 状态含账户与用量信息，限制为仅当前用户可读写（Unix 权限位；
    // Windows 沿用 `%APPDATA%` 的按用户 ACL）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("设置应用状态权限失败：{err}"))?;
    }
    // Windows 上 `fs::rename` 走 MoveFileEx(REPLACE_EXISTING)，可覆盖已存在的状态文件。
    fs::rename(&tmp, path).map_err(|err| format!("替换应用状态失败：{err}"))?;
    Ok(())
}

fn backup_unreadable(path: &Path) {
    let backup = path.with_extension(format!("json.corrupt-{}", epoch_now()));
    let _ = fs::rename(path, backup);
}

/// `refreshing` 是运行时标记，不应跨进程存活。应用在刷新途中退出或被重启时，
/// 持久化文件可能留下 true；启动时统一清除，避免调度器永远跳过该账户。
fn clear_stale_refresh_flags(state: &mut PersistedState) {
    for account in &mut state.accounts {
        for product in &mut account.products {
            if product.query.refreshing {
                // 上次查询被进程退出打断：清掉标记，并让调度器启动后立即重试，
                // 而不是沿用那个永远不会完成的“最近尝试时间”。
                product.query.refreshing = false;
                product.query.last_attempt_epoch = None;
            }
        }
    }
}

fn clear_refreshing(state: &mut PersistedState, account_id: &str) {
    if let Some(account) = account_mut(state, account_id) {
        for product in account.products.iter_mut() {
            product.query.refreshing = false;
        }
    }
}

fn load_state(path: &Path) -> PersistedState {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        // 文件不存在或暂时不可读时都从默认状态启动；
        // 后续成功写入前不会覆盖原文件。
        Err(_) => return default_state(),
    };

    match serde_json::from_str::<PersistedState>(&raw) {
        Ok(mut state) if state.schema_version == SCHEMA_VERSION => {
            clear_stale_refresh_flags(&mut state);
            state
        }
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

struct PendingLogin {
    account_id: String,
    cancel: Arc<AtomicBool>,
}

pub struct StateStore {
    inner: Mutex<PersistedState>,
    path: PathBuf,
    profiles_dir: PathBuf,
    app: AppHandle,
    in_flight: Mutex<HashSet<String>>,
    logins: Mutex<HashMap<String, PendingLogin>>,
    shutdown: AtomicBool,
}

impl StateStore {
    pub fn load(app: AppHandle, path: PathBuf) -> Self {
        let state = load_state(&path);
        let profiles_dir = path
            .parent()
            .map(|parent| parent.join(PROFILES_DIR_NAME))
            .unwrap_or_else(|| PathBuf::from(PROFILES_DIR_NAME));
        Self {
            inner: Mutex::new(state),
            path,
            profiles_dir,
            app,
            in_flight: Mutex::new(HashSet::new()),
            logins: Mutex::new(HashMap::new()),
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

    fn private_profile_dir(&self, account_id: &str) -> Option<PathBuf> {
        if !is_valid_account_id(account_id) {
            return None;
        }
        Some(self.profiles_dir.join(account_id))
    }

    /// 发起官方浏览器授权。新账户授权成功后加入列表；重新授权复用原账户记录。
    pub fn begin_login(
        self: &Arc<Self>,
        account_id: Option<String>,
        display_name: Option<String>,
    ) -> Result<LoginStartPayload, String> {
        let state = self.snapshot();
        let (target_id, is_new) = match account_id {
            Some(id) => {
                let account = state
                    .accounts
                    .iter()
                    .find(|account| account.account_id == id)
                    .ok_or_else(|| format!("未知账户：{id}"))?;
                if account.source != AccountSource::Managed {
                    return Err(
                        "本机账户跟随本机 Codex 登录态，请使用“添加账户”接入独立账户。".to_string(),
                    );
                }
                (id, false)
            }
            None => (new_account_id(), true),
        };

        {
            // 官方登录回调端口由 app-server 占用，同一时间只允许一个授权流程。
            let logins = lock(&self.logins);
            if let Some(pending) = logins.values().next() {
                if pending.account_id == target_id {
                    return Err("该账户正在等待授权，请先完成或取消当前登录。".to_string());
                }
                return Err("已有其他账户的授权流程进行中，请先完成或取消。".to_string());
            }
        }

        let profile = self
            .private_profile_dir(&target_id)
            .ok_or_else(|| "账户标识无效".to_string())?;
        ensure_private_dir(&profile)?;

        let binary = codex::find_codex_binary().map_err(|err| err.message)?;
        let mut session =
            codex::LoginSession::start(&binary, &profile).map_err(|err| err.message)?;
        let info = session.info();
        let cancel = Arc::new(AtomicBool::new(false));

        lock(&self.logins).insert(
            info.login_id.clone(),
            PendingLogin {
                account_id: target_id.clone(),
                cancel: Arc::clone(&cancel),
            },
        );

        let store = Arc::clone(self);
        let login_id = info.login_id.clone();
        let thread_account_id = target_id.clone();
        let requested_name = display_name.filter(|name| !name.trim().is_empty());
        thread::spawn(move || {
            let outcome = session.wait(&cancel, LOGIN_TIMEOUT);
            lock(&store.logins).remove(&login_id);
            match outcome {
                Ok(outcome) => {
                    store.finish_login(&thread_account_id, requested_name.as_deref(), outcome);
                    store.emit_login_event(&login_id, &thread_account_id, "succeeded", None);
                }
                Err(error) => {
                    if is_new {
                        // 未完成授权的新账户不保留半成品目录。
                        let _ = fs::remove_dir_all(&profile);
                    }
                    let status = if error.code == "canceled" {
                        "canceled"
                    } else {
                        "failed"
                    };
                    store.emit_login_event(
                        &login_id,
                        &thread_account_id,
                        status,
                        Some(error.message),
                    );
                }
            }
        });

        Ok(LoginStartPayload {
            login_id: info.login_id,
            account_id: target_id,
            auth_url: info.auth_url,
        })
    }

    pub fn cancel_login(&self, login_id: &str) -> Result<(), String> {
        let logins = lock(&self.logins);
        let pending = logins
            .get(login_id)
            .ok_or_else(|| "未找到等待中的授权流程".to_string())?;
        pending.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn finish_login(
        &self,
        account_id: &str,
        requested_name: Option<&str>,
        outcome: codex::LoginOutcome,
    ) {
        let identity_hint = outcome
            .identity
            .as_ref()
            .and_then(|identity| identity.email.as_deref())
            .map(mask_email);
        let plan_tier = outcome
            .identity
            .as_ref()
            .and_then(|identity| identity.plan_type.clone())
            .or_else(|| {
                outcome.usage.as_ref().and_then(|usage| {
                    usage
                        .buckets
                        .iter()
                        .find_map(|bucket| bucket.plan_type.clone())
                })
            });
        let snapshot = outcome.usage.as_ref().map(snapshot_from_usage);
        let query_error = outcome.usage_error.map(|error| QueryError {
            code: error.code,
            message: error.message,
        });
        let now = epoch_now();
        self.update_soft(|state| {
            apply_login_success(
                state,
                account_id,
                requested_name,
                identity_hint,
                plan_tier,
                snapshot,
                query_error,
                now,
            )
        });
    }

    fn has_pending_login(&self, account_id: &str) -> bool {
        lock(&self.logins)
            .values()
            .any(|pending| pending.account_id == account_id)
    }

    fn emit_login_event(
        &self,
        login_id: &str,
        account_id: &str,
        status: &str,
        message: Option<String>,
    ) {
        let _ = self.app.emit(
            LOGIN_EVENT,
            AccountLoginEvent {
                login_id: login_id.to_string(),
                account_id: account_id.to_string(),
                status: status.to_string(),
                message,
            },
        );
    }

    pub fn rename_account(
        &self,
        account_id: &str,
        display_name: &str,
    ) -> Result<PersistedState, String> {
        let name = display_name.trim();
        if name.is_empty() {
            return Err("账户名称不能为空".to_string());
        }
        if name.chars().count() > DISPLAY_NAME_MAX_CHARS {
            return Err(format!("账户名称不能超过 {DISPLAY_NAME_MAX_CHARS} 个字符"));
        }
        let state = self.snapshot();
        if !state
            .accounts
            .iter()
            .any(|account| account.account_id == account_id)
        {
            return Err(format!("未知账户：{account_id}"));
        }
        let name = name.to_string();
        self.update_strict(|state| {
            if let Some(account) = account_mut(state, account_id) {
                account.display_name = name;
            }
        })
    }

    /// 移除独立授权账户：只清理本应用目录，不触碰本机 Codex 登录态。
    pub fn remove_account(&self, account_id: &str) -> Result<PersistedState, String> {
        let state = self.snapshot();
        let account = state
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .ok_or_else(|| format!("未知账户：{account_id}"))?;
        if account.source != AccountSource::Managed {
            return Err("本机 Codex 账户不能移除；它跟随本机 CLI 登录态。".to_string());
        }
        {
            let logins = lock(&self.logins);
            if logins
                .values()
                .any(|pending| pending.account_id == account_id)
            {
                return Err("该账户正在等待授权，请先取消授权再移除。".to_string());
            }
        }

        let profile = self
            .private_profile_dir(account_id)
            .ok_or_else(|| "账户标识无效".to_string())?;
        if profile.exists() {
            fs::remove_dir_all(&profile).map_err(|err| format!("删除账户授权目录失败：{err}"))?;
        }
        self.update_strict(|state| {
            state
                .accounts
                .retain(|account| account.account_id != account_id)
        })
    }

    /// Some(Some(home)) 为独立授权账户；Some(None) 为本机账户；
    /// None 表示账户已被移除，调用方应放弃该次查询。
    fn profile_home_for(&self, account_id: &str) -> Option<Option<PathBuf>> {
        let state = self.snapshot();
        let account = state
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)?;
        match account.source {
            AccountSource::Local => Some(None),
            AccountSource::Managed => self.private_profile_dir(account_id).map(Some),
        }
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
        let Some(home) = store.profile_home_for(&account_id) else {
            // 账户在排队期间被移除：不发起查询，也不回写旧任务。
            store.update_soft(|state| clear_refreshing(state, &account_id));
            lock(&store.in_flight).remove(&account_id);
            return;
        };
        let result = codex::read_usage(home.as_deref());
        // 重新授权期间旧凭证的查询结果可能晚到；丢弃它，避免覆盖授权后的新状态。
        if store.has_pending_login(&account_id) {
            store.update_soft(|state| clear_refreshing(state, &account_id));
            lock(&store.in_flight).remove(&account_id);
            return;
        }
        let finished_at = epoch_now();
        match result {
            Ok(readout) => {
                let identity_update = readout.identity_checked.then_some(readout.identity);
                match readout.usage {
                    Ok(usage) => {
                        store.update_soft(|state| {
                            apply_success(state, &account_id, usage, identity_update)
                        });
                    }
                    Err(error) => {
                        store.update_soft(|state| {
                            apply_error(state, &account_id, error, finished_at, identity_update)
                        });
                    }
                }
            }
            Err(error) => {
                store
                    .update_soft(|state| apply_error(state, &account_id, error, finished_at, None));
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

        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);

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
    fn success_shows_local_account_identity_and_clears_when_signed_out() {
        let identity = CodexIdentity {
            account_type: "chatgpt".to_string(),
            email: Some("rainbow@example.com".to_string()),
            plan_type: Some("plus".to_string()),
        };
        let mut state = default_state();
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            synthetic_usage(),
            Some(Some(identity.clone())),
        );
        assert_eq!(
            state.accounts[0].identity_hint.as_deref(),
            Some("r***@example.com")
        );

        // account/read 成功但账户为空：确认已退出登录，清除旧提示。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            synthetic_usage(),
            Some(None),
        );
        assert_eq!(state.accounts[0].identity_hint, None);

        // 身份读取失败（None）：保留已确认的身份，不误清空。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            synthetic_usage(),
            Some(Some(identity)),
        );
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);
        assert_eq!(
            state.accounts[0].identity_hint.as_deref(),
            Some("r***@example.com")
        );
    }

    #[test]
    fn failure_still_records_identity_when_account_read_succeeds() {
        let mut state = default_state();
        apply_error(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "rpc".to_string(),
                message: "网络错误".to_string(),
            },
            1_790_100_000,
            Some(Some(CodexIdentity {
                account_type: "chatgpt".to_string(),
                email: Some("rainbow@example.com".to_string()),
                plan_type: Some("plus".to_string()),
            })),
        );

        let account = &state.accounts[0];
        assert_eq!(account.identity_hint.as_deref(), Some("r***@example.com"));
        assert_eq!(account.products[0].plan_tier.as_deref(), Some("plus"));
        assert_eq!(
            account.products[0]
                .query
                .last_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("rpc")
        );
        // 普通网络错误不应被误标为需要重新授权。
        assert_eq!(account.auth_state, AuthState::Unknown);
    }

    #[test]
    fn failure_keeps_last_snapshot_and_records_error() {
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);
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
            None,
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
            None,
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
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage, None);
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

        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);

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

        // 首次没有快照且尚未失败：立即到期。
        let mut state = default_state();
        state.accounts[0].products[0].query.last_attempt_epoch = Some(now);
        assert!(is_account_due(&state.accounts[0], 5, now));

        // 没有快照但上次已失败：按刷新间隔退避，避免 5 秒重试死循环。
        state.accounts[0].products[0].query.last_error = Some(QueryError {
            code: "rpc".to_string(),
            message: "network error".to_string(),
        });
        assert!(!is_account_due(&state.accounts[0], 5, now));
        assert!(!is_account_due(&state.accounts[0], 5, now + 299));
        assert!(is_account_due(&state.accounts[0], 5, now + 300));

        // 有快照后按刷新间隔判断。
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);
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
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);
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
    fn login_success_appends_isolated_account_without_touching_local() {
        let mut state = default_state();
        apply_login_success(
            &mut state,
            "codex-synthetic-1",
            None,
            Some("s***@example.com".to_string()),
            Some("plus".to_string()),
            Some(snapshot_from_usage(&synthetic_usage())),
            None,
            1_790_000_000,
        );

        assert_eq!(state.accounts.len(), 2);
        let account = &state.accounts[1];
        assert_eq!(account.account_id, "codex-synthetic-1");
        assert_eq!(account.source, AccountSource::Managed);
        assert_eq!(account.auth_state, AuthState::Ok);
        assert_eq!(account.display_name, "s***@example.com");
        assert_eq!(account.identity_hint.as_deref(), Some("s***@example.com"));
        assert_eq!(account.products[0].plan_tier.as_deref(), Some("plus"));
        assert!(account.products[0].snapshot.is_some());

        // 本机账户仍保持 Local，数据与授权互不影响。
        assert_eq!(state.accounts[0].source, AccountSource::Local);
        assert_eq!(state.accounts[0].auth_state, AuthState::Unknown);
        assert!(state.accounts[0].products[0].snapshot.is_none());
    }

    #[test]
    fn login_success_updates_existing_managed_account_in_place() {
        let mut state = default_state();
        apply_login_success(
            &mut state,
            "codex-synthetic-1",
            Some("工作账户"),
            None,
            Some("plus".to_string()),
            Some(snapshot_from_usage(&synthetic_usage())),
            None,
            1_790_000_000,
        );
        apply_login_success(
            &mut state,
            "codex-synthetic-1",
            None,
            Some("w***@example.com".to_string()),
            Some("pro".to_string()),
            None,
            Some(QueryError {
                code: "timeout".to_string(),
                message: "额度查询超时".to_string(),
            }),
            1_790_100_000,
        );

        assert_eq!(state.accounts.len(), 2);
        let account = &state.accounts[1];
        assert_eq!(account.display_name, "工作账户");
        assert_eq!(account.auth_state, AuthState::Ok);
        assert_eq!(account.identity_hint.as_deref(), Some("w***@example.com"));
        assert_eq!(account.products[0].plan_tier.as_deref(), Some("pro"));
        // 失败只影响查询状态，不清掉上次成功快照。
        assert!(account.products[0].snapshot.is_some());
        assert_eq!(
            account.products[0]
                .query
                .last_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("timeout")
        );
    }

    #[test]
    fn account_id_validation_blocks_path_escapes() {
        assert!(is_valid_account_id("codex-1790000000000-0"));
        assert!(!is_valid_account_id("../escape"));
        assert!(!is_valid_account_id("a/b"));
        assert!(!is_valid_account_id(""));
    }

    #[test]
    fn email_hint_is_masked() {
        assert_eq!(mask_email("someone@example.com"), "s***@example.com");
        assert_eq!(mask_email("not-an-email"), "已授权账户");
        assert_eq!(mask_email("@example.com"), "已授权账户");
    }

    #[test]
    fn old_state_without_source_is_read_as_local_account() {
        let mut value = serde_json::to_value(default_state()).expect("serialize");
        let accounts = value["accounts"].as_array_mut().expect("accounts");
        let object = accounts[0].as_object_mut().expect("account object");
        object.remove("source");
        object.remove("identityHint");

        let state: PersistedState = serde_json::from_value(value).expect("legacy state parses");
        assert_eq!(state.accounts[0].source, AccountSource::Local);
        assert_eq!(state.accounts[0].identity_hint, None);
    }

    #[test]
    fn stale_refreshing_flag_is_cleared_on_load() {
        // 模拟应用在刷新途中退出：磁盘上留下 refreshing=true。
        let path = temp_state_path("stale-refreshing");
        let mut state = default_state();
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, synthetic_usage(), None);
        state.accounts[0].products[0].query.refreshing = true;
        state.accounts[0].products[0].query.last_attempt_epoch = Some(1_790_000_000);
        write_state(&path, &state).expect("write state");

        let loaded = load_state(&path);
        assert!(!loaded.accounts[0].products[0].query.refreshing);
        // 被打断的那次尝试不能继续压制调度；启动后应立即重试。
        assert_eq!(
            loaded.accounts[0].products[0].query.last_attempt_epoch,
            None
        );
        assert!(is_account_due(&loaded.accounts[0], 15, 1_790_000_100));
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
