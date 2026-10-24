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
use tauri_plugin_notification::NotificationExt;

use crate::antigravity::{self, AntigravityUsage};
use crate::api_key;
use crate::bailian::{
    self, BailianError, BailianReadout, CodingPlanUsage, CodingPlanWindow, TokenPlanSubscription,
    TokenPlanUsage,
};
use crate::codex::{self, CodexIdentity, CodexUsage, UsageError};
use crate::commandcode::{self, CommandCodeUsage};
use crate::opencode::{self, OpencodeUsage};
use crate::proxy::{self, ProxyMode};
use crate::usage_api::ProviderError;

pub const SCHEMA_VERSION: u32 = 1;
pub const STATE_EVENT: &str = "app-state-changed";
pub const LOGIN_EVENT: &str = "account-login-changed";
pub const DEFAULT_ACCOUNT_ID: &str = "codex-local";
pub const DEFAULT_PRODUCT_ID: &str = "codex-subscription";
pub const BAILIAN_TOKEN_PLAN_ID: &str = "bailian-token-plan";
pub const BAILIAN_CODING_PLAN_ID: &str = "bailian-coding-plan";
pub const OPENCODE_GO_PLATFORM: &str = "opencode-go";
pub const OPENCODE_GO_PRODUCT_ID: &str = "opencode-go";
pub const OPENCODE_LOCAL_ACCOUNT_ID: &str = "opencode-local";
pub const COMMAND_CODE_PLATFORM: &str = "commandcode";
pub const COMMAND_CODE_PRODUCT_ID: &str = "commandcode";
pub const ANTIGRAVITY_PLATFORM: &str = "antigravity";
pub const ANTIGRAVITY_PRODUCT_ID: &str = "antigravity";
pub const ANTIGRAVITY_ACCOUNT_ID: &str = "antigravity-local";

/// 与前端设置页的可选项保持一致；后端在这里再次校验，不信任前端传入值。
pub const REFRESH_OPTIONS: [i64; 5] = [1, 5, 15, 30, 60];
const LOW_QUOTA_MIN: i64 = 5;
const LOW_QUOTA_MAX: i64 = 50;
/// 去重键的分隔符；账户与窗口标识都来自受控字符，用不可见字符避免拼接歧义。
const REMINDER_KEY_SEP: char = '\u{1f}';
const SCHEDULER_TICK: Duration = Duration::from_secs(5);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const DISPLAY_NAME_MAX_CHARS: usize = 60;
const PROFILES_DIR_NAME: &str = "codex-profiles";
const BAILIAN_PROFILES_DIR_NAME: &str = "bailian-profiles";
const API_PROFILES_DIR_NAME: &str = "api-profiles";
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
    /// 网络代理模式；旧状态文件缺省为跟随系统。
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    /// 指定代理地址；仅在 `proxy_mode = manual` 时生效。
    #[serde(default)]
    pub proxy_url: Option<String>,
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
            proxy_mode: ProxyMode::System,
            proxy_url: None,
        }
    }
}

/// 普通设置保存不改变自启意图：该字段由系统登录项的实际状态决定，
/// 只能通过 `set_auto_launch` 写入，避免一次无关的设置保存把系统状态覆盖掉。
fn preferences_for_update(current: &Preferences, mut incoming: Preferences) -> Preferences {
    incoming.auto_launch_enabled = current.auto_launch_enabled;
    incoming
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
        if self.proxy_mode == ProxyMode::Manual {
            let url = self
                .proxy_url
                .as_deref()
                .ok_or_else(|| "指定代理时需要填写代理地址。".to_string())?;
            proxy::normalize_proxy_url(url)?;
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
    /// 平台侧窗口名称（如“订阅月”）；缺失时前端按持续时间展示。
    #[serde(default)]
    pub window_name: Option<String>,
    /// 计量单位，保留平台原始口径，例如 percent / calls。
    pub unit: String,
    /// 协议口径。缺失表示平台未提供，不能用 0 代替。
    pub used_percent: Option<i64>,
    /// 派生展示值。缺失表示无法可靠计算。
    pub remaining_percent: Option<i64>,
    /// 平台提供的绝对值；缺失时只展示比例，不推算数量。
    #[serde(default)]
    pub used_amount: Option<f64>,
    #[serde(default)]
    pub total_amount: Option<f64>,
    /// 平台只给出剩余余额时使用（例如订阅赠额未返回上限）。
    #[serde(default)]
    pub remaining_amount: Option<f64>,
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
    /// 平台是否确认用户已开通该类套餐；`None` 表示尚未确认或无法判断。
    /// 账户页据此只列出实际开通的套餐。
    #[serde(default)]
    pub active: Option<bool>,
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

/// 提醒去重记录：按账户/套餐/窗口记住上一次提醒所属的重置周期，
/// 保证同一额度周期只提醒一次，应用重启后也不会重复。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderMarks {
    #[serde(default)]
    pub low_quota: HashMap<String, LowQuotaMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LowQuotaMark {
    /// 上次提醒时该窗口的重置时间。`None` 表示平台未提供重置时间，
    /// 此时改用“跌破阈值”的边沿去重，回到阈值以上后再解除标记。
    pub resets_at_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub schema_version: u32,
    /// 每次状态变更递增；前端据此丢弃乱序事件。
    pub revision: u64,
    pub accounts: Vec<AccountState>,
    pub preferences: Preferences,
    /// 旧版本状态文件没有该字段，读取时回退为空记录。
    #[serde(default)]
    pub reminders: ReminderMarks,
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
                active: None,
                snapshot: None,
                query: QueryState::default(),
            }],
        }],
        preferences: Preferences::default(),
        reminders: ReminderMarks::default(),
    }
}

// ---------- 纯状态变换 ----------

fn epoch_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn new_account_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let sequence = ACCOUNT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
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
                        window_name: None,
                        unit: "percent".to_string(),
                        used_percent: Some(window.used_percent),
                        remaining_percent: Some(window.remaining_percent),
                        used_amount: None,
                        total_amount: None,
                        remaining_amount: None,
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

fn is_bailian_product(product: &ProductState) -> bool {
    matches!(
        product.kind.as_str(),
        "bailian-token-plan" | "bailian-coding-plan"
    )
}

fn is_opencode_product(product: &ProductState) -> bool {
    product.kind == OPENCODE_GO_PRODUCT_ID
}

fn is_commandcode_product(product: &ProductState) -> bool {
    product.kind == COMMAND_CODE_PRODUCT_ID
}

fn is_antigravity_product(product: &ProductState) -> bool {
    product.kind == ANTIGRAVITY_PRODUCT_ID
}

/// 调度与提醒需要覆盖所有已接入的产品；未识别平台不参与自动刷新。
fn is_refreshable_product(product: &ProductState) -> bool {
    is_codex_product(product)
        || is_bailian_product(product)
        || is_opencode_product(product)
        || is_commandcode_product(product)
        || is_antigravity_product(product)
}

fn is_supported_platform(platform: &str) -> bool {
    matches!(
        platform,
        "codex" | "bailian" | OPENCODE_GO_PLATFORM | COMMAND_CODE_PLATFORM | ANTIGRAVITY_PLATFORM
    )
}

fn supports_api_key(platform: &str) -> bool {
    matches!(platform, OPENCODE_GO_PLATFORM | COMMAND_CODE_PLATFORM)
}

/// API Key 平台的单一套餐定义。
fn api_key_product(platform: &str) -> Option<ProductState> {
    let (product_id, display_name) = match platform {
        OPENCODE_GO_PLATFORM => (OPENCODE_GO_PRODUCT_ID, "Go 订阅"),
        COMMAND_CODE_PLATFORM => (COMMAND_CODE_PRODUCT_ID, "订阅额度"),
        _ => return None,
    };
    Some(ProductState {
        product_id: product_id.to_string(),
        kind: product_id.to_string(),
        display_name: display_name.to_string(),
        plan_tier: None,
        active: None,
        snapshot: None,
        query: QueryState::default(),
    })
}

fn antigravity_products() -> Vec<ProductState> {
    vec![ProductState {
        product_id: ANTIGRAVITY_PRODUCT_ID.to_string(),
        kind: ANTIGRAVITY_PRODUCT_ID.to_string(),
        display_name: "模型额度".to_string(),
        plan_tier: None,
        active: None,
        snapshot: None,
        query: QueryState::default(),
    }]
}

fn platform_display_name(platform: &str) -> &'static str {
    match platform {
        OPENCODE_GO_PLATFORM => "OpenCode Go 账户",
        COMMAND_CODE_PLATFORM => "Command Code 账户",
        ANTIGRAVITY_PLATFORM => "本机 Antigravity",
        "bailian" => "百炼账户",
        _ => "Codex 账户",
    }
}

fn bailian_products() -> Vec<ProductState> {
    vec![
        ProductState {
            product_id: BAILIAN_TOKEN_PLAN_ID.to_string(),
            kind: BAILIAN_TOKEN_PLAN_ID.to_string(),
            display_name: "Token Plan".to_string(),
            plan_tier: None,
            active: None,
            snapshot: None,
            query: QueryState::default(),
        },
        ProductState {
            product_id: BAILIAN_CODING_PLAN_ID.to_string(),
            kind: BAILIAN_CODING_PLAN_ID.to_string(),
            display_name: "Coding Plan".to_string(),
            plan_tier: None,
            active: None,
            snapshot: None,
            query: QueryState::default(),
        },
    ]
}

fn clamp_percent(value: i64) -> i64 {
    value.clamp(0, 100)
}

fn token_plan_snapshot(
    usage: &TokenPlanUsage,
    subscription: Option<&TokenPlanSubscription>,
    checked_at: i64,
) -> QuotaSnapshot {
    let mut capability_gaps = Vec::new();
    match subscription {
        Some(subscription) if subscription.active && subscription.spec_code.is_some() => {}
        Some(subscription) if subscription.active => {
            capability_gaps.push("平台未返回 Token Plan 档位".to_string());
        }
        Some(_) => capability_gaps.push("未找到有效的 Token Plan 订阅".to_string()),
        None => capability_gaps.push("未能读取 Token Plan 订阅信息".to_string()),
    }
    if usage.used_percent.is_none() {
        capability_gaps.push("平台未返回 Token Plan 使用比例".to_string());
    }
    if usage.resets_at_epoch.is_none() {
        capability_gaps.push("平台未返回 Token Plan 重置时间".to_string());
    }
    let window = QuotaWindow {
        window_id: "token-plan-month".to_string(),
        source_window_id: "per1Month".to_string(),
        window_name: Some("订阅月".to_string()),
        unit: "percent".to_string(),
        used_percent: usage.used_percent,
        remaining_percent: usage.used_percent.map(|used| clamp_percent(100 - used)),
        used_amount: None,
        total_amount: None,
        remaining_amount: None,
        // 订阅月按购买日滚动，不能固定成 30 天。
        window_duration_mins: None,
        resets_at_epoch: usage.resets_at_epoch,
    };
    QuotaSnapshot {
        checked_at_epoch: checked_at,
        groups: vec![QuotaGroup {
            limit_id: Some("token-plan".to_string()),
            limit_name: None,
            plan_type: None,
            windows: vec![window],
        }],
        capability_gaps,
    }
}

