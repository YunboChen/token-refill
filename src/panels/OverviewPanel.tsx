import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowClockwise } from '@phosphor-icons/react';
import {
  refreshCodexUsage,
  type CodexUsage,
  type UsageError,
} from '../lib/desktop';
import { loadPrefs } from '../lib/prefs';
import styles from '../App.module.css';

type UsageState =
  | { status: 'idle' | 'loading' }
  | { status: 'ok'; data: CodexUsage }
  | { status: 'error'; error: UsageError };

/** 尚未接入的套餐占位；文案与 docs/platform-integration-validation.md 的验证状态保持一致。 */
const pendingPlans = [
  {
    title: '百炼 Token Plan',
    tag: '个人版',
    note: '尚未接入：个人版查询授权与 Credits 计量字段仍待验证，接入后在此按账户展示。',
  },
  {
    title: '百炼 Coding Plan',
    tag: '中国站',
    note: '尚未接入：中国站查询授权与多窗口调用次数口径仍待验证，接入后在此按账户展示。',
  },
];

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

function UsageSkeleton() {
  return (
    <div className={styles.skeleton} aria-hidden="true">
      {[
        { label: 'w96', foot: 'w150' },
        { label: 'w120', foot: 'w180' },
      ].map((row) => (
        <div className={styles.skelRow} key={row.label}>
          <div className={styles.skelHead}>
            <span className={`${styles.skel} ${styles[row.label]}`} />
            <span className={`${styles.skel} ${styles.w56}`} />
          </div>
          <span className={`${styles.skel} ${styles.meterSkel}`} />
          <span className={`${styles.skel} ${styles[row.foot]}`} />
        </div>
      ))}
    </div>
  );
}

export default function OverviewPanel({ desktop }: { desktop: boolean }) {
  const [usage, setUsage] = useState<UsageState>({ status: 'idle' });
  const [prefs] = useState(loadPrefs);
  const usageBusy = useRef(false);

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

  const planType = usage.status === 'ok'
    ? usage.data.buckets.find((bucket) => bucket.planType)?.planType ?? null
    : null;
  const planLabel = planType ? planType[0].toUpperCase() + planType.slice(1) : null;
  const lowOn = prefs.lowQuotaEnabled;

  return (
    <section>
      <h1 className={styles.panelTitle}>总览</h1>
      <p className={styles.subtitle}>
        各账户与套餐的额度、周期和重置时间在一处查看；不同计量口径的数据不做合并。
      </p>

      <article className={styles.card} aria-live="polite">
        <header className={styles.cardHead}>
          <div className={styles.cardTitleRow}>
            <h2>Codex 订阅</h2>
            {planLabel ? <span className={styles.planTag}>{planLabel}</span> : null}
          </div>
          <div className={styles.cardTools}>
            {usage.status === 'ok' ? (
              <span className={styles.stamp}>更新于 {checkedAtLabel(usage.data.checkedAtEpoch)}</span>
            ) : null}
            <button
              type="button"
              className={styles.iconButton}
              onClick={() => void loadUsage()}
              disabled={!desktop || usage.status === 'loading'}
              aria-label="刷新 Codex 额度"
              title={desktop ? '刷新 Codex 额度' : '浏览器预览无法调用 Rust'}
            >
              <ArrowClockwise
                size={16}
                weight="regular"
                className={usage.status === 'loading' ? styles.spin : undefined}
              />
            </button>
          </div>
        </header>

        {usage.status === 'ok' ? (
          usage.data.buckets.length === 0 ? (
            <p className={styles.usageNote}>当前账号未返回额度窗口数据。</p>
          ) : (
            <div className={styles.rows}>
              {usage.data.buckets.map((bucket, bucketIndex) =>
                bucket.windows.map((window, windowIndex) => {
                  const label = durationLabel(window.windowDurationMins)
                    || bucket.limitName
                    || bucket.limitId
                    || '额度窗口';
                  const low = lowOn && window.remainingPercent <= prefs.lowQuotaThreshold;
                  return (
                    <div className={styles.usageRow} key={`${bucketIndex}-${windowIndex}`}>
                      <div className={styles.usageRowHead}>
                        <span className={styles.usageLabel}>{label}</span>
                        <span className={styles.usagePercent} data-low={low || undefined}>
                          {window.remainingPercent}%
                        </span>
                      </div>
                      <div className={styles.meter} role="progressbar"
                        aria-label={`${label}剩余比例`}
                        aria-valuenow={window.remainingPercent} aria-valuemin={0} aria-valuemax={100}>
                        <div
                          className={styles.meterFill}
                          style={{ width: `${window.remainingPercent}%` }}
                          data-low={low || undefined}
                        />
                      </div>
                      <div className={styles.usageRowFoot}>
                        <span>{resetLabel(window.resetsAtEpoch)}</span>
                        <span>已用 {window.usedPercent}%</span>
                      </div>
                      {low ? (
                        <p className={styles.lowNote}>剩余已低于提醒阈值 {prefs.lowQuotaThreshold}%</p>
                      ) : null}
                    </div>
                  );
                }),
              )}
            </div>
          )
        ) : null}

        {usage.status === 'loading' || usage.status === 'idle' ? (
          desktop ? (
            <>
              <UsageSkeleton />
              <p className={styles.usageUpdated}>正在通过本机 Codex CLI 查询额度…</p>
            </>
          ) : (
            <p className={styles.usageNote}>
              浏览器预览无法调用 Rust，请在桌面应用内查看（
              <code className={styles.codeChip}>pnpm tauri dev</code>）。
            </p>
          )
        ) : null}

        {usage.status === 'error' ? (
          <p className={styles.noticeDanger}>
            {usage.error.message}
            <span className={styles.usageNote}>（错误码：{usage.error.code}）</span>
          </p>
        ) : null}
      </article>

      <h2 className={styles.groupTitle}>尚未接入</h2>
      <article className={`${styles.card} ${styles.listCard}`}>
        {pendingPlans.map((plan) => (
          <div key={plan.title} className={styles.pendingRow}>
            <div className={styles.pendingHead}>
              <h3>{plan.title}</h3>
              <span className={styles.planTag}>{plan.tag}</span>
            </div>
            <p className={styles.usageNote}>{plan.note}</p>
          </div>
        ))}
      </article>
    </section>
  );
}
