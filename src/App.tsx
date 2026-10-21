import { useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import { Gauge, GearSix, Info, UsersThree, type Icon } from '@phosphor-icons/react';
import { version as appVersion } from '../package.json';
import logoWordmark from './assets/logo-text.png';
import OverviewPanel from './panels/OverviewPanel';
import AccountsPanel from './panels/AccountsPanel';
import SettingsPanel from './panels/SettingsPanel';
import styles from './App.module.css';

type PanelKey = 'overview' | 'accounts' | 'settings';

/* 侧栏底部的说明文案：只在悬浮/聚焦 tip 图标时出现 */
const LOCAL_ONLY_NOTE =
  '本地运行。Codex 额度通过本机官方 app-server 只读查询，不读取或保存登录凭证。';

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
    subtitle: '各账户与套餐的额度、周期和重置时间在一处查看；不同计量口径的数据不做合并。',
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
  const desktop = isTauri();
  const reducedMotion = useReducedMotion();
  const current = PANELS.find((panel) => panel.key === active) ?? PANELS[0];

  return (
    <div className={styles.app}>
      <aside className={styles.sidebar}>
        {/* macOS Overlay 标题栏：顶部安全区留给红绿灯，同时作为窗口拖拽区 */}
        <div className={styles.titlebarSafeArea} data-tauri-drag-region aria-hidden="true" />
        <img
          className={styles.wordmark}
          src={logoWordmark}
          alt="Token Refill"
          width={480}
          height={87}
        />

        <nav className={styles.nav} aria-label="管理窗口分区" role="tablist" aria-orientation="vertical">
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
              {active === panel.key ? (
                <m.span
                  layoutId="navPill"
                  className={styles.navPill}
                  initial={false}
                  transition={{ type: 'spring', stiffness: 520, damping: 38 }}
                />
              ) : null}
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
            <AnimatePresence mode="wait" initial={false}>
              <m.div
                key={active}
                data-tauri-drag-region
                initial={reducedMotion ? false : { opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={reducedMotion ? { opacity: 1 } : { opacity: 0 }}
                transition={{ duration: reducedMotion ? 0 : 0.15, ease: 'easeOut' }}
              >
                <h1 className={styles.panelTitle} data-tauri-drag-region>
                  {current.title}
                </h1>
                <p className={styles.subtitle} data-tauri-drag-region>
                  {current.subtitle}
                </p>
              </m.div>
            </AnimatePresence>
          </div>
        </header>

        <div className={styles.mainScroll}>
          <AnimatePresence mode="wait" initial={false}>
            <m.div
              key={active}
              id={`panel-${active}`}
              role="tabpanel"
              aria-labelledby={`tab-${active}`}
              className={styles.panelWrap}
              initial={reducedMotion ? false : { opacity: 0, y: 5 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reducedMotion ? { opacity: 1 } : { opacity: 0 }}
              transition={{ duration: reducedMotion ? 0 : 0.15, ease: 'easeOut' }}
            >
              {active === 'overview' ? <OverviewPanel desktop={desktop} /> : null}
              {active === 'accounts' ? <AccountsPanel /> : null}
              {active === 'settings' ? <SettingsPanel /> : null}
            </m.div>
          </AnimatePresence>

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
