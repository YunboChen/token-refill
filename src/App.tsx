import { useEffect, useState, type CSSProperties } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { Gauge, GearSix, Info, UsersThree, type Icon } from '@phosphor-icons/react';
import { version as appVersion } from '../package.json';
import logoWordmark from './assets/logo-text.png';
import { getAppInfo } from './lib/desktop';
import { useAppState } from './lib/store';
import OverviewPanel from './panels/OverviewPanel';
import AccountsPanel from './panels/AccountsPanel';
import SettingsPanel from './panels/SettingsPanel';
import styles from './App.module.css';

type PanelKey = 'overview' | 'accounts' | 'settings';

/* 侧栏底部的说明文案：只在悬浮/聚焦 tip 图标时出现 */
const LOCAL_ONLY_NOTE =
  '本地运行。本机账户沿用 Codex CLI 登录态；独立账户通过官方浏览器授权，各自使用隔离的本地授权目录，令牌不在界面中展示。';

const PANELS: {
  key: PanelKey;
  label: string;
  Icon: Icon;
  title: string;
  subtitle: string;
}[] = [
  {
    key: 'overview',
    label: '总览',
    Icon: Gauge,
    title: '总览',
    subtitle: '每个账户的额度、周期与重置时间，一处看清。',
  },
  {
    key: 'accounts',
    label: '账户',
    Icon: UsersThree,
    title: '账户',
    subtitle: '管理已接入的账户、授权状态与关联套餐。',
  },
  {
    key: 'settings',
    label: '设置',
    Icon: GearSix,
    title: '设置',
    subtitle: '刷新节奏、提醒与系统行为。',
  },
];

export default function App() {
  const [active, setActive] = useState<PanelKey>('overview');
  const [platform, setPlatform] = useState<string | null>(null);
  const { error, update, openExternal } = useAppState();
  const desktop = isTauri();
  const current = PANELS.find((panel) => panel.key === active) ?? PANELS[0];
  const activeIndex = Math.max(0, PANELS.findIndex((panel) => panel.key === active));
  // 启动检查只在有正式新版本时提示；设置页仍可手动重新检查。
  const updateNotice =
    update?.updateAvailable && update.releaseUrl
      ? { version: update.latestVersion, url: update.releaseUrl }
      : null;

  // 标题栏形态按平台区分：macOS 是 Overlay（红绿灯浮在内容上），
  // Windows 用系统标题栏。取不到平台时保持 macOS 布局，浏览器预览不受影响。
  useEffect(() => {
    if (!desktop) return;
    let mounted = true;
    getAppInfo()
      .then((info) => {
        if (mounted) setPlatform(info.platform);
      })
      .catch(() => {
        // 平台信息只影响留白，读取失败时不阻塞界面。
      });
    return () => {
      mounted = false;
    };
  }, [desktop]);

  return (
    <div className={styles.app} data-platform={platform ?? undefined}>
      <aside className={styles.sidebar}>
        {/* macOS Overlay 标题栏：顶部安全区留给红绿灯，同时作为窗口拖拽区。
            Windows 用系统标题栏，这段只保留极小的顶部留白。 */}
        <div className={styles.titlebarSafeArea} data-tauri-drag-region aria-hidden="true" />
        <img
          className={styles.wordmark}
          src={logoWordmark}
          alt="Token Refill"
          width={480}
          height={87}
        />

        <nav
          className={styles.nav}
          style={{ '--nav-index': activeIndex } as CSSProperties}
          aria-label="管理窗口分区"
          role="tablist"
          aria-orientation="vertical"
        >
          <span className={styles.navPill} aria-hidden="true" />
          {PANELS.map((panel) => (
            <button
              key={panel.key}
              type="button"
              role="tab"
              id={`tab-${panel.key}`}
              aria-selected={active === panel.key}
              aria-controls={`panel-${panel.key}`}
              className={styles.navItem}
              data-active={active === panel.key || undefined}
              onClick={() => setActive(panel.key)}
            >
              <span className={styles.navLabel}>
                <panel.Icon size={18} className={styles.navIcon} aria-hidden />
                <span className={styles.navText}>{panel.label}</span>
              </span>
            </button>
          ))}
        </nav>

        {/* 侧栏底部：版本号 + 数据来源说明 tip，悬浮或键盘聚焦时展开 */}
        <div className={styles.sidebarFoot}>
          <span className={styles.version}>Token Refill v{appVersion}</span>
          <button
            type="button"
            className={styles.tipTrigger}
            aria-label="数据来源说明"
            aria-describedby="local-only-tip"
          >
            <Info size={14} aria-hidden />
          </button>
          <span role="tooltip" id="local-only-tip" className={styles.tooltip}>
            {LOCAL_ONLY_NOTE}
          </span>
        </div>
      </aside>

      <main className={styles.main}>
        {/* 页面头部独立于滚动区：直接顶到窗口上沿，同时作为拖拽区 */}
        <header className={styles.pageHeader} data-tauri-drag-region>
          <div className={styles.pageHeaderInner}>
            <h1 className={styles.panelTitle} data-tauri-drag-region>
              {current.title}
            </h1>
            <p className={styles.subtitle} data-tauri-drag-region>
              {current.subtitle}
            </p>
          </div>
        </header>

        <div className={styles.mainScroll}>
          {updateNotice ? (
            <div className={styles.noticeUpdate} role="status">
              <span className={styles.noticeUpdateText}>
                发现新版本 v{updateNotice.version}，当前 v{appVersion}。
              </span>
              <button
                type="button"
                className={styles.noticeUpdateAction}
                onClick={() => void openExternal(updateNotice.url)}
              >
                前往下载
              </button>
            </div>
          ) : null}
          {error ? (
            <p className={`${styles.noticeDanger} ${styles.globalNotice}`} role="alert">
              {error}
            </p>
          ) : null}
          <div
            key={active}
            id={`panel-${active}`}
            role="tabpanel"
            aria-labelledby={`tab-${active}`}
            className={styles.panelWrap}
          >
            {active === 'overview' ? <OverviewPanel desktop={desktop} /> : null}
            {active === 'accounts' ? <AccountsPanel /> : null}
            {active === 'settings' ? <SettingsPanel /> : null}
          </div>

          {!desktop ? (
            <footer className={styles.footer}>
              当前为浏览器预览，接口需在桌面应用内调用（
              <code className={styles.codeChip}>pnpm tauri dev</code>）。
            </footer>
          ) : null}
        </div>
      </main>
    </div>
  );
}