fn coding_window_snapshot(
    window_id: &str,
    source_window_id: &str,
    window_name: &str,
    window_duration_mins: Option<i64>,
    window: &CodingPlanWindow,
    capability_gaps: &mut Vec<String>,
) -> QuotaWindow {
    let total = window.total.filter(|total| *total > 0);
    let used = window.used;
    if used.is_none() || total.is_none() {
        capability_gaps.push(format!("平台未返回{window_name}的完整额度"));
    }
    let used_percent = match (used, total) {
        (Some(used), Some(total)) => Some(clamp_percent(
            ((used as f64 / total as f64) * 100.0).round() as i64,
        )),
        _ => None,
    };
    QuotaWindow {
        window_id: window_id.to_string(),
        source_window_id: source_window_id.to_string(),
        window_name: Some(window_name.to_string()),
        unit: "calls".to_string(),
        used_percent,
        remaining_percent: used_percent.map(|used| clamp_percent(100 - used)),
        used_amount: used.map(|value| value as f64),
        total_amount: total.map(|value| value as f64),
        remaining_amount: None,
        window_duration_mins,
        resets_at_epoch: window.resets_at_epoch,
    }
}

fn coding_plan_snapshot(usage: &CodingPlanUsage, checked_at: i64) -> QuotaSnapshot {
    if !usage.found {
        return QuotaSnapshot {
            checked_at_epoch: checked_at,
            groups: Vec::new(),
            capability_gaps: vec!["未找到有效的 Coding Plan 订阅".to_string()],
        };
    }
    let mut capability_gaps = Vec::new();
    let windows = vec![
        coding_window_snapshot(
            "coding-5h",
            "per5Hour",
            "5 小时",
            Some(300),
            &usage.per_5_hour,
            &mut capability_gaps,
        ),
        coding_window_snapshot(
            "coding-week",
            "perWeek",
            "1 周",
            Some(10_080),
            &usage.per_week,
            &mut capability_gaps,
        ),
        coding_window_snapshot(
            "coding-month",
            "perBillMonth",
            "订阅月",
            None,
            &usage.per_bill_month,
            &mut capability_gaps,
        ),
    ];
    QuotaSnapshot {
        checked_at_epoch: checked_at,
        groups: vec![QuotaGroup {
            limit_id: Some("coding-plan".to_string()),
            limit_name: None,
            plan_type: usage.instance_type.clone(),
            windows,
        }],
        capability_gaps,
    }
}

fn is_product_for_platform(product: &ProductState, platform: &str) -> bool {
    match platform {
        OPENCODE_GO_PLATFORM => is_opencode_product(product),
        COMMAND_CODE_PLATFORM => is_commandcode_product(product),
        ANTIGRAVITY_PLATFORM => is_antigravity_product(product),
        _ => false,
    }
}

fn opencode_snapshot(usage: &OpencodeUsage) -> QuotaSnapshot {
    let mut capability_gaps = Vec::new();
    if usage
        .windows
        .iter()
        .any(|window| window.resets_at_epoch.is_none())
    {
        capability_gaps.push("部分额度窗口未提供重置时间".to_string());
    }

    let windows = usage
        .windows
        .iter()
        .map(|window| QuotaWindow {
            window_id: window.source_id.to_string(),
            source_window_id: window.source_id.to_string(),
            window_name: Some(window.name.to_string()),
            unit: "percent".to_string(),
            used_percent: Some(window.used_percent),
            remaining_percent: Some(clamp_percent(100 - window.used_percent)),
            used_amount: None,
            total_amount: None,
            remaining_amount: None,
            window_duration_mins: window.window_duration_mins,
            resets_at_epoch: window.resets_at_epoch,
        })
        .collect();

    QuotaSnapshot {
        checked_at_epoch: usage.checked_at_epoch,
        groups: vec![QuotaGroup {
            limit_id: Some("opencode-go".to_string()),
            limit_name: None,
            plan_type: None,
            windows,
        }],
        capability_gaps,
    }
}

fn rolling_window(
    window_id: &str,
    name: &str,
    duration_mins: i64,
    window: &commandcode::RollingWindow,
) -> QuotaWindow {
    QuotaWindow {
        window_id: window_id.to_string(),
        source_window_id: window_id.to_string(),
        window_name: Some(name.to_string()),
        unit: "usd".to_string(),
        used_percent: Some(window.used_percent),
        remaining_percent: Some(clamp_percent(100 - window.used_percent)),
        used_amount: Some(crate::quota::round_money(window.used)),
        total_amount: Some(crate::quota::round_money(window.cap)),
        remaining_amount: None,
        window_duration_mins: Some(duration_mins),
        resets_at_epoch: window.resets_at_epoch,
    }
}

fn commandcode_snapshot(usage: &CommandCodeUsage) -> QuotaSnapshot {
    let mut windows = Vec::new();
    if let Some(five_hour) = &usage.five_hour {
        windows.push(rolling_window("five-hour", "5 小时窗口", 300, five_hour));
    }
    if let Some(weekly) = &usage.weekly {
        windows.push(rolling_window("weekly", "1 周窗口", 10_080, weekly));
    }
    windows.push(QuotaWindow {
        window_id: "monthly".to_string(),
        source_window_id: "monthly".to_string(),
        window_name: Some("订阅月".to_string()),
        unit: "usd".to_string(),
        used_percent: usage.monthly.used_percent,
        remaining_percent: usage
            .monthly
            .used_percent
            .map(|used| clamp_percent(100 - used)),
        used_amount: usage.monthly.used.map(crate::quota::round_money),
        total_amount: usage.monthly.total.map(crate::quota::round_money),
        remaining_amount: Some(crate::quota::round_money(usage.monthly.remaining)),
        window_duration_mins: None,
        resets_at_epoch: usage.monthly.resets_at_epoch,
    });

    QuotaSnapshot {
        checked_at_epoch: usage.checked_at_epoch,
        groups: vec![QuotaGroup {
            limit_id: Some("commandcode".to_string()),
            limit_name: None,
            plan_type: usage.plan_tier.clone(),
            windows,
        }],
        capability_gaps: usage.capability_gaps.clone(),
    }
}

fn antigravity_snapshot(usage: &AntigravityUsage) -> QuotaSnapshot {
    let mut capability_gaps = Vec::new();
    let mut groups = Vec::new();
    for group in &usage.groups {
        let mut windows = Vec::new();
        for window in &group.windows {
            if window.remaining_percent.is_none() {
                capability_gaps.push(format!("平台未返回{name}的剩余比例", name = window.name));
            }
            windows.push(QuotaWindow {
                window_id: window.source_id.clone(),
                source_window_id: window.source_id.clone(),
                window_name: Some(window.name.clone()),
                unit: "percent".to_string(),
                used_percent: window
                    .remaining_percent
                    .map(|value| clamp_percent(100 - value)),
                remaining_percent: window.remaining_percent,
                used_amount: None,
                total_amount: None,
                remaining_amount: None,
                window_duration_mins: window.window_duration_mins,
                resets_at_epoch: window.resets_at_epoch,
            });
        }
        groups.push(QuotaGroup {
            limit_id: Some(group.name.clone()),
            limit_name: Some(group.name.clone()),
            plan_type: usage.plan_tier.clone(),
            windows,
        });
    }

    QuotaSnapshot {
        checked_at_epoch: usage.checked_at_epoch,
        groups,
        capability_gaps,
    }
}

/// 单套餐平台的成功写入：只更新该平台自己的产品。
fn apply_platform_success(
    state: &mut PersistedState,
    account_id: &str,
    platform: &str,
    snapshot: QuotaSnapshot,
    plan_tier: Option<String>,
    identity_hint: Option<String>,
    active: Option<bool>,
) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    account.auth_state = AuthState::Ok;
    if let Some(hint) = identity_hint {
        account.identity_hint = Some(hint);
    }
    let checked_at = snapshot.checked_at_epoch;
    for product in account
        .products
        .iter_mut()
        .filter(|product| is_product_for_platform(product, platform))
    {
        if let Some(plan_tier) = plan_tier.clone() {
            product.plan_tier = Some(plan_tier);
        }
        if let Some(active) = active {
            product.active = Some(active);
        }
        product.snapshot = Some(snapshot.clone());
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(checked_at);
        product.query.last_error = None;
    }
}

/// 单套餐平台的失败写入：保留上次成功快照，只更新查询状态。
fn apply_platform_error(
    state: &mut PersistedState,
    account_id: &str,
    platform: &str,
    error: &ProviderError,
    now: i64,
) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    if error.is_auth_error() {
        account.auth_state = AuthState::NeedsReauth;
    }
    for product in account
        .products
        .iter_mut()
        .filter(|product| is_product_for_platform(product, platform))
    {
        if error.code == "not-subscribed" {
            product.active = Some(false);
        }
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(now);
        product.query.last_error = Some(QueryError {
            code: error.code.clone(),
            message: error.message.clone(),
        });
    }
}

