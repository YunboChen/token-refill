import { invoke } from '@tauri-apps/api/core';
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';

export type AppChannel = 'pro' | 'test';

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
  /** pro：正式版；test：独立标识的测试版。 */
  channel: AppChannel;
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('get_app_info');
}

// ---------- 与 Rust state.rs 对应的脱敏状态 ----------

export type AuthState = 'unknown' | 'ok' | 'needs-reauth';
/** local：本机 Codex 登录态；managed：应用内独立授权账户。 */
export type AccountSource = 'local' | 'managed';

export interface Preferences {
  refreshMinutes: number;
  lowQuotaEnabled: boolean;
  lowQuotaThreshold: number;
  authExpiredEnabled: boolean;
  resetNoticeEnabled: boolean;
  autoLaunchEnabled: boolean;
  /** system：跟随系统；none：直连；manual：使用 proxyUrl。 */
  proxyMode: ProxyMode;
  proxyUrl: string | null;
}

export type ProxyMode = 'system' | 'none' | 'manual';

export interface QueryError {
  code: string;
  message: string;
}

export interface QueryState {
  refreshing: boolean;
  lastAttemptEpoch: number | null;
  lastError: QueryError | null;
}

export interface QuotaWindow {
  windowId: string;
  sourceWindowId: string;
  /** 平台侧窗口名（如“订阅月”）；为空时按持续时间展示。 */
  windowName: string | null;
  /** 原始计量单位，例如 percent / calls。 */
  unit: string;
  usedPercent: number | null;
  remainingPercent: number | null;
  /** 平台提供的绝对值；没有时只展示比例。 */
  usedAmount: number | null;
  totalAmount: number | null;
  /** 只有剩余余额时使用。 */
  remainingAmount: number | null;
  windowDurationMins: number | null;
  resetsAtEpoch: number | null;
}

export interface QuotaGroup {
  limitId: string | null;
  limitName: string | null;
  planType: string | null;
  windows: QuotaWindow[];
}

export interface QuotaSnapshot {
  checkedAtEpoch: number;
  groups: QuotaGroup[];
  capabilityGaps: string[];
}

export interface ProductState {
  productId: string;
  kind: string;
  displayName: string;
  planTier: string | null;
  /** null 表示尚未确认；false 表示平台确认未开通。 */
  active: boolean | null;
  snapshot: QuotaSnapshot | null;
  query: QueryState;
}

export interface AccountState {
  accountId: string;
  platform: string;
  displayName: string;
  authState: AuthState;
  source: AccountSource;
  identityHint: string | null;
  products: ProductState[];
}

export interface LoginStart {
  loginId: string;
  accountId: string;
  authUrl: string;
}

export type LoginStatus = 'succeeded' | 'failed' | 'canceled';

export interface AccountLoginEvent {
  loginId: string;
  accountId: string;
  status: LoginStatus;
  message: string | null;
}

/** 总览的展示顺序：平台键与账户标识，未列出的保持默认顺序。 */
export interface OverviewOrder {
  platforms: string[];
  accounts: string[];
}

export interface AppState {
  schemaVersion: number;
  revision: number;
  accounts: AccountState[];
  preferences: Preferences;
  overviewOrder: OverviewOrder;
  /** 中转站账户的展示顺序；未列出的保持注册表顺序。 */
  relayOrder: string[];
}

export const APP_STATE_EVENT = 'app-state-changed';
export const ACCOUNT_LOGIN_EVENT = 'account-login-changed';
export const THEME_CHANGED_EVENT = 'theme-changed';

/** 主题是前端本地偏好；切换后广播给菜单栏面板。 */
export function emitThemeChanged(theme: string): Promise<void> {
  return emit(THEME_CHANGED_EVENT, theme);
}

export function onThemeChanged(
  handler: (theme: string) => void,
): Promise<UnlistenFn> {
  return listen<string>(THEME_CHANGED_EVENT, (event) => handler(event.payload));
}

export function getAppState(): Promise<AppState> {
  return invoke<AppState>('get_app_state');
}

