import { useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import {
  ArrowUp,
  Gauge,
  GearSix,
  PlugsIcon,
  UsersThree,
  type Icon,
} from '@phosphor-icons/react';
import logoWordmark from './assets/logo-text.png';
import { useAppState } from './lib/store';
import OverviewPanel from './panels/OverviewPanel';
import AccountsPanel from './panels/AccountsPanel';
import RelayPanel from './panels/RelayPanel';
import SettingsPanel from './panels/SettingsPanel';
import TitleBar from './components/TitleBar';
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
    subtitle: '订阅账户与中转站账户的额度、周期与重置时间，一处看清。',
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
    title: '中转站账户管理',
    subtitle: '添加、扫描与维护中转站账户；额度在总览中查看。',
  },
  {
    key: 'settings',
    label: '设置',
    Icon: GearSix,
    title: '设置',
    subtitle: '刷新节奏、提醒与系统行为。',
  },
];

/* 平台信息由 Rust 侧异步返回；先用 UA 同步兜底，避免 Windows 自绘标题栏首帧缺失。 */
function platformHint(): string | null {
  if (typeof navigator === 'undefined') return null;
  if (/Windows/i.test(navigator.userAgent)) return 'windows';
  if (/Macintosh|Mac OS X/i.test(navigator.userAgent)) return 'macos';
  return null;
}

export default function App() {
  const [active, setActive] = useState<PanelKey>('overview');
  const [isMaximized, setIsMaximized] = useState(false);
  const { error, stateFileNotice, update, openExternal, appInfo } = useAppState();
  const desktop = isTauri();
  // 应用信息（名称、版本、渠道、平台）统一来自 Rust 合并后的配置；首帧先用 UA
  // 兜底平台，避免 Windows 自绘标题栏与关闭装饰的窗口在首帧错位。
  const platform = appInfo?.platform ?? platformHint();
  const current = PANELS.find((panel) => panel.key === active) ?? PANELS[0];
  // 启动检查只在有正式新版本时提示；设置页仍可手动重新检查。
  // 侧栏页脚只放一个轻量入口，详细状态仍在设置页。
  const updateAvailable =
    update?.updateAvailable && update.releaseUrl
      ? {
          version: update.latestVersion,
          url: update.releaseUrl,
          selfUpdateSupported: update.selfUpdateSupported,
        }
      : null;

  // 安装版直接进入设置页的版本区块；免安装版仍打开发布页手动替换。
  function openUpdateEntry() {
    if (!updateAvailable) return;
    if (updateAvailable.selfUpdateSupported) {
      setActive('settings');
      window.requestAnimationFrame(() => {
        document
          .getElementById('settings-version')
          ?.scrollIntoView({ block: 'start' });
      });
      return;
    }
    void openExternal(updateAvailable.url);
  }

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

  // 标题栏形态按平台区分：macOS 用 Overlay 原生标题栏（红绿灯浮在内容上），
  // Windows 关闭系统装饰，由 TitleBar 自绘。取不到平台时保持 macOS 布局。
  const isWindows = platform === 'windows';

  return (
    <div
      className={styles.app}
      data-platform={platform ?? undefined}
      data-maximized={isMaximized || undefined}
    >
      {isWindows ? <TitleBar onMaximizedChange={setIsMaximized} /> : null}
      <div className={styles.appBody}>
        <aside className={styles.sidebar}>
          {/* macOS 原生标题栏：顶部安全区留给红绿灯，同时作为窗口拖拽区。 */}
          {isWindows ? null : (
            <div className={styles.titlebarSafeArea} data-tauri-drag-region aria-hidden="true" />
          )}
          <img
            className={styles.wordmark}
            src={logoWordmark}
            alt={appInfo?.name ?? 'Token Refill'}
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
            <span className={styles.versionGroup}>
              <span className={styles.version}>
                {appInfo ? `v${appInfo.version}` : 'v—'}
              </span>
              {appInfo?.channel === 'test' ? (
                <span className={styles.testBadge}>测试版</span>
              ) : null}
            </span>
            {updateAvailable ? (
              <button
                type="button"
                className={styles.updateChip}
                onClick={openUpdateEntry}
                title={`发现新版本 v${updateAvailable.version}，前往更新`}
                aria-label={`发现新版本 v${updateAvailable.version}，前往更新`}
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
            {stateFileNotice ? (
              <p className={`${styles.noticeDanger} ${styles.globalNotice}`} role="alert">
                {stateFileNotice}
              </p>
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
    </div>
  );
}
