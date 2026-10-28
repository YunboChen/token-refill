import { useEffect, useState, type MouseEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import logoIcon from '../assets/logo.png';
import styles from './TitleBar.module.css';

export interface TitleBarProps {
  title?: string;
  onMaximizedChange?: (maximized: boolean) => void;
}

/* Windows 专用自绘标题栏：窗口在 Rust 侧关闭系统装饰，最小化/最大化/关闭
   与拖拽都由这里承担。macOS 继续使用 Overlay 原生标题栏，不渲染本组件。 */
export default function TitleBar({ title = 'Token Refill', onMaximizedChange }: TitleBarProps) {
  const [isMaximized, setIsMaximized] = useState(false);
  const desktop = isTauri();

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const syncMaximized = async () => {
      try {
        const max = await getCurrentWindow().isMaximized();
        if (disposed) return;
        setIsMaximized(max);
        onMaximizedChange?.(max);
      } catch {
        // 忽略非桌面或初始化未就绪的情况
      }
    };

    void syncMaximized();

    const appWindow = getCurrentWindow();
    appWindow
      .onResized(syncMaximized)
      .then((fn) => {
        // 组件在订阅返回前卸载时立刻退订，避免监听泄漏。
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        if (disposed) return;
        window.addEventListener('resize', syncMaximized);
        unlisten = () => window.removeEventListener('resize', syncMaximized);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop, onMaximizedChange]);

  const handleMinimize = async (e: MouseEvent) => {
    e.stopPropagation();
    if (!desktop) return;
    try {
      await getCurrentWindow().minimize();
    } catch (err) {
      console.error('窗口最小化失败:', err);
    }
  };

  const handleToggleMaximize = async (e?: MouseEvent) => {
    e?.stopPropagation();
    if (!desktop) return;
    try {
      const appWindow = getCurrentWindow();
      await appWindow.toggleMaximize();
      const max = await appWindow.isMaximized();
      setIsMaximized(max);
      onMaximizedChange?.(max);
    } catch (err) {
      console.error('窗口切换最大化失败:', err);
    }
  };

  const handleClose = async (e: MouseEvent) => {
    e.stopPropagation();
    if (!desktop) return;
    try {
      await getCurrentWindow().close();
    } catch (err) {
      console.error('窗口关闭失败:', err);
    }
  };

  return (
    <header className={styles.titleBar} role="banner">
      <div
        className={styles.brand}
        data-tauri-drag-region
        onDoubleClick={handleToggleMaximize}
      >
        <img
          src={logoIcon}
          alt=""
          className={styles.appIcon}
          width={16}
          height={16}
          data-tauri-drag-region
        />
        <span className={styles.appTitle} data-tauri-drag-region>
          {title}
        </span>
      </div>

      <div
        className={styles.dragArea}
        data-tauri-drag-region
        onDoubleClick={handleToggleMaximize}
      />

      <div className={styles.winControls}>
        <button
          type="button"
          className={styles.winButton}
          onClick={handleMinimize}
          title="最小化"
          aria-label="最小化"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" aria-hidden="true">
            <rect width="10" height="1" fill="currentColor" />
          </svg>
        </button>
        <button
          type="button"
          className={styles.winButton}
          onClick={handleToggleMaximize}
          title={isMaximized ? '向下还原' : '最大化'}
          aria-label={isMaximized ? '向下还原' : '最大化'}
        >
          {isMaximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M2.5 0.5h7v7h-2v2h-7v-7h2v-2z" fill="none" />
              <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" strokeWidth="1" />
              <path d="M2.5 2.5v-2h7v7h-2" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1" />
            </svg>
          )}
        </button>
        <button
          type="button"
          className={`${styles.winButton} ${styles.winClose}`}
          onClick={handleClose}
          title="关闭"
          aria-label="关闭"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" strokeWidth="1" />
            <line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" strokeWidth="1" />
          </svg>
        </button>
      </div>
    </header>
  );
}