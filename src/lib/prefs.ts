import type { AppState, Preferences } from './desktop';

export type ThemePref = 'light' | 'dark' | 'system';

export const REFRESH_OPTIONS = [1, 5, 15, 30, 60];
export const THEME_OPTIONS: ThemePref[] = ['light', 'dark', 'system'];
export const THEME_LABELS: Record<ThemePref, string> = {
  light: '浅色',
  dark: '深色',
  system: '跟随系统',
};

// 与 Rust `Preferences::default` 保持一致：5 分钟刷新、低额度 20% 且默认开、
// 失效提醒默认开、重置提醒与自启默认关。
export const DEFAULT_PREFERENCES: Preferences = {
  refreshMinutes: 5,
  lowQuotaEnabled: true,
  lowQuotaThreshold: 20,
  authExpiredEnabled: true,
  resetNoticeEnabled: false,
  autoLaunchEnabled: false,
};

// 浏览器预览使用的占位状态；桌面端以 Rust 返回的完整状态为准。
export function defaultAppState(): AppState {
  return {
    schemaVersion: 1,
    revision: 0,
    accounts: [
      {
        accountId: 'codex-local',
        platform: 'codex',
        displayName: '本机 Codex',
        authState: 'unknown',
        source: 'local',
        identityHint: null,
        products: [
          {
            productId: 'codex-subscription',
            kind: 'codex-subscription',
            displayName: 'Codex 订阅',
            planTier: null,
            active: null,
            snapshot: null,
            query: {
              refreshing: false,
              lastAttemptEpoch: null,
              lastError: null,
            },
          },
        ],
      },
    ],
    preferences: DEFAULT_PREFERENCES,
  };
}

const THEME_KEY = 'token-refill.theme';
const LEGACY_PREFS_KEY = 'token-refill.prefs-draft';

function validTheme(value: unknown): value is ThemePref {
  return value === 'light' || value === 'dark' || value === 'system';
}

function readLegacyDraft(): Record<string, unknown> | null {
  try {
    const raw = localStorage.getItem(LEGACY_PREFS_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === 'object' && parsed !== null
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

export function loadTheme(): ThemePref {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    if (validTheme(stored)) return stored;
  } catch {
    // 浏览器禁用本地存储时退回系统外观。
  }

  const legacy = readLegacyDraft();
  if (legacy && validTheme(legacy.theme)) return legacy.theme;
  return 'system';
}

export function saveTheme(theme: ThemePref): void {
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // 本地写入失败不影响当前会话的外观。
  }
}

/**
 * 旧版本把整套设置以草稿形式存在 localStorage。升级后读取一次并写入 Rust，
 * 写入成功后再删除旧键，避免迁移失败时丢掉原有设置。
 */
export function readLegacyPreferences(): Preferences | null {
  const legacy = readLegacyDraft();
  if (!legacy) return null;

  const refreshMinutes =
    typeof legacy.refreshMinutes === 'number'
    && REFRESH_OPTIONS.includes(legacy.refreshMinutes)
      ? legacy.refreshMinutes
      : DEFAULT_PREFERENCES.refreshMinutes;
  const lowQuotaThreshold =
    typeof legacy.lowQuotaThreshold === 'number'
    && legacy.lowQuotaThreshold >= 5
    && legacy.lowQuotaThreshold <= 50
      ? legacy.lowQuotaThreshold
      : DEFAULT_PREFERENCES.lowQuotaThreshold;

  return {
    refreshMinutes,
    lowQuotaEnabled: typeof legacy.lowQuotaEnabled === 'boolean'
      ? legacy.lowQuotaEnabled
      : DEFAULT_PREFERENCES.lowQuotaEnabled,
    lowQuotaThreshold,
    authExpiredEnabled: typeof legacy.authExpiredEnabled === 'boolean'
      ? legacy.authExpiredEnabled
      : DEFAULT_PREFERENCES.authExpiredEnabled,
    resetNoticeEnabled: typeof legacy.resetNoticeEnabled === 'boolean'
      ? legacy.resetNoticeEnabled
      : DEFAULT_PREFERENCES.resetNoticeEnabled,
    autoLaunchEnabled: typeof legacy.autoLaunchEnabled === 'boolean'
      ? legacy.autoLaunchEnabled
      : DEFAULT_PREFERENCES.autoLaunchEnabled,
  };
}

export function clearLegacyPreferences(): void {
  try {
    localStorage.removeItem(LEGACY_PREFS_KEY);
  } catch {
    // 删除失败不影响已写入后端的设置；下次启动可能再次迁移一次。
  }
}