/** 只提交刷新请求；查询在 Rust 后台执行，结果通过 app-state-changed 推送。 */
export function refreshUsage(accountId?: string): Promise<void> {
  return invoke<void>('refresh_usage', { accountId: accountId ?? null });
}

export function updatePreferences(preferences: Preferences): Promise<AppState> {
  return invoke<AppState>('update_preferences', { preferences });
}

/** 保存总览的账户类型与账户排序；只提交标识列表。 */
export function setOverviewOrder(
  platforms: string[],
  accounts: string[],
): Promise<AppState> {
  return invoke<AppState>('set_overview_order', { platforms, accounts });
}

/** 保存中转站账户的展示顺序；只提交注册表标识。 */
export function setRelayOrder(ids: string[]): Promise<AppState> {
  return invoke<AppState>('set_relay_order', { ids });
}

export function onAppStateChanged(
  handler: (state: AppState) => void,
): Promise<UnlistenFn> {
  return listen<AppState>(APP_STATE_EVENT, (event) => handler(event.payload));
}

/** 发起 Codex 官方浏览器授权；authUrl 由 app-server 返回，令牌不经过前端。 */
export function beginAccountLogin(
  accountId?: string,
  displayName?: string,
): Promise<LoginStart> {
  return invoke<LoginStart>('begin_account_login', {
    accountId: accountId ?? null,
    displayName: displayName ?? null,
  });
}

/** 发起百炼控制台浏览器授权；令牌通过本机回环地址直接交给 Rust。 */
export function beginBailianLogin(
  accountId?: string,
  displayName?: string,
): Promise<LoginStart> {
  return invoke<LoginStart>('begin_bailian_login', {
    accountId: accountId ?? null,
    displayName: displayName ?? null,
  });
}

export function cancelAccountLogin(loginId: string): Promise<void> {
  return invoke<void>('cancel_account_login', { loginId });
}

/** 用 API Key 接入订阅平台；密钥只传递给 Rust，不进入前端状态。 */
export function addApiKeyAccount(
  platform: string,
  displayName: string | undefined,
  apiKey: string,
): Promise<AppState> {
  return invoke<AppState>('add_api_key_account', {
    platform,
    displayName: displayName ?? null,
    apiKey,
  });
}

/** 验证通过后替换已有 API Key 账户的密钥。 */
export function replaceApiKey(accountId: string, apiKey: string): Promise<AppState> {
  return invoke<AppState>('replace_api_key', { accountId, apiKey });
}

/** 接入本机 Antigravity CLI 登录态。 */
export function addAntigravityAccount(displayName?: string): Promise<AppState> {
  return invoke<AppState>('add_antigravity_account', {
    displayName: displayName ?? null,
  });
}

export function renameAccount(accountId: string, displayName: string): Promise<AppState> {
  return invoke<AppState>('rename_account', { accountId, displayName });
}

export function removeAccount(accountId: string): Promise<AppState> {
  return invoke<AppState>('remove_account', { accountId });
}

/** 修改开机自启；由 Rust 写入系统登录项，成功后返回实际状态。 */
export function setAutoLaunch(enabled: boolean): Promise<AppState> {
  return invoke<AppState>('set_auto_launch', { enabled });
}

/** 设置页的显式测试入口；失败时返回可直接展示的原因（如通知权限被拒绝）。 */
export function sendTestNotification(): Promise<void> {
  return invoke<void>('send_test_notification');
}

/** 由 Rust 校验仅允许 HTTPS，并交给系统浏览器打开。 */
export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>('open_external_url', { url });
}

// ---------- 版本检查 ----------

export interface UpdateStatus {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  /** 当前安装形态是否支持应用内下载与替换；免安装 exe 为 false。 */
  selfUpdateSupported: boolean;
  releaseUrl: string | null;
  releaseNotes?: string;
  publishedAt: string | null;
}

export interface UpdateDownloadProgress {
  downloaded: number;
  total: number | null;
}

const UPDATE_PROGRESS_EVENT = 'update-download-progress';