/// 把一次百炼查询结果写入对应产品。某一类失败只影响该产品，
/// 两个接口都判定授权失效时才把账户标为需要重新授权。
fn apply_bailian_readout(
    state: &mut PersistedState,
    account_id: &str,
    readout: &BailianReadout,
    now: i64,
) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    let mut any_auth_failed = false;
    let mut any_success = false;

    for product in account
        .products
        .iter_mut()
        .filter(|p| is_bailian_product(p))
    {
        let result = match product.kind.as_str() {
            "bailian-token-plan" => readout.token_plan.as_ref().map(|usage| {
                let subscription = readout.token_plan_subscription.as_ref();
                let plan_tier = subscription
                    .filter(|subscription| subscription.active)
                    .and_then(|subscription| subscription.spec_code.clone());
                let active = subscription.map(|subscription| subscription.active);
                (
                    token_plan_snapshot(usage, subscription, readout.checked_at_epoch),
                    plan_tier,
                    active,
                )
            }),
            "bailian-coding-plan" => readout.coding_plan.as_ref().map(|usage| {
                let plan_tier = usage.instance_type.as_ref().map(|value| value.to_string());
                (
                    coding_plan_snapshot(usage, readout.checked_at_epoch),
                    plan_tier,
                    Some(usage.found),
                )
            }),
            _ => continue,
        };
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(now);
        match result {
            Ok((snapshot, plan_tier, active)) => {
                if let Some(plan_tier) = plan_tier {
                    product.plan_tier = Some(plan_tier);
                }
                // 订阅接口失败（active=None）时保留上一次确认的状态。
                if active.is_some() {
                    product.active = active;
                }
                product.snapshot = Some(snapshot);
                product.query.last_error = None;
                any_success = true;
            }
            Err(error) => {
                if error.is_auth_error() {
                    any_auth_failed = true;
                }
                product.query.last_error = Some(QueryError {
                    code: error.code.clone(),
                    message: error.message.clone(),
                });
            }
        }
    }

    if let Some(identity) = &readout.identity {
        if account.display_name == "百炼账户" {
            // 新账户未指定名称时，优先使用控制台返回的真实昵称；纯数字 UID
            // 不作为账户名，避免把一个长数字当成用户名。
            if let Some(nickname) = identity.nickname() {
                account.display_name = nickname.to_string();
            }
        }
        if let Some(hint) = identity.hint() {
            account.identity_hint = Some(hint);
        }
    }
    // 只要有一类套餐读到数据，就说明控制台会话仍有效；只有两类都失败时
    // 才按“需要重新授权”处理，避免单个接口的权限差异误伤整个账户。
    if any_success {
        account.auth_state = AuthState::Ok;
    } else if any_auth_failed {
        account.auth_state = AuthState::NeedsReauth;
    }
}

/// 读取凭证或网络层失败会同时影响该账户的两类套餐；保留上次成功快照。
fn apply_bailian_error(
    state: &mut PersistedState,
    account_id: &str,
    error: &BailianError,
    now: i64,
) {
    let Some(account) = account_mut(state, account_id) else {
        return;
    };
    if error.is_auth_error() {
        account.auth_state = AuthState::NeedsReauth;
    }
    for product in account
        .products
        .iter_mut()
        .filter(|p| is_bailian_product(p))
    {
        product.query.refreshing = false;
        product.query.last_attempt_epoch = Some(now);
        product.query.last_error = Some(QueryError {
            code: error.code.clone(),
            message: error.message.clone(),
        });
    }
}

fn mark_refreshing(state: &mut PersistedState, account_id: &str, now: i64) {
    if let Some(account) = account_mut(state, account_id) {
        for product in account
            .products
            .iter_mut()
            .filter(|p| is_refreshable_product(p))
        {
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
        product.active = Some(true);
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
// 各参数相互独立、没有自然的聚合体，硬拆成结构体只会让调用点更难读。
#[allow(clippy::too_many_arguments)]
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
            product.active = Some(true);
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
            active: Some(true),
            snapshot,
            query: QueryState {
                refreshing: false,
                last_attempt_epoch: Some(checked_at),
                last_error: query_error,
            },
        }],
    });
}

/// 一条待发送的低额度提醒；只含展示所需的脱敏信息。
#[derive(Debug, Clone, PartialEq)]
pub struct LowQuotaNotice {
    pub account_name: String,
    pub window_label: String,
    pub remaining_percent: i64,
    pub threshold: i64,
}

/// 一条待发送的授权失效提醒。
#[derive(Debug, Clone, PartialEq)]
pub struct AuthExpiredNotice {
    pub account_name: String,
    pub platform: String,
    pub source: AccountSource,
}

/// 只在授权状态从非失效跃迁到失效时返回提醒。`auth_state` 本身随状态持久化，
/// 所以重复查询、重复失败与应用重启都不会重复提醒；重新授权成功后状态回到
/// `Ok`，下一次失效会重新提醒。普通网络错误不会把状态改为失效。
fn auth_expired_notice(
    state: &PersistedState,
    account_id: &str,
    was_reauth: bool,
) -> Option<AuthExpiredNotice> {
    if was_reauth || !state.preferences.auth_expired_enabled {
        return None;
    }
    let account = state
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)?;
    (account.auth_state == AuthState::NeedsReauth).then(|| AuthExpiredNotice {
        account_name: account.display_name.clone(),
        platform: account.platform.clone(),
        source: account.source,
    })
}

fn reminder_key(account_id: &str, product_id: &str, window_id: &str) -> String {
    format!("{account_id}{REMINDER_KEY_SEP}{product_id}{REMINDER_KEY_SEP}{window_id}")
}

/// 与总览页 `durationLabel` 保持一致，避免通知和界面使用不同的周期叫法。
fn window_label(mins: Option<i64>) -> String {
    match mins {
        None => "额度窗口".to_string(),
        Some(mins) if mins < 60 => format!("{mins} 分钟窗口"),
        Some(mins) if mins < 1440 => format!("{} 小时窗口", (mins as f64 / 60.0).round() as i64),
        Some(10080) => "1 周窗口".to_string(),
        Some(mins) => format!("{} 天窗口", (mins as f64 / 1440.0).round() as i64),
    }
}

/// 依据刚写入的快照判断是否需要发低额度提醒，并同步更新去重记录。
///
/// - 同一重置周期内只提醒一次，重启后仍保持；
/// - 平台未提供重置时间时，退回“从阈值以上跌破”的边沿触发；
/// - 剩余比例缺失时跳过，不用猜测值触发提醒。
fn low_quota_notices(state: &mut PersistedState, account_id: &str) -> Vec<LowQuotaNotice> {
    if !state.preferences.low_quota_enabled {
        return Vec::new();
    }
    let threshold = state.preferences.low_quota_threshold;

    // 先收集候选，结束对账户的可变借用后再写去重记录。
    let (account_name, candidates) = {
        let Some(account) = account_mut(state, account_id) else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        let multi_product = account
            .products
            .iter()
            .filter(|p| is_refreshable_product(p))
            .count()
            > 1;
        for product in account
            .products
            .iter()
            .filter(|p| is_refreshable_product(p))
        {
            let Some(snapshot) = &product.snapshot else {
                continue;
            };
            for group in &snapshot.groups {
                for window in &group.windows {
                    let Some(remaining) = window.remaining_percent else {
                        continue;
                    };
                    let base_label = window
                        .window_name
                        .clone()
                        .unwrap_or_else(|| window_label(window.window_duration_mins));
                    let label = if multi_product {
                        format!("{} · {base_label}", product.display_name)
                    } else {
                        base_label
                    };
                    candidates.push((
                        reminder_key(account_id, &product.product_id, &window.window_id),
                        remaining,
                        window.resets_at_epoch,
                        label,
                    ));
                }
            }
        }
        (account.display_name.clone(), candidates)
    };

    let mut notices = Vec::new();
    for (key, remaining, resets_at, label) in candidates {
        let mark = state.reminders.low_quota.get(&key).cloned();
        if remaining <= threshold {
            let already_notified = match (&mark, resets_at) {
                (Some(mark), Some(cycle)) => mark.resets_at_epoch == Some(cycle),
                // 没有重置时间可比较时，只要提醒过就视为同一周期。
                (Some(_), None) => true,
                (None, _) => false,
            };
            if !already_notified {
                state.reminders.low_quota.insert(
                    key,
                    LowQuotaMark {
                        resets_at_epoch: resets_at,
                    },
                );
                notices.push(LowQuotaNotice {
                    account_name: account_name.clone(),
                    window_label: label,
                    remaining_percent: remaining,
                    threshold,
                });
            }
        } else if resets_at.is_none() {
            // 无法用重置时间判断周期：回到阈值以上后解除标记，下次跌破再提醒。
            // 有重置时间的标记保留到周期变化，避免同一周期补货后再次跌破时重复提醒。
            state.reminders.low_quota.remove(&key);
        }
    }
    notices
}

/// 一条待发送的额度重置提醒。
#[derive(Debug, Clone, PartialEq)]
pub struct ResetNotice {
    pub account_name: String,
    pub window_label: String,
    pub remaining_percent: Option<i64>,
}

/// 在写入新快照前记录各窗口的重置时间，作为判断新周期的基线。
fn window_resets(state: &PersistedState, account_id: &str) -> HashMap<String, i64> {
    let Some(account) = state
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
    else {
        return HashMap::new();
    };
    let mut resets = HashMap::new();
    for product in account
        .products
        .iter()
        .filter(|p| is_refreshable_product(p))
    {
        let Some(snapshot) = &product.snapshot else {
            continue;
        };
        for group in &snapshot.groups {
            for window in &group.windows {
                if let Some(reset) = window.resets_at_epoch {
                    resets.insert(
                        reminder_key(account_id, &product.product_id, &window.window_id),
                        reset,
                    );
                }
            }
        }
    }
    resets
}

/// 只有查询结果同时确认“旧周期已经结束”和“新周期已经开始”时才提醒：
/// 重置时间必须晚于旧值、旧周期在上次查询前已到期、新周期在本次查询后仍有效。
/// 不根据本地倒计时或重置时间的提前小幅调整推断重置。
fn collect_reset_notices(
    state: &PersistedState,
    account_id: &str,
    previous_resets: &HashMap<String, i64>,
) -> Vec<ResetNotice> {
    if !state.preferences.reset_notice_enabled {
        return Vec::new();
    }
    let Some(account) = state
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
    else {
        return Vec::new();
    };

    let mut notices = Vec::new();
    let multi_product = account
        .products
        .iter()
        .filter(|p| is_refreshable_product(p))
        .count()
        > 1;
    for product in account
        .products
        .iter()
        .filter(|p| is_refreshable_product(p))
    {
        let Some(snapshot) = &product.snapshot else {
            continue;
        };
        let checked_at = snapshot.checked_at_epoch;
        for group in &snapshot.groups {
            for window in &group.windows {
                let Some(new_reset) = window.resets_at_epoch else {
                    continue;
                };
                let key = reminder_key(account_id, &product.product_id, &window.window_id);
                let Some(&old_reset) = previous_resets.get(&key) else {
                    continue;
                };
                if new_reset > old_reset && old_reset <= checked_at && new_reset > checked_at {
                    let base_label = window
                        .window_name
                        .clone()
                        .unwrap_or_else(|| window_label(window.window_duration_mins));
                    let label = if multi_product {
                        format!("{} · {base_label}", product.display_name)
                    } else {
                        base_label
                    };
                    notices.push(ResetNotice {
                        account_name: account.display_name.clone(),
                        window_label: label,
                        remaining_percent: window.remaining_percent,
                    });
                }
            }
        }
    }
    notices
}

