import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * 首帧的平台提示：`get_app_info()` 异步返回前，先用 UA 同步落定
 * `html[data-platform]`，让窗口材质与平台专用 token 从第一帧就生效。
 */
export function hostPlatformHint(): 'macos' | 'windows' | null {
  if (typeof navigator === 'undefined') return null;
  if (/Windows/i.test(navigator.userAgent)) return 'windows';
  if (/Macintosh|Mac OS X/i.test(navigator.userAgent)) return 'macos';
  return null;
}

/** 同一份前端产物服务于主窗口、菜单栏面板、悬浮球与悬浮球弹层。 */
export type WindowView = 'main' | 'tray' | 'float' | 'float-popover';

const params = new URLSearchParams(window.location.search);
const requested = params.get('view');
const label = isTauri() ? getCurrentWindow().label : '';

/**
 * 窗口 URL 上的 `?view=` 是首选依据；窗口 label 作为兜底，
 * 防止某些打包环境下查询串丢失。
 */
export const WINDOW_VIEW: WindowView =
  requested === 'tray' || label === 'tray-panel'
    ? 'tray'
    : requested === 'float' || label === 'floating-ball'
      ? 'float'
      : requested === 'float-popover' || label === 'float-popover'
        ? 'float-popover'
        : 'main';
