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
  products: ProductState[];
}

export interface AppState {
  schemaVersion: number;
  revision: number;
  accounts: AccountState[];
  preferences: Preferences;
}

export const APP_STATE_EVENT = 'app-state-changed';

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
