import { useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import logoMark from './assets/logo.png';
import OverviewPanel from './panels/OverviewPanel';
import AccountsPanel from './panels/AccountsPanel';
import SettingsPanel from './panels/SettingsPanel';
import styles from './App.module.css';

type PanelKey = 'overview' | 'accounts' | 'settings';

const PANELS: { key: PanelKey; label: string }[] = [
  { key: 'overview', label: '总览' },
  { key: 'accounts', label: '账户' },
  { key: 'settings', label: '设置' },
];

export default function App() {
  const [active, setActive] = useState<PanelKey>('overview');
  const desktop = isTauri();
  const reducedMotion = useReducedMotion();

  return (
    <div className={styles.app}>
      {/* macOS Overlay 标题栏：字标与红绿灯同行，整条与应用同色、可拖拽。 */}
      <div className={styles.titlebar} aria-hidden="true">
        <div className={styles.titlebarSide} data-tauri-drag-region>
          <div className={styles.brand} data-tauri-drag-region>
            <img
              className={styles.mark}
              src={logoMark}
              alt=""
              width={28}
              height={32}
              data-tauri-drag-region
            />
            <span className={styles.brandName} data-tauri-drag-region>Token Refill</span>
          </div>
        </div>
        <div className={styles.titlebarMain} data-tauri-drag-region />
      </div>

      <div className={styles.body}>
        <aside className={styles.sidebar}>
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
                <span className={styles.navLabel}>{panel.label}</span>
              </button>
            ))}
          </nav>
        </aside>

        <main className={styles.main}>
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

          <footer className={styles.footer}>
            本地运行。Codex 额度通过本机官方 app-server 只读查询，不读取或保存登录凭证。
            {!desktop ? (
              <>
                当前为浏览器预览，接口需在桌面应用内调用（
                <code className={styles.codeChip}>pnpm tauri dev</code>）。
              </>
            ) : null}
          </footer>
        </main>
      </div>
    </div>
  );
}
