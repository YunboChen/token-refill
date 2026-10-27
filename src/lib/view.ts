import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * 菜单栏快速查看面板与主窗口共用同一份前端产物：面板通过窗口 URL 上的
 * `?view=tray` 区分，窗口 label 作为兜底（防止某些打包环境下查询串丢失）。
 */
export const IS_TRAY_VIEW =
  new URLSearchParams(window.location.search).get('view') === 'tray'
  || (isTauri() && getCurrentWindow().label === 'tray-panel');