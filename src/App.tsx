import { useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import { getAppInfo, type AppInfo } from './lib/desktop';
import styles from './App.module.css';

type ConnectionState =
  | { status: 'idle' | 'loading' }
  | { status: 'success'; info: AppInfo }
  | { status: 'error'; message: string };

const statusLabels = {
  idle: '尚未检测',
  loading: '正在连接 Rust…',
  success: 'React 与 Rust 通信正常',
  error: '连接失败，请重试',
};

export default function App() {
  const [connection, setConnection] = useState<ConnectionState>({ status: 'idle' });
  const desktop = isTauri();
  const reducedMotion = useReducedMotion();
  const loading = connection.status === 'loading';

  async function checkConnection() {
    if (!desktop || loading) return;

    setConnection({ status: 'loading' });

    try {
      const info = await getAppInfo();
      setConnection({ status: 'success', info });
    } catch (error) {
      setConnection({
        status: 'error',
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return (
    <main className={styles.app}>
      <header className={styles.header}>
        <div className={styles.wordmark}>
          <span className={styles.mark} aria-hidden="true">TR</span>
          <span>Token Refill</span>
        </div>
        <span className={styles.mode}>{desktop ? '桌面模式' : '浏览器预览'}</span>
      </header>

      <section className={styles.accounts} aria-labelledby="accounts-title">
        <h1 id="accounts-title">你的 AI 套餐，一处查看。</h1>
        <p className={styles.subtitle}>关注额度，也关注下一次重置。</p>

        <div className={styles.emptyState}>
          <h2>尚未添加账户</h2>
          <p>接入首个平台后，这里将显示套餐用量与重置时间。</p>
          <span className={styles.note}>当前为项目骨架，尚未接入额度数据。</span>
        </div>
      </section>

      <section className={styles.environment} aria-labelledby="environment-title">
        <div className={styles.sectionHeader}>
          <h2 id="environment-title">开发环境</h2>
          <span className={styles.stack}>React + Rust</span>
        </div>
        <p className={styles.description}>
          {desktop
            ? '通过一次本地调用，检查前端与 Rust 的连接。'
            : '网页可预览界面；Rust 调用需要在桌面应用中执行。'}
        </p>

        <m.button
          type="button"
          className={styles.button}
          onClick={checkConnection}
          disabled={!desktop || loading}
          aria-describedby="connection-status"
          whileTap={reducedMotion ? undefined : { scale: 0.98 }}
        >
          {loading ? '正在检测…' : '检查 Rust 连接'}
        </m.button>

        {!desktop ? (
          <p className={styles.previewHint}>使用 <code>pnpm tauri dev</code> 启动桌面应用。</p>
        ) : null}

        <div
          id="connection-status"
          className={styles.result}
          aria-live="polite"
          aria-atomic="true"
          aria-busy={loading}
        >
          <AnimatePresence mode="wait" initial={false}>
            <m.div
              key={connection.status}
              initial={reducedMotion ? false : { opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={reducedMotion ? { opacity: 1 } : { opacity: 0 }}
              transition={{ duration: reducedMotion ? 0 : 0.14, ease: 'easeOut' }}
            >
              <p
                className={styles.status}
                data-status={connection.status}
              >
                {statusLabels[connection.status]}
              </p>
              {connection.status === 'success' ? (
                <dl className={styles.details}>
                  <div><dt>应用</dt><dd>{connection.info.name}</dd></div>
                  <div><dt>版本</dt><dd>{connection.info.version}</dd></div>
                  <div><dt>平台</dt><dd>{connection.info.platform}</dd></div>
                  <div><dt>架构</dt><dd>{connection.info.arch}</dd></div>
                </dl>
              ) : null}
              {connection.status === 'error' ? (
                <p className={styles.error}>{connection.message}</p>
              ) : null}
            </m.div>
          </AnimatePresence>
        </div>
      </section>

      <footer className={styles.footer}>
        本地运行。当前不读取账户凭证，不发送分析数据。
      </footer>
    </main>
  );
}
