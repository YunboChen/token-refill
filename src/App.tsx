import { useEffect, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import {
  ArrowUp,
  Gauge,
  GearSix,
  PlugsIcon,
  UsersThree,
  type Icon,
} from '@phosphor-icons/react';
import { version as appVersion } from '../package.json';
import logoWordmark from './assets/logo-text.png';
import { getAppInfo } from './lib/desktop';
import { useAppState } from './lib/store';
import OverviewPanel from './panels/OverviewPanel';
import AccountsPanel from './panels/AccountsPanel';
import RelayPanel from './panels/RelayPanel';
import SettingsPanel from './panels/SettingsPanel';
import styles from './App.module.css';

type PanelKey = 'overview' | 'accounts' | 'relay' | 'settings';

interface PanelDef {
  key: PanelKey;
  label: string;
  Icon: Icon;
  title: string;
  subtitle: string;
}

/* 菜单按使用顺序平铺；底部只留版本信息与数据来源说明。 */
const PANELS: PanelDef[] = [
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
    key: 'relay',
    label: '中转站',
    Icon: PlugsIcon,
    title: '中转站额度',
    subtitle: '自动读取 Claude Code / Codex 配置，也可手动填写地址与 API Key。',
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
  // 启动检查只在有正式新版本时提示；设置页仍可手动重新检查。
  // 侧栏页脚只放一个轻量入口，详细状态仍在设置页。
  const updateAvailable =
    update?.updateAvailable && update.releaseUrl
      ? { version: update.latestVersion, url: update.releaseUrl }
      : null;

  const renderNavItem = (panel: PanelDef) => (
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
        <panel.Icon size={17} className={styles.navIcon} aria-hidden />
        <span className={styles.navText}>{panel.label}</span>
      </span>
    </button>
  );

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
          aria-label="主导航"
          role="tablist"
          aria-orientation="vertical"
        >
          {PANELS.map(renderNavItem)}
        </nav>

        {/* 侧栏底部：版本号 + 可选的升级入口。分隔线只负责分隔导航与应用信息。
            版本号与导航图标同一条左缘；有新版本时入口落在右侧。 */}
        <div className={styles.sidebarFoot}>
          <span className={styles.version}>v{appVersion}</span>
          {updateAvailable ? (
            <button
              type="button"
              className={styles.updateChip}
              onClick={() => void openExternal(updateAvailable.url)}
              title={`发现新版本 v${updateAvailable.version}，前往下载页`}
              aria-label={`发现新版本 v${updateAvailable.version}，前往下载页`}
            >
              <ArrowUp size={11} weight="bold" aria-hidden />
              v{updateAvailable.version}
            </button>
          ) : null}
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
            {active === 'relay' ? <RelayPanel /> : null}
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
