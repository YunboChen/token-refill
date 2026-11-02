import { useEffect } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { onThemeChanged } from './desktop';
import { THEME_OPTIONS, type ThemePref } from './prefs';

// 外观偏好写到根元素，由 CSS 挑选浅色/深色变量：
// light / dark 显式覆盖；system 交给 prefers-color-scheme 媒体查询决定。
export function applyTheme(theme: ThemePref): void {
  document.documentElement.dataset.theme = theme;
}

/**
 * 主题偏好保存在前端 localStorage；主窗口切换后通过事件广播给其他窗口。
 * 每个窗口启动时已经自行 applyTheme，挂载后只需跟随后续变化。
 */
export function useThemeSync(): void {
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onThemeChanged((theme) => {
      if (THEME_OPTIONS.includes(theme as ThemePref)) {
        applyTheme(theme as ThemePref);
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
