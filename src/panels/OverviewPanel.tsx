import { ArrowClockwise } from '@phosphor-icons/react';
import type { AccountState } from '../lib/desktop';
import { useAppState } from '../lib/store';
import styles from '../App.module.css';

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

function clockLabel(epoch: number): string {
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

function CodexAccountCard({
  account,
  desktop,
  threshold,
  lowQuotaEnabled,
  refresh,
  ready,
}: {
  account: AccountState;
  desktop: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  refresh: (accountId?: string) => Promise<void>;
  ready: boolean;
}) {
  const product =
    account.products.find((item) => item.kind === 'codex-subscription')
    ?? account.products[0]
    ?? null;
  const snapshot = product?.snapshot ?? null;
  const query = product?.query ?? null;
  const busy = query?.refreshing ?? false;
  const lastError = query?.lastError ?? null;
  const planLabel = product?.planTier
    ? product.planTier[0].toUpperCase() + product.planTier.slice(1)
    : null;

  return (
    <article className={styles.card} aria-live="polite">
      <header className={styles.cardHead}>
        <div className={styles.cardTitleRow}>
          <h2>{account.displayName}</h2>
          <span className={styles.accountProduct}>
            {product?.displayName ?? 'Codex 订阅'}
            {account.identityHint ? ` · ${account.identityHint}` : ''}
          </span>
          {planLabel ? <span className={styles.planTag}>{planLabel}</span> : null}
        </div>
        <div className={styles.cardTools}>
          {snapshot ? (
            <span className={styles.stamp}>
              更新于 {clockLabel(snapshot.checkedAtEpoch)}
              {busy ? ' · 刷新中' : ''}
            </span>
          ) : null}
          <button
            type="button"
            className={styles.iconButton}
            onClick={() => void refresh(account.accountId)}
            disabled={!desktop || busy}
            aria-label={`刷新 ${account.displayName} 的额度`}
            title={desktop ? '刷新额度' : '浏览器预览无法调用 Rust'}
          >
            <ArrowClockwise
              size={16}
              weight="regular"
              className={busy ? styles.spin : undefined}
            />
          </button>
        </div>
      </header>

      {!desktop ? (
        <p className={styles.usageNote}>
          浏览器预览无法调用 Rust，请在桌面应用内查看（
          <code className={styles.codeChip}>pnpm tauri dev</code>）。
        </p>
      ) : !ready ? (
        <UsageSkeleton />
      ) : (
        <>
          {snapshot && lastError ? (
            <p className={styles.noticeStale} role="status">
              上次刷新失败：{lastError.message}
              <span className={styles.usageNote}>（错误码：{lastError.code}）</span>
              {snapshot ? ` 当前显示 ${clockLabel(snapshot.checkedAtEpoch)} 的数据。` : null}
            </p>
          ) : null}

          {snapshot ? (
            <>
              {snapshot.groups.length === 0 ? (
                <p className={styles.usageNote}>当前账号未返回额度窗口数据。</p>
              ) : (
                <div className={styles.rows}>
                  {snapshot.groups.map((group, groupIndex) => (
                    <div
                      className={styles.usageGroup}
                      key={group.limitId ?? `group-${groupIndex}`}
                    >
                      {group.limitName ? (
                        <p className={styles.groupLabel}>{group.limitName}</p>
                      ) : null}
                      <div className={styles.rows}>
                        {group.windows.map((window) => {
                          const remaining = window.remainingPercent;
                          const used = window.usedPercent;
                          const label = durationLabel(window.windowDurationMins) || '额度窗口';
                          const low =
                            lowQuotaEnabled
                            && remaining !== null
                            && remaining <= threshold;
                          return (
                            <div className={styles.usageRow} key={window.windowId}>
                              <div className={styles.usageRowHead}>
                                <span className={styles.usageLabel}>{label}</span>
                                <span className={styles.usagePercent} data-low={low || undefined}>
                                  {remaining === null ? '—' : `${remaining}%`}
                                </span>
                              </div>
                              <div
                                className={styles.meter}
                                role="progressbar"
                                aria-label={`${account.displayName} ${label}剩余比例`}
                                aria-valuenow={remaining ?? undefined}
                                aria-valuemin={0}
                                aria-valuemax={100}
                              >
                                <div
                                  className={styles.meterFill}
                                  style={{ width: `${remaining ?? 0}%` }}
                                  data-low={low || undefined}
                                  data-unknown={remaining === null || undefined}
                                />
                              </div>
                              <div className={styles.usageRowFoot}>
                                <span>{resetLabel(window.resetsAtEpoch)}</span>
                                <span>{used === null ? '已用未知' : `已用 ${used}%`}</span>
                              </div>
                              {low ? (
                                <p className={styles.lowNote}>
                                  剩余已低于提醒阈值 {threshold}%
                                </p>
                              ) : null}
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              )}
              {snapshot.capabilityGaps.length > 0 ? (
                <p className={styles.usageUpdated}>
                  数据缺口：{snapshot.capabilityGaps.join('；')}
                </p>
              ) : null}
            </>
          ) : busy ? (
            <>
              <UsageSkeleton />
              <p className={styles.usageUpdated}>正在通过 Codex CLI 查询额度…</p>
            </>
          ) : lastError ? (
            <p className={styles.noticeDanger}>
              {lastError.message}
              <span className={styles.usageNote}>（错误码：{lastError.code}）</span>
            </p>
          ) : (
            <p className={styles.usageNote}>
              尚未获取额度。点击右上角刷新；后台会自动重试。
            </p>
          )}
        </>
      )}
    </article>
  );
}

export default function OverviewPanel({ desktop }: { desktop: boolean }) {
  const { state, ready, refresh } = useAppState();
  const threshold = state.preferences.lowQuotaThreshold;
  const lowOn = state.preferences.lowQuotaEnabled;

  return (
    <section>
      {state.accounts.length === 0 ? (
        <p className={styles.usageNote}>尚未接入账户。请在“账户”页添加。</p>
      ) : (
        <div className={styles.cardStack}>
          {state.accounts.map((account) => (
            <CodexAccountCard
              key={account.accountId}
              account={account}
              desktop={desktop}
              threshold={threshold}
              lowQuotaEnabled={lowOn}
              refresh={refresh}
              ready={ready}
            />
          ))}
        </div>
      )}

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