/// 账户移除后清掉它的去重记录，避免留下永不复用的键。
fn clear_reminder_marks(state: &mut PersistedState, account_id: &str) {
    let prefix = format!("{account_id}{REMINDER_KEY_SEP}");
    state
        .reminders
        .low_quota
        .retain(|key, _| !key.starts_with(&prefix));
}

fn is_account_due(account: &AccountState, refresh_minutes: i64, now: i64) -> bool {
    if !is_supported_platform(&account.platform) {
        return false;
    }
    account
        .products
        .iter()
        .filter(|p| is_refreshable_product(p))
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

/// 仍占用授权入口的流程。已请求取消的流程正在回收回调端口与子进程，
/// 不再阻塞新的授权；否则用户取消后要等后台线程收尾才能重新发起。
fn active_login(logins: &HashMap<String, PendingLogin>) -> Option<&PendingLogin> {
    logins
        .values()
        .find(|pending| !pending.cancel.load(Ordering::SeqCst))
}

pub struct StateStore {
    inner: Mutex<PersistedState>,
    path: PathBuf,
    profiles_dir: PathBuf,
    bailian_dir: PathBuf,
    api_dir: PathBuf,
    app: AppHandle,
    in_flight: Mutex<HashSet<String>>,
    logins: Mutex<HashMap<String, PendingLogin>>,
    shutdown: AtomicBool,
}

impl StateStore {
    pub fn load(app: AppHandle, path: PathBuf) -> Self {
        let state = load_state(&path);
        proxy::configure(
            state.preferences.proxy_mode,
            state.preferences.proxy_url.clone(),
        );
        let parent = path.parent();
        let profiles_dir = parent
            .map(|parent| parent.join(PROFILES_DIR_NAME))
            .unwrap_or_else(|| PathBuf::from(PROFILES_DIR_NAME));
        let bailian_dir = parent
            .map(|parent| parent.join(BAILIAN_PROFILES_DIR_NAME))
            .unwrap_or_else(|| PathBuf::from(BAILIAN_PROFILES_DIR_NAME));
        let api_dir = parent
            .map(|parent| parent.join(API_PROFILES_DIR_NAME))
            .unwrap_or_else(|| PathBuf::from(API_PROFILES_DIR_NAME));
        Self {
            inner: Mutex::new(state),
            path,
            profiles_dir,
            bailian_dir,
            api_dir,
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

    /// 提醒走系统通知；发送失败只记录日志，不影响额度查询与状态落盘。
    fn show_notification(&self, title: &str, body: String) {
        if let Err(err) = self
            .app
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            eprintln!("发送「{title}」失败：{err}");
        }
    }

    fn notify_low_quota(&self, notice: &LowQuotaNotice) {
        let body = format!(
            "{} · {}剩余 {}%，低于提醒阈值 {}%",
            notice.account_name, notice.window_label, notice.remaining_percent, notice.threshold
        );
        self.show_notification("低额度提醒", body);
    }

    fn notify_reset(&self, notice: &ResetNotice) {
        let body = match notice.remaining_percent {
            Some(remaining) => format!(
                "{} · {}已进入新周期，剩余额度 {remaining}%",
                notice.account_name, notice.window_label
            ),
            None => format!(
                "{} · {}已进入新周期",
                notice.account_name, notice.window_label
            ),
        };
        self.show_notification("额度重置提醒", body);
    }

    fn notify_auth_expired(&self, notice: &AuthExpiredNotice) {
        let action = match (notice.platform.as_str(), notice.source) {
            ("codex", AccountSource::Local) => "请重新登录本机 Codex CLI 后再刷新。",
            (ANTIGRAVITY_PLATFORM, _) => "请先在终端重新运行 agy 完成登录后再刷新。",
            (OPENCODE_GO_PLATFORM, AccountSource::Local) => {
                "请先在 opencode 中登录，或在账户页重新填写 API Key。"
            }
            (OPENCODE_GO_PLATFORM | COMMAND_CODE_PLATFORM, _) => {
                "请在账户页重新填写 API Key 后再刷新。"
            }
            _ => "请打开 Token Refill 重新授权。",
        };
        self.show_notification(
            "授权失效提醒",
            format!("{} 的授权已失效，{}", notice.account_name, action),
        );
    }

    /// 应用一次刷新失败，并在授权状态跃迁到“需要重新授权”时提醒。
    fn apply_refresh_error(
        &self,
        account_id: &str,
        error: UsageError,
        now: i64,
        identity_update: Option<Option<CodexIdentity>>,
    ) {
        let mut notice = None;
        self.update_soft(|state| {
            let was_reauth = state
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.auth_state == AuthState::NeedsReauth)
                .unwrap_or(true);
            apply_error(state, account_id, error, now, identity_update);
            notice = auth_expired_notice(state, account_id, was_reauth);
        });
        if let Some(notice) = notice {
            self.notify_auth_expired(&notice);
        }
    }

    /// 百炼查询失败：只记录对应产品的错误并保留上次成功快照；
    /// 授权失效时与 Codex 一样按状态跃迁提醒一次。
    fn apply_bailian_refresh_error(&self, account_id: &str, error: &BailianError, now: i64) {
        let mut notice = None;
        self.update_soft(|state| {
            let was_reauth = state
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.auth_state == AuthState::NeedsReauth)
                .unwrap_or(true);
            apply_bailian_error(state, account_id, error, now);
            notice = auth_expired_notice(state, account_id, was_reauth);
        });
        if let Some(notice) = notice {
            self.notify_auth_expired(&notice);
        }
    }

    /// 单套餐平台失败：记录查询错误，授权失效时按状态跃迁提醒一次。
    fn apply_platform_refresh_error(
        &self,
        account_id: &str,
        platform: &str,
        error: &ProviderError,
    ) {
        let now = epoch_now();
        let mut notice = None;
        self.update_soft(|state| {
            let was_reauth = state
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.auth_state == AuthState::NeedsReauth)
                .unwrap_or(true);
            apply_platform_error(state, account_id, platform, error, now);
            notice = auth_expired_notice(state, account_id, was_reauth);
        });
        if let Some(notice) = notice {
            self.notify_auth_expired(&notice);
        }
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
        let mut next = preferences_for_update(&self.snapshot().preferences, preferences);
        next.validate()?;
        if next.proxy_mode == ProxyMode::Manual {
            next.proxy_url = Some(proxy::normalize_proxy_url(
                next.proxy_url.as_deref().unwrap_or(""),
            )?);
        } else {
            next.proxy_url = next
                .proxy_url
                .map(|url| url.trim().to_string())
                .filter(|url| !url.is_empty());
        }
        let state = self.update_strict(|state| state.preferences = next)?;
        proxy::configure(
            state.preferences.proxy_mode,
            state.preferences.proxy_url.clone(),
        );
        Ok(state)
    }

    /// 自启设置已由系统确认生效后，回写偏好并返回新状态。
    pub fn set_auto_launch_preference(&self, enabled: bool) -> Result<PersistedState, String> {
        self.update_strict(|state| state.preferences.auto_launch_enabled = enabled)
    }

    /// 启动时把系统登录项的实际状态同步到偏好。失败只记录日志，不阻断启动。
    pub fn reconcile_auto_launch(&self, enabled: bool) {
        if self.snapshot().preferences.auto_launch_enabled == enabled {
            return;
        }
        if let Err(err) =
            self.update_strict(|state| state.preferences.auto_launch_enabled = enabled)
        {
            eprintln!("同步开机自启状态失败：{err}");
        }
    }

    fn private_profile_dir(&self, account_id: &str) -> Option<PathBuf> {
        if !is_valid_account_id(account_id) {
            return None;
        }
        Some(self.profiles_dir.join(account_id))
    }

    fn private_bailian_dir(&self, account_id: &str) -> Option<PathBuf> {
        if !is_valid_account_id(account_id) {
            return None;
        }
        Some(self.bailian_dir.join(account_id))
    }

    fn private_api_dir(&self, account_id: &str) -> Option<PathBuf> {
        if !is_valid_account_id(account_id) {
            return None;
        }
        Some(self.api_dir.join(account_id))
    }

    /// 用 API Key 接入 OpenCode Go / Command Code。先完成一次真实查询再落库，
    /// 避免把无效密钥保存下来；密钥只写入该账户的私有目录。
    pub fn add_api_key_account(
        &self,
        platform: &str,
        display_name: Option<&str>,
        api_key: &str,
    ) -> Result<PersistedState, String> {
        if !supports_api_key(platform) {
            return Err("该平台不支持使用 API Key 接入。".to_string());
        }
        let api_key = api_key::validate(api_key)?;
        let (snapshot, plan_tier, active) =
            probe_api_key(platform, &api_key).map_err(|error| error.message)?;

        let product = api_key_product(platform).ok_or_else(|| "未知平台".to_string())?;
        let account_id = new_account_id(platform);
        let profile = self
            .private_api_dir(&account_id)
            .ok_or_else(|| "账户标识无效".to_string())?;
        api_key::save(&profile, platform, &api_key)?;

        let name = display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| platform_display_name(platform).to_string());
        let checked_at = snapshot.checked_at_epoch;
        let result = self.update_strict(|state| {
            state.accounts.push(AccountState {
                account_id: account_id.clone(),
                platform: platform.to_string(),
                display_name: name,
                auth_state: AuthState::Ok,
                source: AccountSource::Managed,
                identity_hint: None,
                products: vec![ProductState {
                    plan_tier,
                    active,
                    snapshot: Some(snapshot),
                    query: QueryState {
                        refreshing: false,
                        last_attempt_epoch: Some(checked_at),
                        last_error: None,
                    },
                    ..product
                }],
            });
        });
        if result.is_err() {
            let _ = fs::remove_dir_all(&profile);
        }
        result
    }

    /// 重新填写 API Key：验证通过后覆盖凭证并立即更新快照。
    pub fn replace_api_key(
        &self,
        account_id: &str,
        api_key: &str,
    ) -> Result<PersistedState, String> {
        let state = self.snapshot();
        let account = state
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .ok_or_else(|| format!("未知账户：{account_id}"))?;
        let platform = account.platform.clone();
        if !supports_api_key(&platform) || account.source != AccountSource::Managed {
            return Err("该账户不使用 API Key 授权。".to_string());
        }
        let api_key = api_key::validate(api_key)?;
        let (snapshot, plan_tier, active) =
            probe_api_key(&platform, &api_key).map_err(|error| error.message)?;

        let profile = self
            .private_api_dir(account_id)
            .ok_or_else(|| "账户标识无效".to_string())?;
        api_key::save(&profile, &platform, &api_key)?;
        self.update_strict(|state| {
            apply_platform_success(
                state, account_id, &platform, snapshot, plan_tier, None, active,
            )
        })
    }

    /// 接入本机 Antigravity CLI 登录态。额度查询走 `agy` 官方打印模式，
    /// 应用不读取或复制 CLI 的令牌。查询在后台执行，命令立即返回。
    pub fn add_antigravity_account(
        self: &Arc<Self>,
        display_name: Option<&str>,
    ) -> Result<PersistedState, String> {
        if self
            .snapshot()
            .accounts
            .iter()
            .any(|account| account.platform == ANTIGRAVITY_PLATFORM)
        {
            return Err("已存在本机 Antigravity 账户。".to_string());
        }
        let name = display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| platform_display_name(ANTIGRAVITY_PLATFORM).to_string());
        let state = self.update_strict(|state| {
            state.accounts.push(AccountState {
                account_id: ANTIGRAVITY_ACCOUNT_ID.to_string(),
                platform: ANTIGRAVITY_PLATFORM.to_string(),
                display_name: name,
                auth_state: AuthState::Unknown,
                source: AccountSource::Local,
                identity_hint: None,
                products: antigravity_products(),
            });
        })?;
        let _ = self.request_refresh(Some(ANTIGRAVITY_ACCOUNT_ID));
        Ok(state)
    }

    /// 启动时自动接入本机 OpenCode CLI 已保存的凭据。只读取官方
    /// `opencode auth export` 的输出；找不到凭据时保持未接入，由用户手动添加。
    fn detect_local_opencode(self: &Arc<Self>) {
        if self
            .snapshot()
            .accounts
            .iter()
            .any(|account| account.platform == OPENCODE_GO_PLATFORM)
        {
            return;
        }
        if opencode::read_cli_api_key().is_err() {
            return;
        }
        let Some(product) = api_key_product(OPENCODE_GO_PLATFORM) else {
            return;
        };
        let mut added = false;
        self.update_soft(|state| {
            if state
                .accounts
                .iter()
                .any(|account| account.platform == OPENCODE_GO_PLATFORM)
            {
                return;
            }
            state.accounts.push(AccountState {
                account_id: OPENCODE_LOCAL_ACCOUNT_ID.to_string(),
                platform: OPENCODE_GO_PLATFORM.to_string(),
                display_name: "本机 OpenCode".to_string(),
                auth_state: AuthState::Unknown,
                source: AccountSource::Local,
                identity_hint: None,
                products: vec![product],
            });
            added = true;
        });
        if added {
            let _ = self.request_refresh(Some(OPENCODE_LOCAL_ACCOUNT_ID));
        }
    }

    /// 启动时检查本机 agy 登录态：能直接读到额度才自动接入；
    /// 未安装或未登录时保持未接入，避免制造一个必然失败的账户。
    fn detect_local_antigravity(self: &Arc<Self>) {
        if self
            .snapshot()
            .accounts
            .iter()
            .any(|account| account.platform == ANTIGRAVITY_PLATFORM)
        {
            return;
        }
        if antigravity::find_agy_binary().is_err() {
            return;
        }
        let Ok(usage) = antigravity::read_usage() else {
            return;
        };
        let snapshot = antigravity_snapshot(&usage);
        let checked_at = snapshot.checked_at_epoch;
        let plan_tier = usage.plan_tier.clone();
        let identity_hint = usage
            .account
            .as_deref()
            .map(mask_email)
            .filter(|hint| hint != "已授权账户");
        let mut products = antigravity_products();
        if let Some(product) = products.first_mut() {
            product.plan_tier = plan_tier;
            product.active = Some(true);
            product.snapshot = Some(snapshot);
            product.query.last_attempt_epoch = Some(checked_at);
        }
        let mut notices = Vec::new();
        let mut added = false;
        self.update_soft(|state| {
            if state
                .accounts
                .iter()
                .any(|account| account.platform == ANTIGRAVITY_PLATFORM)
            {
                return;
            }
            state.accounts.push(AccountState {
                account_id: ANTIGRAVITY_ACCOUNT_ID.to_string(),
                platform: ANTIGRAVITY_PLATFORM.to_string(),
                display_name: platform_display_name(ANTIGRAVITY_PLATFORM).to_string(),
                auth_state: AuthState::Ok,
                source: AccountSource::Local,
                identity_hint,
                products,
            });
            notices = low_quota_notices(state, ANTIGRAVITY_ACCOUNT_ID);
            added = true;
        });
        if added {
            for notice in &notices {
                self.notify_low_quota(notice);
            }
        }
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
            None => (new_account_id("codex"), true),
        };

        {
            // 官方登录回调端口由 app-server 占用，同一时间只允许一个授权流程。
            let logins = lock(&self.logins);
            if let Some(pending) = active_login(&logins) {
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

    /// 发起百炼控制台浏览器授权。新账户成功后加入列表；重新授权复用原账户。
    /// 令牌只写入应用私有目录，事件与状态里都只有流程标识与结果。
    pub fn begin_bailian_login(
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
                if account.platform != "bailian" {
                    return Err("该账户不是百炼账户。".to_string());
                }
                if account.source != AccountSource::Managed {
                    return Err("该百炼账户不能重新授权。".to_string());
                }
                (id, false)
            }
            None => (new_account_id("bailian"), true),
        };

        {
            // 同一时间只允许一个授权流程，避免多个回环监听同时等待。
            let logins = lock(&self.logins);
            if let Some(pending) = active_login(&logins) {
                if pending.account_id == target_id {
                    return Err("该账户正在等待授权，请先完成或取消当前登录。".to_string());
                }
                return Err("已有其他账户的授权流程进行中，请先完成或取消。".to_string());
            }
        }

        let profile = self
            .private_bailian_dir(&target_id)
            .ok_or_else(|| "账户标识无效".to_string())?;
        ensure_private_dir(&profile)?;

        let session = bailian::begin_console_login().map_err(|error| error.message)?;
        let auth_url = session.auth_url.clone();
        let login_id = session.login_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&self.logins).insert(
            login_id.clone(),
            PendingLogin {
                account_id: target_id.clone(),
                cancel: Arc::clone(&cancel),
            },
        );

        let store = Arc::clone(self);
        let thread_account_id = target_id.clone();
        let thread_login_id = login_id.clone();
        let requested_name = display_name.filter(|name| !name.trim().is_empty());
        thread::spawn(move || {
            let outcome = session.wait_for_token(&cancel, LOGIN_TIMEOUT);
            lock(&store.logins).remove(&thread_login_id);
            match outcome {
                Ok(token) => match bailian::save_credentials(&profile, &token) {
                    Ok(()) => {
                        store.finish_bailian_login(&thread_account_id, requested_name.as_deref());
                        store.emit_login_event(
                            &thread_login_id,
                            &thread_account_id,
                            "succeeded",
                            None,
                        );
                        let _ = store.request_refresh(Some(&thread_account_id));
                    }
                    Err(message) => {
                        if is_new {
                            let _ = fs::remove_dir_all(&profile);
                        }
                        store.emit_login_event(
                            &thread_login_id,
                            &thread_account_id,
                            "failed",
                            Some(message),
                        );
                    }
                },
                Err(error) => {
                    if is_new {
                        let _ = fs::remove_dir_all(&profile);
                    }
                    let status = if error.code == "canceled" {
                        "canceled"
                    } else {
                        "failed"
                    };
                    store.emit_login_event(
                        &thread_login_id,
                        &thread_account_id,
                        status,
                        Some(error.message),
                    );
                }
            }
        });

        Ok(LoginStartPayload {
            login_id,
            account_id: target_id,
            auth_url,
        })
    }

    fn finish_bailian_login(&self, account_id: &str, requested_name: Option<&str>) {
        let requested_name = requested_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        self.update_soft(|state| {
            if let Some(account) = account_mut(state, account_id) {
                account.auth_state = AuthState::Unknown;
                account.source = AccountSource::Managed;
                if let Some(name) = &requested_name {
                    account.display_name = name.clone();
                }
                // 兼容旧状态：确保两类产品都存在，不覆盖已有快照。
                for product in bailian_products() {
                    if !account
                        .products
                        .iter()
                        .any(|existing| existing.product_id == product.product_id)
                    {
                        account.products.push(product);
                    }
                }
                return;
            }
            state.accounts.push(AccountState {
                account_id: account_id.to_string(),
                platform: "bailian".to_string(),
                display_name: requested_name.unwrap_or_else(|| "百炼账户".to_string()),
                auth_state: AuthState::Unknown,
                source: AccountSource::Managed,
                identity_hint: None,
                products: bailian_products(),
            });
        });
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

    /// 移除账户：只清理本应用目录，不触碰外部工具的登录状态。
    /// 本机 Codex 账户跟随 CLI 登录态，始终不可移除。
    pub fn remove_account(&self, account_id: &str) -> Result<PersistedState, String> {
        let state = self.snapshot();
        let account = state
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .ok_or_else(|| format!("未知账户：{account_id}"))?;
        if account.platform == "codex" && account.source == AccountSource::Local {
            return Err("本机 Codex 账户不能移除；它跟随本机 CLI 登录态。".to_string());
        }
        let profile = match account.platform.as_str() {
            "codex" => Some(
                self.private_profile_dir(account_id)
                    .ok_or_else(|| "账户标识无效".to_string())?,
            ),
            "bailian" => Some(
                self.private_bailian_dir(account_id)
                    .ok_or_else(|| "账户标识无效".to_string())?,
            ),
            OPENCODE_GO_PLATFORM | COMMAND_CODE_PLATFORM => Some(
                self.private_api_dir(account_id)
                    .ok_or_else(|| "账户标识无效".to_string())?,
            ),
            ANTIGRAVITY_PLATFORM => None,
            _ => return Err("未知账户平台".to_string()),
        };
        {
            let logins = lock(&self.logins);
            if logins
                .values()
                .any(|pending| pending.account_id == account_id)
            {
                return Err("该账户正在等待授权，请先取消授权再移除。".to_string());
            }
        }

        if let Some(profile) = profile {
            if profile.exists() {
                fs::remove_dir_all(&profile)
                    .map_err(|err| format!("删除账户授权目录失败：{err}"))?;
            }
        }
        self.update_strict(|state| {
            state
                .accounts
                .retain(|account| account.account_id != account_id);
            clear_reminder_marks(state, account_id);
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

    fn account_platform(&self, account_id: &str) -> Option<String> {
        self.snapshot()
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .map(|account| account.platform.clone())
    }

    fn bailian_profile_dir_for(&self, account_id: &str) -> Option<PathBuf> {
        let state = self.snapshot();
        let account = state
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)?;
        if account.platform != "bailian" || account.source != AccountSource::Managed {
            return None;
        }
        self.private_bailian_dir(account_id)
    }

    /// 手动刷新入口。`account_id` 为空时刷新全部已接入账户；
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
                .filter(|account| is_supported_platform(&account.platform))
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
        match store.account_platform(&account_id).as_deref() {
            Some("codex") => refresh_codex(&store, &account_id),
            Some("bailian") => refresh_bailian(&store, &account_id),
            Some(OPENCODE_GO_PLATFORM) => {
                refresh_api_key(&store, &account_id, OPENCODE_GO_PLATFORM)
            }
            Some(COMMAND_CODE_PLATFORM) => {
                refresh_api_key(&store, &account_id, COMMAND_CODE_PLATFORM)
            }
            Some(ANTIGRAVITY_PLATFORM) => refresh_antigravity(&store, &account_id),
            // 账户在排队期间被移除：不发起查询，也不回写旧任务。
            _ => {
                store.update_soft(|state| clear_refreshing(state, &account_id));
            }
        }
        lock(&store.in_flight).remove(&account_id);
    });
}

