import type { ThemePref } from './prefs';

// 外观偏好写到根元素，由 CSS 挑选浅色/深色变量：
// light / dark 显式覆盖；system 交给 prefers-color-scheme 媒体查询决定。
export function applyTheme(theme: ThemePref): void {
  document.documentElement.dataset.theme = theme;
}