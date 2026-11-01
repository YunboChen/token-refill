import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

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
