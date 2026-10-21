export type ThemePref = 'light' | 'dark' | 'system';

export interface Prefs {
  refreshMinutes: number;
  lowQuotaEnabled: boolean;
  lowQuotaThreshold: number;
  authExpiredEnabled: boolean;
  resetNoticeEnabled: boolean;
  autoLaunchEnabled: boolean;
  theme: ThemePref;
}

export const REFRESH_OPTIONS = [1, 5, 15, 30, 60];
export const THEME_OPTIONS: ThemePref[] = ['light', 'dark', 'system'];
export const THEME_LABELS: Record<ThemePref, string> = {
  light: '浅色',
  dark: '深色',
  system: '跟随系统',
};

const STORAGE_KEY = 'token-refill.prefs-draft';

// 默认值取自需求文档已确认项：5 分钟刷新、低额度 20% 且默认开、失效提醒默认开、
// 重置提醒与自启默认关。
export const DEFAULT_PREFS: Prefs = {
  refreshMinutes: 5,
  lowQuotaEnabled: true,
  lowQuotaThreshold: 20,
  authExpiredEnabled: true,
  resetNoticeEnabled: false,
  autoLaunchEnabled: false,
  theme: 'system',
};

export function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return DEFAULT_PREFS;
    const draft = parsed as Partial<Prefs>;
    return {
      refreshMinutes: REFRESH_OPTIONS.includes(draft.refreshMinutes ?? NaN)
        ? draft.refreshMinutes as number
        : DEFAULT_PREFS.refreshMinutes,
      lowQuotaEnabled: typeof draft.lowQuotaEnabled === 'boolean'
        ? draft.lowQuotaEnabled : DEFAULT_PREFS.lowQuotaEnabled,
      lowQuotaThreshold: typeof draft.lowQuotaThreshold === 'number'
        && draft.lowQuotaThreshold >= 5 && draft.lowQuotaThreshold <= 50
        ? draft.lowQuotaThreshold : DEFAULT_PREFS.lowQuotaThreshold,
      authExpiredEnabled: typeof draft.authExpiredEnabled === 'boolean'
        ? draft.authExpiredEnabled : DEFAULT_PREFS.authExpiredEnabled,
      resetNoticeEnabled: typeof draft.resetNoticeEnabled === 'boolean'
        ? draft.resetNoticeEnabled : DEFAULT_PREFS.resetNoticeEnabled,
      autoLaunchEnabled: typeof draft.autoLaunchEnabled === 'boolean'
        ? draft.autoLaunchEnabled : DEFAULT_PREFS.autoLaunchEnabled,
      theme: THEME_OPTIONS.includes(draft.theme as ThemePref)
        ? draft.theme as ThemePref
        : DEFAULT_PREFS.theme,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function savePrefs(prefs: Prefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // 本地草稿写入失败不影响页面；正式持久化以后端为准。
  }
}