/**
 * 查询 GitHub 最新正式版本。同一会话内复用首次结果，避免每次打开设置页
 * 都请求接口；失败时不缓存，下次可以重试。
 */
let pendingUpdateCheck: Promise<UpdateStatus> | null = null;

export function checkForUpdate(force = false): Promise<UpdateStatus> {
  if (force || !pendingUpdateCheck) {
    pendingUpdateCheck = invoke<UpdateStatus>('check_for_update').catch(
      (error: unknown) => {
        pendingUpdateCheck = null;
        throw error;
      },
    );
  }
  return pendingUpdateCheck;
}

/**
 * 下载更新包并校验签名。必须由用户点击触发；下载完成后还需要调用
 * installUpdate 才会替换应用。返回下载到的版本号。
 */
export function downloadUpdate(): Promise<string> {
  return invoke<string>('download_update');
}

/** 安装已下载的更新并重启（Windows 上由 NSIS 安装器接管）。 */
export function installUpdate(): Promise<void> {
  return invoke<void>('install_update');
}

/** 已下载待安装的版本号；设置页重新打开时恢复状态。 */
export function downloadedUpdateVersion(): Promise<string | null> {
  return invoke<string | null>('downloaded_update_version');
}

export function onUpdateDownloadProgress(
  handler: (progress: UpdateDownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<UpdateDownloadProgress>(UPDATE_PROGRESS_EVENT, (event) =>
    handler(event.payload),
  );
}

export function onAccountLoginChanged(
  handler: (event: AccountLoginEvent) => void,
): Promise<UnlistenFn> {
  return listen<AccountLoginEvent>(ACCOUNT_LOGIN_EVENT, (event) => handler(event.payload));
}

// ---------- 菜单栏快速查看 ----------

const TRAY_PANEL_SHOWN_EVENT = 'tray-panel-shown';

/** 面板每次从菜单栏打开时触发；用于补查中转站额度。 */
export function onTrayPanelShown(handler: () => void): Promise<UnlistenFn> {
  return listen(TRAY_PANEL_SHOWN_EVENT, () => handler());
}

/** 打开管理窗口；由 Rust 收起面板并恢复被隐藏的主窗口。 */
export function openMainWindow(): Promise<void> {
  return invoke<void>('open_main_window');
}

export function hideTrayPanel(): Promise<void> {
  return invoke<void>('hide_tray_panel');
}

/** 明确的退出入口：停止后台刷新并结束进程。 */
export function quitApp(): Promise<void> {
  return invoke<void>('quit_app');
}

// ---------- 中转站密钥额度（手动凭证由 Rust 保存在应用数据目录）----------

export type QuotaProvider = 'new-api' | 'sub2-api';
export type QuotaMode = 'key' | 'subscription' | 'wallet' | 'unrestricted';
export type AccountQuotaSource = 'key' | 'access-token';

export interface KeyQuotaWindow {
  windowId: string;
  durationMins: number | null;
  limitUsd: number | null;
  usedUsd: number | null;
  remainingUsd: number | null;
  remainingPercent: number | null;
  resetsAtEpoch: number | null;
}

export interface AccountQuota {
  totalUsd: number | null;
  usedUsd: number | null;
  remainingUsd: number | null;
  source: AccountQuotaSource;
  windows: KeyQuotaWindow[];
}

export interface KeyQuota {
  provider: QuotaProvider;
  mode: QuotaMode;
  keyName: string | null;
  planName: string | null;
  /** 金额单位，例如 USD / CNY；缺省按 USD 展示。 */
  currency: string | null;
  valid: boolean;
  unlimited: boolean;
  limitUsd: number | null;
  usedUsd: number | null;
  remainingUsd: number | null;
  account: AccountQuota | null;
  accountError: string | null;
  expiresAtEpoch: number | null;
  windows: KeyQuotaWindow[];
  checkedAtEpoch: number;
}

export type RelayOrigin = 'manual' | 'cc-switch' | 'claude-code' | 'codex';

export interface RelayAccount {
  id: string;
  baseUrl: string;
  /** 账户标题：用户手动设置的标题，或扫描导入时的 provider 名称；为空时显示服务地址。 */
  label: string | null;
  origin: RelayOrigin;
  provider: QuotaProvider;
  quota: KeyQuota | null;
  checkedAtEpoch: number | null;
  lastError: string | null;
  /** 密钥是否已保存到应用目录（仅手动填写的条目）。 */
  persisted: boolean;
  /** 是否已保存来源引用；下次启动会重新读取源配置。 */
  referenceSaved: boolean;
  /** 是否已配置 New API 访问令牌（不返回令牌本身）。 */
  hasAccountToken: boolean;
  /** 旧版 New API 单令牌鉴权使用的用户 ID（不是密钥）。 */
  newApiUserId: string | null;
}

export interface SkippedConfig {
  origin: RelayOrigin;
  baseUrl: string;
  reason: string;
}

export interface DiscoveryReport {
  accounts: RelayAccount[];
  skipped: SkippedConfig[];
}

/** 扫描结果中的一条候选；密钥留在 Rust 侧，前端只拿到展示信息。 */
export interface RelayCandidate {
  id: string;
  label: string | null;
  /** cc-switch 的 app_type（claude / codex）；当前配置文件条目为空。 */
  app: string | null;
  origin: RelayOrigin;
  baseUrl: string;
  alreadyAdded: boolean;
}

/** 扫描 cc-switch、Claude Code / Codex 的本地配置；只列候选，不发起网络请求。 */
export function scanRelayCandidates(): Promise<RelayCandidate[]> {
  return invoke<RelayCandidate[]>('relay_scan');
}

/** 导入勾选的候选：识别 New API / Sub2API 并查询额度。 */
export function importRelayCandidates(ids: string[]): Promise<DiscoveryReport> {
  return invoke<DiscoveryReport>('relay_import', { ids });
}

/** 自动识别 New API / Sub2API 并查询；手动凭证由 Rust 保存，不进入前端状态。 */
export function queryRelayAccount(
  baseUrl: string,
  apiKey: string,
  accountToken?: string,
  newApiUserId?: string,
  label?: string,
): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_query', {
    baseUrl,
    apiKey,
    accountToken: accountToken?.trim() || null,
    newApiUserId: newApiUserId?.trim() || null,
    label: label?.trim() || null,
  });
}

