import { useEffect } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { onThemeChanged } from './desktop';
import { THEME_OPTIONS, type ThemePref } from './prefs';

// 外观偏好写到根元素，由 CSS 挑选浅色/深色变量：
// light / dark 显式覆盖；system 交给 prefers-color-scheme 媒体查询决定。
export function applyTheme(theme: ThemePref): void {
  document.documentElement.dataset.theme = theme;
  // macOS 的窗口材质、红绿灯与原生菜单跟随系统外观；把应用内偏好同步过去，
  // 否则显式选深色时玻璃仍是系统浅色。system 传 null 表示继续跟随系统。
  if (isTauri()) {
    void invoke('set_app_theme', {
      theme: theme === 'system' ? null : theme,
    }).catch(() => {
      // 窗口材质只是外观增强，同步失败时保留 CSS 主题，不打扰用户。
    });
  }
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
