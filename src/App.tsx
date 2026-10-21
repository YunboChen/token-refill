import { useCallback, useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import {
  getAppInfo,
  refreshCodexUsage,
  type AppInfo,
  type CodexUsage,
  type UsageError,
} from './lib/desktop';
import logoMark from './assets/logo.png';
import styles from './App.module.css';

type ConnectionState =
  | { status: 'idle' | 'loading' }
  | { status: 'success'; info: AppInfo }
  | { status: 'error'; message: string };

type UsageState =
  | { status: 'idle' | 'loading' }
  | { status: 'ok'; data: CodexUsage }
  | { status: 'error'; error: UsageError };

const statusLabels = {
  idle: '尚未检测',
  loading: '正在连接 Rust…',
  success: 'React 与 Rust 通信正常',
  error: '连接失败，请重试',
};

function toUsageError(err: unknown): UsageError {
  if (typeof err === 'object' && err !== null && 'code' in err && 'message' in err) {
    return err as UsageError;
  }
  return { code: 'unknown', message: err instanceof Error ? err.message : String(err) };
}

function durationLabel(mins: number | null): string {
  if (mins === null) return '';
  if (mins < 60) return `${mins} 分钟`;
  if (mins < 1440) return `${Math.round(mins / 60)} 小时`;
  if (mins === 10080) return '1 周';
  return `${Math.round(mins / 1440)} 天`;
}

function resetLabel(epoch: number | null): string {
  if (epoch === null) return '重置时间未知';
  const date = new Date(epoch * 1000);
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const now = new Date();
  if (date.toDateString() === now.toDateString()) return `重置于 ${time}`;
  const day = `${date.getMonth() + 1}月${date.getDate()}日`;
  return `重置于 ${day} ${time}`;
}

function checkedAtLabel(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

export default function App() {
  const [connection, setConnection] = useState<ConnectionState>({ status: 'idle' });
  const [usage, setUsage] = useState<UsageState>({ status: 'idle' });
  const usageBusy = useRef(false);
  const desktop = isTauri();
  const reducedMotion = useReducedMotion();
  const loading = connection.status === 'loading';

  const loadUsage = useCallback(async () => {
    if (!desktop || usageBusy.current) return;
    usageBusy.current = true;
    setUsage({ status: 'loading' });
    try {
      const data = await refreshCodexUsage();
      setUsage({ status: 'ok', data });
    } catch (err) {
      setUsage({ status: 'error', error: toUsageError(err) });
    } finally {
      usageBusy.current = false;
    }
  }, [desktop]);

  useEffect(() => {
    if (!desktop) return;
    setUsage({ status: 'idle' });
    void loadUsage();
  }, [desktop, loadUsage]);

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

  const planType = usage.status === 'ok'
    ? usage.data.buckets.find((bucket) => bucket.planType)?.planType ?? null
    : null;
  const planLabel = planType ? planType[0].toUpperCase() + planType.slice(1) : null;

  return (
    <main className={styles.app}>
      <header className={styles.header}>
        <div className={styles.wordmark}>
          <img className={styles.mark} src={logoMark} alt="" width={25} height={28} />
          <span>Token Refill</span>
        </div>
        <span className={styles.mode}>{desktop ? '桌面模式' : '浏览器预览'}</span>
      </header>

      <section className={styles.accounts} aria-labelledby="accounts-title">
        <h1 id="accounts-title">你的 AI 套餐，一处查看。</h1>
        <p className={styles.subtitle}>关注额度，也关注下一次重置。</p>

        <div className={styles.usageCard} aria-live="polite">
          <div className={styles.sectionHeader}>
            <h2>Codex 订阅</h2>
            {planLabel ? <span className={styles.planTag}>{planLabel}</span> : null}
          </div>

          {usage.status === 'ok' ? (
            <>
              {usage.data.buckets.length === 0 ? (
                <p className={styles.usageNote}>当前账号未返回额度窗口数据。</p>
              ) : null}
              {usage.data.buckets.map((bucket, bucketIndex) =>
                bucket.windows.map((window, windowIndex) => {
                  const label = durationLabel(window.windowDurationMins)
                    || bucket.limitName
                    || bucket.limitId
                    || '额度窗口';
                  return (
                    <div className={styles.usageRow} key={`${bucketIndex}-${windowIndex}`}>
                      <div className={styles.usageRowHead}>
                        <span className={styles.usageLabel}>{label}</span>
                        <span className={styles.usagePercent}>剩余 {window.remainingPercent}%</span>
                      </div>
                      <div className={styles.meter} aria-hidden="true">
                        <div
                          className={styles.meterFill}
                          style={{ width: `${window.remainingPercent}%` }}
                        />
                      </div>
                      <div className={styles.usageRowFoot}>
                        <span className={styles.usageNote}>{resetLabel(window.resetsAtEpoch)}</span>
                        <span className={styles.usageNote}>已用 {window.usedPercent}%</span>
                      </div>
                    </div>
                  );
                }),
              )}
              <p className={styles.usageUpdated}>更新于 {checkedAtLabel(usage.data.checkedAtEpoch)}</p>
            </>
          ) : null}

          {usage.status === 'loading' || usage.status === 'idle' ? (
            <p className={styles.usageNote}>
              {desktop ? '正在通过本机 Codex CLI 查询额度…' : '浏览器预览无法调用 Rust。'}
            </p>
          ) : null}

          {usage.status === 'error' ? (
            <p className={styles.usageError}>
              {usage.error.message}
              <span className={styles.usageNote}>（错误码：{usage.error.code}）</span>
            </p>
          ) : null}

          <button
            type="button"
            className={styles.button}
            onClick={() => void loadUsage()}
            disabled={!desktop || usage.status === 'loading'}
          >
            {usage.status === 'loading' ? '正在查询…' : '刷新 Codex 额度'}
          </button>
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
        本地运行。Codex 额度通过本机官方 app-server 只读查询，不读取或保存登录凭证。
      </footer>
    </main>
  );
}