/** 为已有的 New API 账户补充访问令牌并刷新。 */
export function setRelayAccountToken(
  id: string,
  accountToken: string,
  newApiUserId?: string,
): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_set_account_token', {
    id,
    accountToken,
    newApiUserId: newApiUserId?.trim() || null,
  });
}

/** 修改服务地址与 API Key；`apiKey` 留空表示沿用当前密钥并立即重新查询。 */
export function updateRelayAccount(
  id: string,
  baseUrl: string,
  apiKey?: string,
): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_update', {
    id,
    baseUrl,
    apiKey: apiKey?.trim() || null,
  });
}

/** 设置中转站账户标题；空字符串表示恢复显示服务地址。 */
export function setRelayAccountLabel(
  id: string,
  label: string,
): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_set_label', { id, label });
}

export function refreshRelayAccount(id: string): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_refresh', { id });
}

/** 中转站条目列表 + 修订号；两个窗口共用同一份 Rust 注册表。 */
export interface RelayAccountsEvent {
  revision: number;
  accounts: RelayAccount[];
}

const RELAY_ACCOUNTS_EVENT = 'relay-accounts-changed';

/** 任意窗口变更中转站条目后广播，另一个窗口据此更新；按修订号丢弃旧快照。 */
export function onRelayAccountsChanged(
  handler: (event: RelayAccountsEvent) => void,
): Promise<UnlistenFn> {
  return listen<RelayAccountsEvent>(RELAY_ACCOUNTS_EVENT, (event) =>
    handler(event.payload),
  );
}

export function listRelayAccounts(): Promise<RelayAccountsEvent> {
  return invoke<RelayAccountsEvent>('relay_list');
}

export function removeRelayAccount(id: string): Promise<void> {
  return invoke<void>('relay_remove', { id });
}
