import { invoke } from '@tauri-apps/api/core';

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('get_app_info');
}

export interface CodexWindow {
  role: string;
  /** 协议口径：已用百分比。 */
  usedPercent: number;
  /** 派生展示值：100 - usedPercent，超限时为 0。 */
  remainingPercent: number;
  windowDurationMins: number | null;
  resetsAtEpoch: number | null;
}

export interface CodexLimitBucket {
  limitId: string | null;
  limitName: string | null;
  planType: string | null;
  windows: CodexWindow[];
}

export interface CodexUsage {
  checkedAtEpoch: number;
  buckets: CodexLimitBucket[];
}

export interface UsageError {
  code: string;
  message: string;
}

export function refreshCodexUsage(): Promise<CodexUsage> {
  return invoke<CodexUsage>('refresh_codex_usage');
}
