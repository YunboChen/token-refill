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
  unit: string;
  usedPercent: number | null;
  remainingPercent: number | null;
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

/** 发起官方浏览器授权；authUrl 由 Codex app-server 返回，令牌不经过前端。 */
export function beginAccountLogin(
  accountId?: string,
  displayName?: string,
): Promise<LoginStart> {
  return invoke<LoginStart>('begin_account_login', {
    accountId: accountId ?? null,
    displayName: displayName ?? null,
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

export function onAccountLoginChanged(
  handler: (event: AccountLoginEvent) => void,
): Promise<UnlistenFn> {
  return listen<AccountLoginEvent>(ACCOUNT_LOGIN_EVENT, (event) => handler(event.payload));
}