fn refresh_codex(store: &Arc<StateStore>, account_id: &str) {
    let Some(home) = store.profile_home_for(account_id) else {
        store.update_soft(|state| clear_refreshing(state, account_id));
        return;
    };
    let result = codex::read_usage(home.as_deref());
    // 重新授权期间旧凭证的查询结果可能晚到；丢弃它，避免覆盖授权后的新状态。
    if store.has_pending_login(account_id) {
        store.update_soft(|state| clear_refreshing(state, account_id));
        return;
    }
    let finished_at = epoch_now();
    match result {
        Ok(readout) => {
            let identity_update = readout.identity_checked.then_some(readout.identity);
            match readout.usage {
                Ok(usage) => {
                    // 提醒判断与快照写入放在同一次状态更新里，避免并发刷新
                    // 各自看到旧去重记录而重复提醒；重置判断需要旧快照作为基线，
                    // 因此必须在覆盖快照前取一次。
                    let mut low_notices = Vec::new();
                    let mut reset_notices = Vec::new();
                    store.update_soft(|state| {
                        let previous_resets = window_resets(state, account_id);
                        apply_success(state, account_id, usage, identity_update);
                        low_notices = low_quota_notices(state, account_id);
                        reset_notices = collect_reset_notices(state, account_id, &previous_resets);
                    });
                    for notice in &low_notices {
                        store.notify_low_quota(notice);
                    }
                    for notice in &reset_notices {
                        store.notify_reset(notice);
                    }
                }
                Err(error) => {
                    store.apply_refresh_error(account_id, error, finished_at, identity_update);
                }
            }
        }
        Err(error) => {
            store.apply_refresh_error(account_id, error, finished_at, None);
        }
    }
}

