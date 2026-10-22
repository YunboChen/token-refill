import { ArrowClockwise, Gauge } from '@phosphor-icons/react';
import { m, useReducedMotion } from 'motion/react';
import type { AccountState } from '../lib/desktop';
import { groupAccountsByPlatform } from '../lib/platforms';
import { useAppState } from '../lib/store';
import PlatformHeading from '../components/PlatformHeading';
import PlatformLogo from '../components/PlatformLogo';
import styles from '../App.module.css';

const RING_SIZE = 64;
const RING_STROKE = 5;
const RING_RADIUS = (RING_SIZE - RING_STROKE) / 2;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

function durationLabel(mins: number | null): string {
  if (mins === null) return '';
  if (mins < 60) return `${mins} 分钟`;
  if (mins < 1440) return `${Math.round(mins / 60)} 小时`;
  if (mins === 10080) return '1 周';
  return `${Math.round(mins / 1440)} 天`;
}

function windowLabel(mins: number | null): string {
  const duration = durationLabel(mins);
  return duration ? `${duration}窗口` : '额度窗口';
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

/** 剩余额度环：数值在环中央，弧长随剩余比例变化。 */
function QuotaRing({
  value,
  low,
  label,
}: {
  value: number | null;
  low: boolean;
  label: string;
}) {
  const reducedMotion = useReducedMotion();
  const known = value !== null;
  const percent = known ? Math.min(100, Math.max(0, value)) : 0;
  const offset = RING_CIRCUMFERENCE * (1 - percent / 100);

  return (
    <div
      className={styles.ring}
      data-low={low || undefined}
      data-unknown={!known || undefined}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={known ? percent : undefined}
      aria-valuetext={known ? `剩余 ${percent}%` : '剩余比例未知'}
    >
      <svg
        className={styles.ringSvg}
        viewBox={`0 0 ${RING_SIZE} ${RING_SIZE}`}
        aria-hidden="true"
        focusable="false"
      >
        <circle
          className={styles.ringTrack}
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={RING_RADIUS}
          fill="none"
          strokeWidth={RING_STROKE}
        />
        {known ? (
          <m.circle
            className={styles.ringArc}
            cx={RING_SIZE / 2}
            cy={RING_SIZE / 2}
            r={RING_RADIUS}
            fill="none"
            strokeWidth={RING_STROKE}
            strokeLinecap="round"
            strokeDasharray={RING_CIRCUMFERENCE}
            initial={false}
            animate={{ strokeDashoffset: offset }}
            transition={
              reducedMotion
                ? { duration: 0 }
                : { duration: 0.8, ease: [0.16, 1, 0.3, 1] }
            }
            transform={`rotate(-90 ${RING_SIZE / 2} ${RING_SIZE / 2})`}
          />
        ) : null}
      </svg>
      <span className={styles.ringValue}>{known ? `${percent}%` : '-'}</span>
    </div>
  );
}

function UsageSkeleton() {
  return (
    <div className={styles.quotaGrid} aria-hidden="true">
      {[0, 1].map((index) => (
        <div className={styles.quotaTile} key={index}>
          <span className={`${styles.skel} ${styles.skelRing}`} />
          <div className={styles.skelLines}>
            <span className={`${styles.skel} ${styles.w96}`} />
            <span className={`${styles.skel} ${styles.w150}`} />
          </div>
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
    <article className={styles.accountCard} aria-live="polite">
      <header className={styles.accountHead}>
        <div className={styles.accountTitleBlock}>
          <h3 className={styles.accountName}>{account.displayName}</h3>
          <p className={styles.accountMetaLine}>
            <span>{product?.displayName ?? 'Codex 订阅'}</span>
            {planLabel ? <span className={styles.planBadge}>{planLabel}</span> : null}
            {account.identityHint ? (
              <span className={styles.accountHint} title={account.identityHint}>
                {account.identityHint}
              </span>
            ) : null}
          </p>
        </div>
        <div className={styles.accountTools}>
          {snapshot ? (
            <span className={styles.updatedAt}>
              更新于 {clockLabel(snapshot.checkedAtEpoch)}
              {busy ? ' · 刷新中' : ''}
            </span>
          ) : null}
          <button
            type="button"
            className={styles.refreshButton}
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
        <p className={styles.accountNote}>
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
              <span className={styles.noticeAside}>（错误码：{lastError.code}）</span>
              {snapshot ? ` 当前显示 ${clockLabel(snapshot.checkedAtEpoch)} 的数据。` : null}
            </p>
          ) : null}

          {snapshot ? (
            <>
              {snapshot.groups.length === 0 ? (
                <p className={styles.accountNote}>当前账号未返回额度窗口数据。</p>
              ) : (
                snapshot.groups.map((group, groupIndex) => (
                  <section
                    className={styles.quotaGroup}
                    key={group.limitId ?? `group-${groupIndex}`}
                  >
                    {group.limitName ? (
                      <p className={styles.quotaGroupLabel}>{group.limitName}</p>
                    ) : null}
                    <div className={styles.quotaGrid}>
                      {group.windows.map((window) => {
                        const remaining = window.remainingPercent;
                        const used = window.usedPercent;
                        const low =
                          lowQuotaEnabled
                          && remaining !== null
                          && remaining <= threshold;
                        const label = windowLabel(window.windowDurationMins);
                        return (
                          <div className={styles.quotaTile} key={window.windowId}>
                            <QuotaRing
                              value={remaining}
                              low={low}
                              label={`${account.displayName} ${label}剩余比例`}
                            />
                            <div className={styles.quotaCopy}>
                              <div className={styles.quotaTitleRow}>
                                <span className={styles.quotaLabel}>{label}</span>
                              </div>
                              <span className={styles.quotaMeta}>
                                {resetLabel(window.resetsAtEpoch)}
                                {used !== null ? ` · 已用 ${used}%` : ''}
                              </span>
                              {low ? (
                                <span className={styles.quotaLowNote}>
                                  剩余已低于提醒阈值 {threshold}%
                                </span>
                              ) : null}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </section>
                ))
              )}
              {snapshot.capabilityGaps.length > 0 ? (
                <p className={styles.dataGaps}>
                  数据缺口：{snapshot.capabilityGaps.join('；')}
                </p>
              ) : null}
            </>
          ) : busy ? (
            <>
              <UsageSkeleton />
              <p className={styles.accountNote}>正在通过 Codex CLI 查询额度…</p>
            </>
          ) : lastError ? (
            <p className={styles.noticeDanger}>
              {lastError.message}
              <span className={styles.noticeAside}>（错误码：{lastError.code}）</span>
            </p>
          ) : (
            <p className={styles.accountNote}>
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
  const groups = groupAccountsByPlatform(state.accounts);

  return (
    <section>
      <div className={styles.overviewStack}>
        {state.accounts.length === 0 ? (
          <div className={styles.overviewEmpty}>
            <span className={styles.emptyGlyph} aria-hidden="true">
              <Gauge size={24} weight="duotone" />
            </span>
            <h2 className={styles.emptyTitle}>还没有可查看的额度</h2>
            <p className={styles.usageNote}>
              在“账户”页接入账户后，这里会显示每个账户的剩余额度、周期与重置时间。
            </p>
          </div>
        ) : (
          groups.map((group) => (
            <section className={styles.platformGroup} key={group.platform}>
              <PlatformHeading
                platform={group.platform}
                count={group.accounts.length}
                active
              />
              <div className={styles.accountStack}>
                {group.accounts.map((account) => (
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
            </section>
          ))
        )}

        <footer className={styles.upcoming}>
          <span className={styles.upcomingBadge} aria-hidden="true">
            <PlatformLogo platform="bailian" fallback="百" />
          </span>
          <p className={styles.upcomingNote}>
            <span className={styles.upcomingName}>百炼</span>
            Token Plan 与 Coding Plan 正在验证接入，开放后会在这里按账户分别展示。
          </p>
          <span className={styles.upcomingStatus}>待接入</span>
        </footer>
      </div>
    </section>
  );
}