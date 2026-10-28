import { useEffect, useState, type MouseEvent } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import logoIcon from '../assets/logo.png';
import styles from './TitleBar.module.css';

export interface TitleBarProps {
  platform: string | null;
  title?: string;
  onMaximizedChange?: (maximized: boolean) => void;
}

export default function TitleBar({ platform, title = 'Token Refill', onMaximizedChange }: TitleBarProps) {
  const [isMaximized, setIsMaximized] = useState(false);
  const desktop = isTauri();

  // 若平台尚未获取到，根据用户环境平滑兜底
  const isMac = platform
    ? platform === 'macos' || platform === 'darwin'
    : typeof navigator !== 'undefined' && /Macintosh|Mac OS X/i.test(navigator.userAgent);

  useEffect(() => {
    if (!desktop) return;
    let unlisten: (() => void) | undefined;
    let mounted = true;

    const syncMaximized = async () => {
      try {
        const appWindow = getCurrentWindow();
        const max = await appWindow.isMaximized();
        if (mounted) {
          setIsMaximized(max);
          onMaximizedChange?.(max);
        }
      } catch {
        // 忽略非桌面或初始化未就绪的情况
      }
    };

    void syncMaximized();

    const appWindow = getCurrentWindow();
    appWindow
      .onResized(syncMaximized)
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        window.addEventListener('resize', syncMaximized);
        unlisten = () => window.removeEventListener('resize', syncMaximized);
      });

    return () => {
      mounted = false;
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

  if (isMac) {
    return (
      <header className={styles.titleBar} data-platform="macos" role="banner">
        <div className={styles.macControls}>
          <button
            type="button"
            className={`${styles.macButton} ${styles.macClose}`}
            onClick={handleClose}
            title="关闭"
            aria-label="关闭"
          >
            <svg width="6" height="6" viewBox="0 0 6 6" aria-hidden="true">
              <line x1="0.7" y1="0.7" x2="5.3" y2="5.3" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
              <line x1="5.3" y1="0.7" x2="0.7" y2="5.3" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
            </svg>
          </button>
          <button
            type="button"
            className={`${styles.macButton} ${styles.macMinimize}`}
            onClick={handleMinimize}
            title="最小化"
            aria-label="最小化"
          >
            <svg width="6" height="2" viewBox="0 0 6 2" aria-hidden="true">
              <line x1="0.5" y1="1" x2="5.5" y2="1" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
            </svg>
          </button>
          <button
            type="button"
            className={`${styles.macButton} ${styles.macMaximize}`}
            onClick={handleToggleMaximize}
            title={isMaximized ? '还原' : '全屏'}
            aria-label={isMaximized ? '还原' : '全屏'}
          >
            <svg width="6" height="6" viewBox="0 0 6 6" aria-hidden="true">
              <path
                d="M1 5L5 1M1 2.6V5H3.4M2.6 1H5V3.4"
                stroke="currentColor"
                strokeWidth="0.9"
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
              />
            </svg>
          </button>
        </div>

        <div
          className={`${styles.dragArea} ${styles.macCenter}`}
          data-tauri-drag-region
          onDoubleClick={handleToggleMaximize}
        >
          <span className={styles.macTitle} data-tauri-drag-region>
            {title}
          </span>
        </div>

        <div className={styles.macRightSpacer} aria-hidden="true" />
      </header>
    );
  }

  return (
    <header className={styles.titleBar} data-platform="windows" role="banner">
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