fn refresh_bailian(store: &Arc<StateStore>, account_id: &str) {
    let Some(profile) = store.bailian_profile_dir_for(account_id) else {
        store.update_soft(|state| clear_refreshing(state, account_id));
        return;
    };
    let credentials = bailian::load_credentials(&profile);
    if store.has_pending_login(account_id) {
        store.update_soft(|state| clear_refreshing(state, account_id));
        return;
    }
    let finished_at = epoch_now();
    let credentials = match credentials {
        Ok(credentials) => credentials,
        Err(error) => {
            store.apply_bailian_refresh_error(account_id, &error, finished_at);
            return;
        }
    };

    let readout = bailian::read_usage(&credentials.access_token);
    // 重新授权期间旧凭证的查询结果可能晚到；丢弃它。
    if store.has_pending_login(account_id) {
        store.update_soft(|state| clear_refreshing(state, account_id));
        return;
    }

    let mut low_notices = Vec::new();
    let mut reset_notices = Vec::new();
    store.update_soft(|state| {
        let previous_resets = window_resets(state, account_id);
        apply_bailian_readout(state, account_id, &readout, finished_at);
        low_notices = low_quota_notices(state, account_id);
        reset_notices = collect_reset_notices(state, account_id, &previous_resets);
    });
    for notice in &low_notices {
        store.notify_low_quota(notice);
    }
    for notice in &reset_notices {
        store.notify_reset(notice);
    }
}

fn probe_api_key(
    platform: &str,
    api_key: &str,
) -> Result<(QuotaSnapshot, Option<String>, Option<bool>), ProviderError> {
    match platform {
        OPENCODE_GO_PLATFORM => {
            opencode::read_usage(api_key).map(|usage| (opencode_snapshot(&usage), None, Some(true)))
        }
        COMMAND_CODE_PLATFORM => commandcode::read_usage(api_key).map(|usage| {
            let plan_tier = usage.plan_tier.clone();
            (commandcode_snapshot(&usage), plan_tier, Some(true))
        }),
        _ => Err(ProviderError::new(
            "unsupported",
            "该平台不支持 API Key 查询。",
        )),
    }
}

fn refresh_api_key(store: &Arc<StateStore>, account_id: &str, platform: &str) {
    let source = store
        .snapshot()
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
        .map(|account| account.source);
    let api_key = if source == Some(AccountSource::Local) && platform == OPENCODE_GO_PLATFORM {
        match opencode::read_cli_api_key() {
            Ok(api_key) => api_key,
            Err(error) => {
                store.apply_platform_refresh_error(account_id, platform, &error);
                return;
            }
        }
    } else {
        let Some(profile) = store.private_api_dir(account_id) else {
            store.update_soft(|state| clear_refreshing(state, account_id));
            return;
        };
        match api_key::load(&profile) {
            Ok(credential) if credential.platform == platform => credential.api_key,
            _ => {
                store.apply_platform_refresh_error(
                    account_id,
                    platform,
                    &ProviderError::new(
                        "credentials-missing",
                        "该账户的 API Key 已丢失，请重新填写。",
                    ),
                );
                return;
            }
        }
    };

    match probe_api_key(platform, &api_key) {
        Ok((snapshot, plan_tier, active)) => {
            let mut low_notices = Vec::new();
            let mut reset_notices = Vec::new();
            store.update_soft(|state| {
                let previous_resets = window_resets(state, account_id);
                apply_platform_success(
                    state, account_id, platform, snapshot, plan_tier, None, active,
                );
                low_notices = low_quota_notices(state, account_id);
                reset_notices = collect_reset_notices(state, account_id, &previous_resets);
            });
            for notice in &low_notices {
                store.notify_low_quota(notice);
            }
            for notice in &reset_notices {
                store.notify_reset(notice);
            }
        }
        Err(error) => store.apply_platform_refresh_error(account_id, platform, &error),
    }
}

fn refresh_antigravity(store: &Arc<StateStore>, account_id: &str) {
    match antigravity::read_usage() {
        Ok(usage) => {
            let snapshot = antigravity_snapshot(&usage);
            let plan_tier = usage.plan_tier.clone();
            let identity_hint = usage
                .account
                .as_deref()
                .map(mask_email)
                .filter(|hint| hint != "已授权账户");
            let mut low_notices = Vec::new();
            let mut reset_notices = Vec::new();
            store.update_soft(|state| {
                let previous_resets = window_resets(state, account_id);
                apply_platform_success(
                    state,
                    account_id,
                    ANTIGRAVITY_PLATFORM,
                    snapshot,
                    plan_tier,
                    identity_hint,
                    Some(true),
                );
                low_notices = low_quota_notices(state, account_id);
                reset_notices = collect_reset_notices(state, account_id, &previous_resets);
            });
            for notice in &low_notices {
                store.notify_low_quota(notice);
            }
            for notice in &reset_notices {
                store.notify_reset(notice);
            }
        }
        Err(error) => store.apply_platform_refresh_error(account_id, ANTIGRAVITY_PLATFORM, &error),
    }
}

