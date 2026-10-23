import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
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
}

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

export interface AppState {
  schemaVersion: number;
  revision: number;
  accounts: AccountState[];
  preferences: Preferences;
}

export const APP_STATE_EVENT = 'app-state-changed';
export const ACCOUNT_LOGIN_EVENT = 'account-login-changed';

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

/**
 * 新增百炼 Token Plan 团队版账户。AccessKey 只作为命令参数传给 Rust，
 * 不写入前端持久状态，也不进入事件或日志。
 */
export function addBailianTeamAccount(
  accessKeyId: string,
  accessKeySecret: string,
  displayName?: string,
): Promise<AppState> {
  return invoke<AppState>('add_bailian_team_account', {
    accessKeyId,
    accessKeySecret,
    displayName: displayName ?? null,
  });
}

/** 重新输入团队版 AccessKey；保存成功后由 Rust 原子替换旧凭证。 */
export function updateBailianTeamCredentials(
  accountId: string,
  accessKeyId: string,
  accessKeySecret: string,
): Promise<AppState> {
  return invoke<AppState>('update_bailian_team_credentials', {
    accountId,
    accessKeyId,
    accessKeySecret,
  });
}

export function cancelAccountLogin(loginId: string): Promise<void> {
  return invoke<void>('cancel_account_login', { loginId });
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

/** 设置页的显式测试入口；桌面端没有应用内通知授权弹窗。 */
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
  releaseUrl: string | null;
  releaseNotes?: string;
  publishedAt: string | null;
}

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

export function onAccountLoginChanged(
  handler: (event: AccountLoginEvent) => void,
): Promise<UnlistenFn> {
  return listen<AccountLoginEvent>(ACCOUNT_LOGIN_EVENT, (event) => handler(event.payload));
}

// ---------- 中转站密钥额度（密钥只在 Rust 会话内存中）----------

export type QuotaProvider = 'new-api' | 'sub2-api';
export type QuotaMode = 'key' | 'subscription' | 'wallet' | 'unrestricted';

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
  usedUsd: number | null;
  remainingUsd: number | null;
}

export interface KeyQuota {
  provider: QuotaProvider;
  mode: QuotaMode;
  keyName: string | null;
  planName: string | null;
  valid: boolean;
  unlimited: boolean;
  limitUsd: number | null;
  usedUsd: number | null;
  remainingUsd: number | null;
  account: AccountQuota | null;
  expiresAtEpoch: number | null;
  windows: KeyQuotaWindow[];
  checkedAtEpoch: number;
}

export type RelayOrigin = 'manual' | 'claude-code' | 'codex';

export interface RelayAccount {
  id: string;
  baseUrl: string;
  origin: RelayOrigin;
  provider: QuotaProvider;
  quota: KeyQuota | null;
  checkedAtEpoch: number | null;
  lastError: string | null;
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

/** 扫描 Claude Code / Codex 的本地配置并自动识别；密钥只留在 Rust 内存。 */
export function discoverRelayAccounts(): Promise<DiscoveryReport> {
  return invoke<DiscoveryReport>('relay_discover');
}

/** 自动识别 New API / Sub2API 并查询；密钥不写入前端状态或本地状态文件。 */
export function queryRelayAccount(baseUrl: string, apiKey: string): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_query', { baseUrl, apiKey });
}

export function refreshRelayAccount(id: string): Promise<RelayAccount> {
  return invoke<RelayAccount>('relay_refresh', { id });
}

export function listRelayAccounts(): Promise<RelayAccount[]> {
  return invoke<RelayAccount[]>('relay_list');
}

export function removeRelayAccount(id: string): Promise<void> {
  return invoke<void>('relay_remove', { id });
}