pub fn auto_detect_local_accounts(store: Arc<StateStore>) {
    // 启动后在后台探测，避免阻塞窗口；Antigravity 首次查询可能耗时十余秒。
    thread::spawn(move || {
        store.detect_local_opencode();
        store.detect_local_antigravity();
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

    fn usage_with(remaining_percent: i64, resets_at_epoch: Option<i64>) -> CodexUsage {
        usage_at(remaining_percent, resets_at_epoch, 1_790_000_000)
    }

    fn usage_at(
        remaining_percent: i64,
        resets_at_epoch: Option<i64>,
        checked_at_epoch: i64,
    ) -> CodexUsage {
        CodexUsage {
            checked_at_epoch,
            buckets: vec![CodexLimitBucket {
                limit_id: None,
                limit_name: None,
                plan_type: None,
                windows: vec![CodexWindow {
                    role: "primary".to_string(),
                    used_percent: 100 - remaining_percent,
                    remaining_percent,
                    window_duration_mins: Some(300),
                    resets_at_epoch,
                }],
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
    fn low_quota_notifies_once_per_reset_cycle_across_restarts() {
        let mut state = default_state();
        state.preferences.low_quota_threshold = 20;

        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(10, Some(1_790_010_000)),
            None,
        );
        let notices = low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].remaining_percent, 10);
        assert_eq!(notices[0].threshold, 20);
        assert_eq!(notices[0].window_label, "5 小时窗口");

        // 同一重置周期内继续下跌不重复提醒。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(8, Some(1_790_010_000)),
            None,
        );
        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());

        // 去重记录随状态持久化：模拟应用重启后仍不重复。
        let mut reloaded: PersistedState =
            serde_json::from_str(&serde_json::to_string(&state).expect("serialize"))
                .expect("deserialize");
        assert!(low_quota_notices(&mut reloaded, DEFAULT_ACCOUNT_ID).is_empty());

        // 进入新的重置周期后可以再次提醒。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(10, Some(1_790_020_000)),
            None,
        );
        assert_eq!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).len(), 1);
    }

    #[test]
    fn low_quota_stays_quiet_when_disabled_or_above_threshold() {
        let mut state = default_state();
        state.preferences.low_quota_enabled = false;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(10, Some(1_790_010_000)),
            None,
        );
        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());

        let mut state = default_state();
        state.preferences.low_quota_threshold = 20;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(21, Some(1_790_010_000)),
            None,
        );
        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());
    }

    #[test]
    fn low_quota_without_reset_time_rearms_after_recovery() {
        let mut state = default_state();
        state.preferences.low_quota_threshold = 20;

        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage_with(10, None), None);
        assert_eq!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).len(), 1);
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage_with(9, None), None);
        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());

        // 无法判断周期时，回到阈值以上视为恢复，下一次跌破再提醒。
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage_with(60, None), None);
        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());
        apply_success(&mut state, DEFAULT_ACCOUNT_ID, usage_with(10, None), None);
        assert_eq!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).len(), 1);
    }

    #[test]
    fn low_quota_skips_windows_without_remaining_percent() {
        let mut state = default_state();
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_with(10, Some(1_790_010_000)),
            None,
        );
        state.accounts[0].products[0]
            .snapshot
            .as_mut()
            .unwrap()
            .groups[0]
            .windows[0]
            .remaining_percent = None;

        assert!(low_quota_notices(&mut state, DEFAULT_ACCOUNT_ID).is_empty());
    }

    #[test]
    fn removing_an_account_clears_only_its_reminder_marks() {
        let mut state = default_state();
        let own = reminder_key(DEFAULT_ACCOUNT_ID, DEFAULT_PRODUCT_ID, "primary-0");
        let other = reminder_key("codex-other", DEFAULT_PRODUCT_ID, "primary-0");
        state.reminders.low_quota.insert(
            own.clone(),
            LowQuotaMark {
                resets_at_epoch: Some(1),
            },
        );
        state.reminders.low_quota.insert(
            other.clone(),
            LowQuotaMark {
                resets_at_epoch: Some(2),
            },
        );

        clear_reminder_marks(&mut state, DEFAULT_ACCOUNT_ID);

        assert!(!state.reminders.low_quota.contains_key(&own));
        assert!(state.reminders.low_quota.contains_key(&other));
    }

    #[test]
    fn auth_error_notifies_once_per_expiry_and_survives_restart() {
        let mut state = default_state();
        let was_reauth = state.accounts[0].auth_state == AuthState::NeedsReauth;
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
        let notice = auth_expired_notice(&state, DEFAULT_ACCOUNT_ID, was_reauth)
            .expect("first expiry should notify");
        assert_eq!(notice.account_name, "本机 Codex");
        assert_eq!(notice.source, AccountSource::Local);

        // 已经是失效状态时，重复失败不重复提醒。
        let was_reauth = state.accounts[0].auth_state == AuthState::NeedsReauth;
        apply_error(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "auth-expired".to_string(),
                message: "需要重新登录".to_string(),
            },
            1_790_100_060,
            None,
        );
        assert!(auth_expired_notice(&state, DEFAULT_ACCOUNT_ID, was_reauth).is_none());

        // 去重依据是持久化的 authState：模拟重启后继续失败也不重复。
        let mut reloaded: PersistedState =
            serde_json::from_str(&serde_json::to_string(&state).expect("serialize"))
                .expect("deserialize");
        let was_reauth = reloaded.accounts[0].auth_state == AuthState::NeedsReauth;
        apply_error(
            &mut reloaded,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "auth-expired".to_string(),
                message: "需要重新登录".to_string(),
            },
            1_790_100_120,
            None,
        );
        assert!(auth_expired_notice(&reloaded, DEFAULT_ACCOUNT_ID, was_reauth).is_none());
    }

    #[test]
    fn network_error_never_triggers_auth_notice() {
        let mut state = default_state();
        let was_reauth = state.accounts[0].auth_state == AuthState::NeedsReauth;
        apply_error(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            UsageError {
                code: "rpc".to_string(),
                message: "网络错误".to_string(),
            },
            1_790_100_000,
            None,
        );

        assert_eq!(state.accounts[0].auth_state, AuthState::Unknown);
        assert!(auth_expired_notice(&state, DEFAULT_ACCOUNT_ID, was_reauth).is_none());
    }

    #[test]
    fn auth_notice_respects_the_disabled_switch() {
        let mut state = default_state();
        state.preferences.auth_expired_enabled = false;
        let was_reauth = state.accounts[0].auth_state == AuthState::NeedsReauth;
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

        // 开关关闭只影响提醒，不掩盖真实的授权状态。
        assert_eq!(state.accounts[0].auth_state, AuthState::NeedsReauth);
        assert!(auth_expired_notice(&state, DEFAULT_ACCOUNT_ID, was_reauth).is_none());
    }

    #[test]
    fn reauthorization_rearms_auth_notice_for_managed_account() {
        let account_id = "codex-synthetic-1";
        let mut state = default_state();
        apply_login_success(
            &mut state,
            account_id,
            None,
            None,
            None,
            None,
            None,
            1_790_000_000,
        );

        let was_reauth = state.accounts[1].auth_state == AuthState::NeedsReauth;
        apply_error(
            &mut state,
            account_id,
            UsageError {
                code: "auth-expired".to_string(),
                message: "需要重新登录".to_string(),
            },
            1_790_100_000,
            None,
        );
        let notice = auth_expired_notice(&state, account_id, was_reauth).expect("notice");
        assert_eq!(notice.source, AccountSource::Managed);

        // 重新授权成功回到 Ok，下一次失效重新提醒。
        apply_login_success(
            &mut state,
            account_id,
            None,
            None,
            None,
            None,
            None,
            1_790_200_000,
        );
        assert_eq!(state.accounts[1].auth_state, AuthState::Ok);
        let was_reauth = state.accounts[1].auth_state == AuthState::NeedsReauth;
        apply_error(
            &mut state,
            account_id,
            UsageError {
                code: "auth-expired".to_string(),
                message: "需要重新登录".to_string(),
            },
            1_790_300_000,
            None,
        );
        assert!(auth_expired_notice(&state, account_id, was_reauth).is_some());
    }

    #[test]
    fn reset_notifies_only_when_query_confirms_the_new_cycle() {
        let mut state = default_state();
        state.preferences.reset_notice_enabled = true;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(96, Some(1_790_010_000), 1_790_000_000),
            None,
        );
        let previous = window_resets(&state, DEFAULT_ACCOUNT_ID);

        // 旧周期已到期，本次查询返回更晚的重置时间：确认进入新周期。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(100, Some(1_790_020_000), 1_790_015_000),
            None,
        );
        let notices = collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].remaining_percent, Some(100));
        assert_eq!(notices[0].window_label, "5 小时窗口");

        // 同一新周期内的后续刷新不再提醒。
        let previous = window_resets(&state, DEFAULT_ACCOUNT_ID);
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(99, Some(1_790_020_000), 1_790_021_000),
            None,
        );
        assert!(collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).is_empty());
    }

    #[test]
    fn reset_ignores_reset_time_drift_before_the_cycle_ends() {
        let mut state = default_state();
        state.preferences.reset_notice_enabled = true;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(96, Some(1_790_030_000), 1_790_000_000),
            None,
        );
        let previous = window_resets(&state, DEFAULT_ACCOUNT_ID);

        // 旧周期尚未结束，平台把重置时间往后挪：不能当成已重置。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(96, Some(1_790_040_000), 1_790_010_000),
            None,
        );
        assert!(collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).is_empty());
    }

    #[test]
    fn reset_is_silent_without_switch_or_baseline() {
        let mut state = default_state();
        state.preferences.reset_notice_enabled = false;
        let previous = HashMap::new();
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(100, Some(1_790_020_000), 1_790_015_000),
            None,
        );
        assert!(collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).is_empty());

        // 打开开关但没有上一份快照：无法确认是“新周期”，不提醒。
        state.preferences.reset_notice_enabled = true;
        assert!(collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).is_empty());
    }

    #[test]
    fn reset_skips_windows_without_reset_time() {
        let mut state = default_state();
        state.preferences.reset_notice_enabled = true;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(96, Some(1_790_010_000), 1_790_000_000),
            None,
        );
        let previous = window_resets(&state, DEFAULT_ACCOUNT_ID);

        // 新结果缺少重置时间：无法确认新周期，不提醒也不用倒计时推断。
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(100, None, 1_790_015_000),
            None,
        );
        assert!(collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).is_empty());
    }

    #[test]
    fn reset_is_not_repeated_after_restart() {
        let mut state = default_state();
        state.preferences.reset_notice_enabled = true;
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(96, Some(1_790_010_000), 1_790_000_000),
            None,
        );
        let previous = window_resets(&state, DEFAULT_ACCOUNT_ID);
        apply_success(
            &mut state,
            DEFAULT_ACCOUNT_ID,
            usage_at(100, Some(1_790_020_000), 1_790_015_000),
            None,
        );
        assert_eq!(
            collect_reset_notices(&state, DEFAULT_ACCOUNT_ID, &previous).len(),
            1
        );

        // 新快照已持久化；模拟重启后同一周期继续刷新不会重复提醒。
        let mut reloaded: PersistedState =
            serde_json::from_str(&serde_json::to_string(&state).expect("serialize"))
                .expect("deserialize");
        let previous = window_resets(&reloaded, DEFAULT_ACCOUNT_ID);
        apply_success(
            &mut reloaded,
            DEFAULT_ACCOUNT_ID,
            usage_at(98, Some(1_790_020_000), 1_790_030_000),
            None,
        );
        assert!(collect_reset_notices(&reloaded, DEFAULT_ACCOUNT_ID, &previous).is_empty());
    }

    #[test]
    fn normal_preference_saves_do_not_change_auto_launch() {
        let current = Preferences {
            auto_launch_enabled: true,
            ..Preferences::default()
        };
        let incoming = Preferences {
            auto_launch_enabled: false,
            refresh_minutes: 15,
            ..Preferences::default()
        };

        let next = preferences_for_update(&current, incoming);

        // 自启只由系统登录项状态决定，普通设置保存不能把它覆盖掉。
        assert!(next.auto_launch_enabled);
        assert_eq!(next.refresh_minutes, 15);
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

        prefs.low_quota_threshold = 20;
        prefs.proxy_mode = ProxyMode::Manual;
        assert!(prefs.validate().is_err());
        prefs.proxy_url = Some("127.0.0.1:7890".to_string());
        assert!(prefs.validate().is_err());
        prefs.proxy_url = Some("http://127.0.0.1:7890".to_string());
        assert!(prefs.validate().is_ok());
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
        state.reminders.low_quota.insert(
            reminder_key(DEFAULT_ACCOUNT_ID, DEFAULT_PRODUCT_ID, "primary-0"),
            LowQuotaMark {
                resets_at_epoch: Some(1_790_010_000),
            },
        );

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

    fn bailian_account(id: &str) -> AccountState {
        AccountState {
            account_id: id.to_string(),
            platform: "bailian".to_string(),
            display_name: "百炼测试账户".to_string(),
            auth_state: AuthState::Unknown,
            source: AccountSource::Managed,
            identity_hint: None,
            products: bailian_products(),
        }
    }

    fn bailian_readout() -> BailianReadout {
        BailianReadout {
            checked_at_epoch: 1_790_000_000,
            token_plan: Ok(TokenPlanUsage {
                used_percent: Some(5),
                resets_at_epoch: Some(1_795_363_200),
            }),
            coding_plan: Ok(CodingPlanUsage {
                found: true,
                instance_type: Some("pro".to_string()),
                per_5_hour: CodingPlanWindow {
                    used: Some(0),
                    total: Some(6_000),
                    resets_at_epoch: Some(1_792_739_314),
                },
                per_week: CodingPlanWindow {
                    used: Some(1),
                    total: Some(45_000),
                    resets_at_epoch: Some(1_792_944_000),
                },
                per_bill_month: CodingPlanWindow {
                    used: Some(1),
                    total: Some(90_000),
                    resets_at_epoch: Some(1_794_412_800),
                },
            }),
            token_plan_subscription: Some(TokenPlanSubscription {
                active: true,
                spec_code: Some("standard".to_string()),
                status: Some("VALID".to_string()),
                remaining_days: Some(30),
                end_time_epoch: Some(1_795_363_200),
            }),
            identity: Some(crate::bailian::AccountIdentity {
                uid: Some("1679446554858477".to_string()),
                nick_name: Some("1679446554858477".to_string()),
            }),
        }
    }

    #[test]
    fn bailian_readout_populates_both_plan_products() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));

        apply_bailian_readout(&mut state, "bailian-1", &bailian_readout(), 1_790_000_100);

        let account = &state.accounts[1];
        assert_eq!(account.auth_state, AuthState::Ok);
        assert_eq!(account.identity_hint.as_deref(), Some("UID ***8477"));
        assert_eq!(account.products.len(), 2);

        let token_plan = &account.products[0];
        assert_eq!(token_plan.active, Some(true));
        assert_eq!(token_plan.plan_tier.as_deref(), Some("standard"));
        let token_window = &token_plan.snapshot.as_ref().unwrap().groups[0].windows[0];
        assert_eq!(token_window.unit, "percent");
        assert_eq!(token_window.used_percent, Some(5));
        assert_eq!(token_window.remaining_percent, Some(95));
        assert_eq!(token_window.window_name.as_deref(), Some("订阅月"));

        let coding_plan = &account.products[1];
        assert_eq!(coding_plan.active, Some(true));
        assert_eq!(coding_plan.plan_tier.as_deref(), Some("pro"));
        let coding_windows = &coding_plan.snapshot.as_ref().unwrap().groups[0].windows;
        assert_eq!(coding_windows.len(), 3);
        assert_eq!(coding_windows[1].unit, "calls");
        assert_eq!(coding_windows[1].used_amount, Some(1.0));
        assert_eq!(coding_windows[1].total_amount, Some(45_000.0));
        assert_eq!(coding_windows[1].window_duration_mins, Some(10_080));
        assert!(coding_windows[1].remaining_percent.is_some());
    }

    #[test]
    fn bailian_default_account_name_is_replaced_by_a_real_nickname() {
        let mut state = default_state();
        let mut account = bailian_account("bailian-1");
        account.display_name = "百炼账户".to_string();
        state.accounts.push(account);
        let mut readout = bailian_readout();
        readout.identity = Some(crate::bailian::AccountIdentity {
            uid: Some("1679446554858477".to_string()),
            nick_name: Some("rainbow217".to_string()),
        });

        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_100);

        assert_eq!(state.accounts[1].display_name, "rainbow217");
    }

    #[test]
    fn bailian_nickname_is_preferred_over_masked_uid() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));
        let mut readout = bailian_readout();
        readout.identity = Some(crate::bailian::AccountIdentity {
            uid: Some("1679446554858477".to_string()),
            nick_name: Some("rainbow217".to_string()),
        });

        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_100);

        assert_eq!(
            state.accounts[1].identity_hint.as_deref(),
            Some("rainbow217")
        );
    }

    #[test]
    fn bailian_inactive_token_plan_subscription_is_a_capability_gap() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));
        let mut readout = bailian_readout();
        readout.token_plan_subscription = Some(TokenPlanSubscription {
            active: false,
            status: Some("EXPIRED".to_string()),
            ..TokenPlanSubscription::default()
        });

        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_100);

        let token_plan = &state.accounts[1].products[0];
        assert_eq!(token_plan.active, Some(false));
        assert_eq!(token_plan.plan_tier, None);
        assert!(token_plan
            .snapshot
            .as_ref()
            .unwrap()
            .capability_gaps
            .contains(&"未找到有效的 Token Plan 订阅".to_string()));
    }

    #[test]
    fn bailian_auth_error_marks_account_but_keeps_last_snapshot() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));
        apply_bailian_readout(&mut state, "bailian-1", &bailian_readout(), 1_790_000_100);

        let mut readout = bailian_readout();
        readout.token_plan = Err(BailianError {
            code: "auth-expired".to_string(),
            message: "会话已失效".to_string(),
        });
        readout.coding_plan = Err(BailianError {
            code: "auth-expired".to_string(),
            message: "会话已失效".to_string(),
        });
        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_200);

        let account = &state.accounts[1];
        assert_eq!(account.auth_state, AuthState::NeedsReauth);
        assert!(account.products[0].snapshot.is_some());
        assert_eq!(
            account.products[0].query.last_error.as_ref().unwrap().code,
            "auth-expired"
        );
    }

    #[test]
    fn bailian_one_successful_plan_keeps_the_account_authorized() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));
        let mut readout = bailian_readout();
        readout.coding_plan = Err(BailianError {
            code: "auth-expired".to_string(),
            message: "该类套餐不可读".to_string(),
        });

        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_100);

        let account = &state.accounts[1];
        assert_eq!(account.auth_state, AuthState::Ok);
        assert!(account.products[0].snapshot.is_some());
        assert!(account.products[1].query.last_error.is_some());
    }

    #[test]
    fn bailian_account_without_snapshot_is_due_immediately() {
        let account = bailian_account("bailian-1");
        assert!(is_account_due(&account, 5, 1_790_000_000));

        let mut state = default_state();
        state.accounts.push(account);
        apply_bailian_readout(&mut state, "bailian-1", &bailian_readout(), 1_790_000_000);
        let account = &state.accounts[1];
        assert!(!is_account_due(account, 5, 1_790_000_100));
        assert!(is_account_due(account, 5, 1_790_000_400));
    }

    #[test]
    fn bailian_coding_plan_without_subscription_is_a_capability_gap_not_an_error() {
        let mut state = default_state();
        state.accounts.push(bailian_account("bailian-1"));
        let mut readout = bailian_readout();
        readout.coding_plan = Ok(CodingPlanUsage::default());

        apply_bailian_readout(&mut state, "bailian-1", &readout, 1_790_000_100);

        let coding_plan = &state.accounts[1].products[1];
        assert_eq!(coding_plan.active, Some(false));
        assert!(coding_plan.query.last_error.is_none());
        let snapshot = coding_plan.snapshot.as_ref().unwrap();
        assert!(snapshot.groups.is_empty());
        assert_eq!(
            snapshot.capability_gaps,
            vec!["未找到有效的 Coding Plan 订阅"]
        );
        assert_eq!(state.accounts[1].auth_state, AuthState::Ok);
    }

    #[test]
    fn canceled_login_releases_the_authorization_slot_immediately() {
        let mut logins = HashMap::new();
        let cancel = Arc::new(AtomicBool::new(false));
        logins.insert(
            "login-1".to_string(),
            PendingLogin {
                account_id: "codex-1".to_string(),
                cancel: Arc::clone(&cancel),
            },
        );
        assert_eq!(
            active_login(&logins).map(|pending| pending.account_id.as_str()),
            Some("codex-1")
        );

        // 取消后即使后台线程还没回收回调端口，也不该再阻止新的授权。
        cancel.store(true, Ordering::SeqCst);
        assert!(active_login(&logins).is_none());
    }

    #[test]
    fn antigravity_snapshot_keeps_model_groups_and_windows() {
        // 结构与 `agy -p /usage --output-format json` 实际返回一致。
        let usage = AntigravityUsage {
            checked_at_epoch: 1_792_759_583,
            account: None,
            plan_tier: None,
            groups: vec![
                antigravity::AntigravityGroup {
                    name: "Gemini Models".to_string(),
                    windows: vec![antigravity::AntigravityWindow {
                        source_id: "gemini-weekly".to_string(),
                        name: "Weekly Limit Remaining".to_string(),
                        window_duration_mins: Some(10_080),
                        remaining_percent: Some(0),
                        resets_at_epoch: Some(1_792_888_105),
                    }],
                },
                antigravity::AntigravityGroup {
                    name: "Claude and GPT models".to_string(),
                    windows: vec![antigravity::AntigravityWindow {
                        source_id: "3p-5h".to_string(),
                        name: "Five Hour Limit Remaining".to_string(),
                        window_duration_mins: Some(300),
                        remaining_percent: Some(50),
                        resets_at_epoch: Some(1_792_777_576),
                    }],
                },
            ],
        };

        let snapshot = antigravity_snapshot(&usage);
        assert_eq!(snapshot.groups.len(), 2);
        assert_eq!(
            snapshot.groups[0].limit_name.as_deref(),
            Some("Gemini Models")
        );
        let window = &snapshot.groups[0].windows[0];
        assert_eq!(window.remaining_percent, Some(0));
        assert_eq!(window.used_percent, Some(100));
        assert_eq!(window.window_duration_mins, Some(10_080));
        assert_eq!(window.resets_at_epoch, Some(1_792_888_105));
        assert_eq!(snapshot.groups[1].windows[0].remaining_percent, Some(50));
        assert_eq!(snapshot.groups[1].windows[0].used_percent, Some(50));
    }
}
