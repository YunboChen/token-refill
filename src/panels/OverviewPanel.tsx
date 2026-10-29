import { Fragment, useState, type CSSProperties } from 'react';
import {
  ArrowClockwise,
  Gauge,
  PlugsIcon,
  UsersThree,
  type Icon,
} from '@phosphor-icons/react';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import type {
  AccountState,
  ProductState,
  QuotaGroup,
  QuotaWindow,
} from '../lib/desktop';
import { groupAccountsByPlatform } from '../lib/platforms';
import {
  clockLabel,
  compactQuotaWindowLabel,
  groupLabel,
  planTierLabel,
  quotaWindowLabel,
  resetLabel,
  sourceLabel,
} from '../lib/format';
import { useAppState } from '../lib/store';
import ChevronIcon from '../components/ChevronIcon';
import PlatformHeading from '../components/PlatformHeading';
import RelayQuotaBoard from '../components/RelayQuotaBoard';
import styles from '../App.module.css';

type OverviewTab = 'subscription' | 'relay';

const OVERVIEW_TABS: { key: OverviewTab; label: string; Icon: Icon }[] = [
  { key: 'subscription', label: '订阅账户额度', Icon: UsersThree },
  { key: 'relay', label: '中转站账户额度', Icon: PlugsIcon },
];

function amountLabel(window: QuotaWindow): string | null {
  if (window.unit === 'usd') {
    const money = (value: number) => `$${value.toFixed(2)}`;
    if (window.usedAmount !== null && window.totalAmount !== null) {
      return `已用 ${money(window.usedAmount)} / ${money(window.totalAmount)}`;
    }
    if (window.remainingAmount !== null) {
      return `剩余 ${money(window.remainingAmount)}`;
    }
    return null;
  }
  if (window.usedAmount === null || window.totalAmount === null) return null;
  const unit = window.unit === 'calls' ? '次' : window.unit;
  return `已用 ${window.usedAmount} / ${window.totalAmount} ${unit}`;
}

/** 账户级档位只在所有套餐一致时展示；多套餐档位不同则落到每类套餐上。 */
function accountPlanTier(account: AccountState): string | null {
  if (account.products.length === 0) return null;
  const first = account.products[0].planTier;
  if (!first) return null;
  return account.products.every((product) => product.planTier === first)
    ? first
    : null;
}

function updatedAt(account: AccountState): number | null {
  return account.products.reduce<number | null>((latest, product) => {
    const current = product.snapshot?.checkedAtEpoch ?? null;
    if (current === null) return latest;
    return latest === null ? current : Math.max(latest, current);
  }, null);
}

/** 折叠行摘要：单套餐列出各窗口余量；多套餐每个套餐只留剩余最低的窗口。 */
function accountSummary(account: AccountState): string {
  const products = account.products
    .map((product) => ({
      product,
      windows: (product.snapshot?.groups ?? [])
        .flatMap((group) => group.windows)
        .filter(
          (window): window is QuotaWindow & { remainingPercent: number } =>
            window.remainingPercent !== null,
        ),
    }))
    .filter(({ windows }) => windows.length > 0);

  if (products.length === 0) {
    if (account.products.some((product) => product.query.refreshing)) {
      return '正在查询…';
    }
    if (account.products.some((product) => product.query.lastError)) {
      return '刷新失败';
    }
    if (account.products.some((product) => product.snapshot)) {
      return '额度未知';
    }
    return '尚未获取额度';
  }

  const windowSummary = (
    windows: (QuotaWindow & { remainingPercent: number })[],
  ) =>
    windows
      .map(
        (window) =>
          `${compactQuotaWindowLabel(window)} ${Math.round(window.remainingPercent)}%`,
      )
      .join(' · ');

  // 单套餐单分组账户不重复套餐名，直接列出各窗口（如 Codex 的 5 小时 / 周）。
  if (account.products.length === 1) {
    const groups = (account.products[0].snapshot?.groups ?? []).filter(
      (group) => group.windows.some((w) => w.remainingPercent !== null),
    );
    // 单套餐多分组（如 Antigravity 的 Gemini / Claude 模型分组）：按组展示紧凑摘要
    if (groups.length > 1) {
      return groups
        .map((group) => {
          const validWindows = group.windows.filter(
            (w): w is QuotaWindow & { remainingPercent: number } =>
              w.remainingPercent !== null,
          );
          if (validWindows.length === 0) return null;
          const tightest = validWindows.reduce((lowest, w) =>
            w.remainingPercent < lowest.remainingPercent ? w : lowest,
          );
          const name = groupLabel(group.limitName) || '额度';
          return `${name} ${windowSummary([tightest])}`;
        })
        .filter(Boolean)
        .join(' | ');
    }
    return windowSummary(products[0].windows);
  }

  // 多套餐账户（如百炼的 Token Plan + Coding Plan）逐段铺开会挤满整行，
  // 每个套餐只取剩余最低的窗口，摘要保持一眼可读。
  return products
    .map(({ product, windows }) => {
      const tightest = windows.reduce((lowest, window) =>
        window.remainingPercent < lowest.remainingPercent ? window : lowest,
      );
      return `${product.displayName} ${windowSummary([tightest])}`;
    })
    .join(' | ');
}

interface MetricEntry {
  key: string;
  product: ProductState;
  group: QuotaGroup;
  window: QuotaWindow;
  /** 多套餐时，每类套餐的第一个窗口负责显示套餐名与档位。 */
  productLead: boolean;
  /** 单类套餐内有多个额度组时，每组第一个窗口负责显示组名。 */
  groupLead: boolean;
}

interface MetricSet {
  entries: MetricEntry[];
  showEyebrow: boolean;
}

function buildMetrics(account: AccountState): MetricSet {
  const products = account.products.filter(
    (product) =>
      product.snapshot !== null
      && product.snapshot.groups.some((group) => group.windows.length > 0),
  );
  const multipleProducts = account.products.length > 1;
  const entries: MetricEntry[] = [];

  products.forEach((product) => {
    const groups = (product.snapshot?.groups ?? []).filter(
      (group) => group.windows.length > 0,
    );
    const multipleGroups = groups.length > 1;
    groups.forEach((group, groupIndex) => {
      group.windows.forEach((window, windowIndex) => {
        entries.push({
          key: `${product.productId}-${groupIndex}-${window.windowId}`,
          product,
          group,
          window,
          productLead: multipleProducts && groupIndex === 0 && windowIndex === 0,
          groupLead: multipleGroups && windowIndex === 0,
        });
      });
    });
  });

  return {
    entries,
    showEyebrow: multipleProducts
      || entries.some((entry) => entry.groupLead && entry.group.limitName),
  };
}

/** 剩余额度条：数值与进度条同行，弧长随剩余比例变化。 */
function QuotaBar({
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

  return (
    <div
      className={styles.quotaBarRow}
      data-low={low || undefined}
      data-unknown={!known || undefined}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={known ? percent : undefined}
      aria-valuetext={known ? `剩余 ${percent}%` : '剩余比例未知'}
    >
      <span className={styles.quotaPercent}>
        {known ? `${percent}%` : '-'}
      </span>
      <span className={styles.quotaBarTrack} aria-hidden="true">
        {known ? (
          <m.span
            className={styles.quotaBarFill}
            initial={false}
            animate={{ width: `${percent}%` }}
            transition={
              reducedMotion
                ? { duration: 0 }
                : { duration: 0.7, ease: [0.16, 1, 0.3, 1] }
            }
          />
        ) : null}
      </span>
    </div>
  );
}

function UsageSkeleton() {
  return (
    <div className={styles.metricGrid} aria-hidden="true">
      {[0, 1].map((index) => (
        <Fragment key={index}>
          {index > 0 ? <span className={styles.metricDivider} /> : null}
          <div className={styles.metric}>
            <span className={`${styles.skel} ${styles.skelLabel}`} />
            <span className={`${styles.skel} ${styles.skelValue}`} />
            <span className={`${styles.skel} ${styles.skelBar}`} />
            <span className={`${styles.skel} ${styles.skelMeta}`} />
          </div>
        </Fragment>
      ))}
    </div>
  );
}

function AccountBody({
  account,
  desktop,
  threshold,
  lowQuotaEnabled,
  ready,
}: {
  account: AccountState;
  desktop: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  ready: boolean;
}) {
  const metrics = buildMetrics(account);
  const busy = account.products.some((product) => product.query.refreshing);
  const snapshots = account.products.flatMap((product) =>
    product.snapshot ? [product.snapshot] : [],
  );
  const staleProducts = account.products.filter(
    (product) => product.snapshot !== null && product.query.lastError !== null,
  );
  const failedProducts = account.products.filter(
    (product) => product.snapshot === null && product.query.lastError !== null,
  );
  const gaps = [
    ...new Set(snapshots.flatMap((snapshot) => snapshot.capabilityGaps)),
  ];

  if (!desktop) {
    return (
      <p className={styles.accountNote}>
        浏览器预览无法调用 Rust，请在桌面应用内查看（
        <code className={styles.codeChip}>pnpm tauri dev</code>）。
      </p>
    );
  }

  if (!ready) return <UsageSkeleton />;

  return (
    <>
      {staleProducts.length > 0 || failedProducts.length > 0 ? (
        <div className={styles.quotaNotices}>
          {staleProducts.map((product) => (
            <p className={styles.noticeStale} role="status" key={product.productId}>
              {product.displayName}：刷新失败，{product.query.lastError?.message}
              <span className={styles.noticeAside}>
                （错误码：{product.query.lastError?.code}）
              </span>
              {product.snapshot
                ? ` 当前显示 ${clockLabel(product.snapshot.checkedAtEpoch)} 的数据。`
                : null}
            </p>
          ))}
          {failedProducts.map((product) => (
            <p className={styles.noticeDanger} role="alert" key={product.productId}>
              {product.displayName}：{product.query.lastError?.message}
              <span className={styles.noticeAside}>
                （错误码：{product.query.lastError?.code}）
              </span>
            </p>
          ))}
        </div>
      ) : null}

      {snapshots.length === 0 ? (
        busy ? (
          <>
            <UsageSkeleton />
            <p className={styles.accountNote}>正在查询额度…</p>
          </>
        ) : failedProducts.length === 0 ? (
          <p className={styles.accountNote}>
            尚未获取额度。点击右上角刷新；后台会自动重试。
          </p>
        ) : null
      ) : metrics.entries.length === 0 ? (
        <p className={styles.accountNote}>当前账号未返回该类套餐的额度数据。</p>
      ) : (
        <div className={styles.metricGrid}>
          {metrics.entries.map((entry, index) => {
            const { product, group, window } = entry;
            const remaining = window.remainingPercent;
            const low =
              lowQuotaEnabled
              && remaining !== null
              && remaining <= threshold;
            const label = quotaWindowLabel(
              window.windowDurationMins,
              window.windowName,
            );
            const usedAmount = amountLabel(window);
            const usedPercent =
              window.usedPercent !== null
                ? `已用 ${window.usedPercent}%`
                : null;
            // 多窗口并排时列宽有限，已用与重置时间统一分两行；
            // Codex 这类两列布局仍保留一行，节省纵向空间。
            const stackedMeta =
              usedAmount !== null || metrics.entries.length > 2;
            const eyebrowParts = [
              entry.productLead ? product.displayName : null,
              entry.groupLead ? groupLabel(group.limitName) : null,
            ].filter(
              (part, index, list): part is string =>
                Boolean(part) && list.indexOf(part) === index,
            );

            return (
              <Fragment key={entry.key}>
                {index > 0 ? (
                  <span
                    className={styles.metricDivider}
                    data-strong={entry.productLead || undefined}
                    aria-hidden="true"
                  />
                ) : null}
                <div className={styles.metric}>
                  {metrics.showEyebrow ? (
                    <div className={styles.metricEyebrow}>
                      {eyebrowParts.length > 0 ? (
                        <span
                          className={styles.metricProduct}
                          title={eyebrowParts.join(' · ')}
                        >
                          {eyebrowParts.join(' · ')}
                        </span>
                      ) : null}
                      {entry.productLead && product.planTier ? (
                        <span className={styles.planTier}>
                          {planTierLabel(product.planTier)}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                  <span className={styles.metricLabel}>{label}</span>
                  <QuotaBar
                    value={remaining}
                    low={low}
                    label={`${account.displayName} ${product.displayName} ${label}剩余比例`}
                  />
                  <span
                    className={styles.metricMeta}
                    data-stacked={stackedMeta || undefined}
                  >
                    {usedAmount ? (
                      <span className={styles.metricMetaPart}>{usedAmount}</span>
                    ) : usedPercent ? (
                      <>
                        <span className={styles.metricMetaPart}>
                          {usedPercent}
                        </span>
                        {!stackedMeta ? (
                          <span className={styles.metricDot} aria-hidden="true">
                            ·
                          </span>
                        ) : null}
                      </>
                    ) : null}
                    <span className={styles.metricMetaPart}>
                      {resetLabel(window.resetsAtEpoch)}
                    </span>
                  </span>
                  {low ? (
                    <span className={styles.quotaLowNote}>
                      低于提醒阈值 {threshold}%
                    </span>
                  ) : null}
                </div>
              </Fragment>
            );
          })}
        </div>
      )}

      {gaps.length > 0 ? (
        <p className={styles.dataGaps}>数据缺口：{gaps.join('；')}</p>
      ) : null}
    </>
  );
}

/** 账户身份与来源：常驻标题区第二行，展开与收起都不改变头部高度。 */
function AccountIdentity({ account }: { account: AccountState }) {
  return (
    <>
      {account.identityHint ? (
        <>
          <span className={styles.accountHint} title={account.identityHint}>
            {account.identityHint}
          </span>
          <span className={styles.metaDot} aria-hidden="true">
            ·
          </span>
        </>
      ) : null}
      <span>{sourceLabel(account)}</span>
      {account.authState === 'needs-reauth' ? (
        <>
          <span className={styles.metaDot} aria-hidden="true">
            ·
          </span>
          <span className={styles.authStatus} data-state="needs-reauth">
            <span className={styles.authDot} aria-hidden="true" />
            需要重新授权
          </span>
        </>
      ) : null}
    </>
  );
}

function AccountPanel({
  account,
  open,
  onToggle,
  desktop,
  threshold,
  lowQuotaEnabled,
  refresh,
  ready,
}: {
  account: AccountState;
  open: boolean;
  onToggle: () => void;
  desktop: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  refresh: (accountId?: string) => Promise<void>;
  ready: boolean;
}) {
  const reducedMotion = useReducedMotion();
  const busy = account.products.some((product) => product.query.refreshing);
  const updated = updatedAt(account);
  const tier = accountPlanTier(account);
  const bodyId = `overview-account-${account.accountId}`;
  const summary = accountSummary(account);

  return (
    <article className={styles.quotaAccount} aria-live="polite">
      <header className={styles.quotaAccountHead}>
        <button
          type="button"
          className={styles.quotaAccountToggle}
          onClick={onToggle}
          aria-expanded={open}
          aria-controls={bodyId}
        >
          <span className={styles.quotaAccountInfo}>
            <span className={styles.quotaAccountIdentity}>
              <span className={styles.quotaAccountName}>
                {account.displayName}
              </span>
              {tier ? (
                <span className={styles.planTier}>{planTierLabel(tier)}</span>
              ) : null}
            </span>
            {/* 身份/来源常驻第二行：头部高度在展开与收起时不变，
                和「中转站账户额度」一样只动画额度区，避免整行跳动。 */}
            <span className={styles.quotaAccountMeta}>
              <AccountIdentity account={account} />
            </span>
          </span>
          {!open ? <span className={styles.quotaSummary}>{summary}</span> : null}
        </button>

        <div className={styles.quotaAccountTools}>
          {open && (updated !== null || busy) ? (
            <span className={styles.updatedAt}>
              {updated !== null ? `更新于 ${clockLabel(updated)}` : '刷新中'}
              {updated !== null && busy ? ' · 刷新中' : ''}
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
          {/* 展开/收起已由左侧名称按钮承担；这里只是同一动作的视觉补充。 */}
          <span
            className={styles.chevronButton}
            onClick={onToggle}
            aria-hidden="true"
            title={open ? '收起账户' : '展开账户'}
          >
            {open ? (
              <ChevronIcon direction="down" />
            ) : (
              <ChevronIcon direction="right" />
            )}
          </span>
        </div>
      </header>

      <AnimatePresence initial={false}>
        {open ? (
          <m.div
            key="body"
            id={bodyId}
            className={styles.quotaBody}
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={
              reducedMotion
                ? { duration: 0 }
                : { duration: 0.26, ease: [0.16, 1, 0.3, 1] }
            }
          >
            <div className={styles.quotaBodyInner}>
              <AccountBody
                account={account}
                desktop={desktop}
                threshold={threshold}
                lowQuotaEnabled={lowQuotaEnabled}
                ready={ready}
              />
            </div>
          </m.div>
        ) : null}
      </AnimatePresence>
    </article>
  );
}

/** 同一平台共用一个卡片；默认展开第一个账户，其余账户折叠为摘要行。 */
function PlatformBoard({
  platform,
  accounts,
  desktop,
  threshold,
  lowQuotaEnabled,
  refresh,
  ready,
}: {
  platform: string;
  accounts: AccountState[];
  desktop: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  refresh: (accountId?: string) => Promise<void>;
  ready: boolean;
}) {
  // undefined 表示沿用默认值（第一个账户展开）；null 表示全部收起。
  const [openAccountId, setOpenAccountId] = useState<string | null | undefined>(
    undefined,
  );
  const fallbackId = accounts[0]?.accountId ?? null;
  const activeId =
    openAccountId === undefined
      ? fallbackId
      : accounts.some((account) => account.accountId === openAccountId)
        ? openAccountId
        : null;

  return (
    <section className={styles.platformGroup}>
      {/* 与账户页共用同一套平台标题样式：小尺寸品牌图形、无底色，
          标题行与下方账户卡左缘对齐。 */}
      <div className={styles.platformSectionHead}>
        <PlatformHeading
          platform={platform}
          count={accounts.length}
          active
        />
      </div>
      <div className={styles.quotaBoard}>
        {accounts.map((account) => (
          <AccountPanel
            key={account.accountId}
            account={account}
            open={account.accountId === activeId}
            onToggle={() =>
              setOpenAccountId(
                account.accountId === activeId ? null : account.accountId,
              )
            }
            desktop={desktop}
            threshold={threshold}
            lowQuotaEnabled={lowQuotaEnabled}
            refresh={refresh}
            ready={ready}
          />
        ))}
      </div>
    </section>
  );
}

export default function OverviewPanel({ desktop }: { desktop: boolean }) {
  const { state, ready, refresh } = useAppState();
  const [tab, setTab] = useState<OverviewTab>('subscription');
  const tabIndex = Math.max(0, OVERVIEW_TABS.findIndex(({ key }) => key === tab));
  const threshold = state.preferences.lowQuotaThreshold;
  const lowOn = state.preferences.lowQuotaEnabled;
  const groups = groupAccountsByPlatform(state.accounts);

  return (
    <section>
      <div
        className={styles.overviewTabs}
        role="tablist"
        aria-label="总览类别"
        style={{
          '--pill-index': tabIndex,
          '--pill-count': OVERVIEW_TABS.length,
        } as CSSProperties}
      >
        <span className={styles.pillThumb} aria-hidden="true" />
        {OVERVIEW_TABS.map(({ key, label, Icon }) => (
          <button
            key={key}
            type="button"
            role="tab"
            id={`overview-tab-${key}`}
            aria-selected={tab === key}
            aria-controls={`overview-panel-${key}`}
            className={styles.overviewTab}
            data-active={tab === key || undefined}
            onClick={() => setTab(key)}
          >
            <Icon size={15} aria-hidden />
            {label}
          </button>
        ))}
      </div>

      {tab === 'subscription' ? (
        <div
          id="overview-panel-subscription"
          role="tabpanel"
          aria-labelledby="overview-tab-subscription"
        >
          <div className={styles.overviewStack}>
            {state.accounts.length === 0 ? (
              <div className={styles.overviewEmpty}>
                <span className={styles.emptyGlyph} aria-hidden="true">
                  <Gauge size={24} weight="duotone" />
                </span>
                <h2 className={styles.emptyTitle}>还没有可查看的订阅额度</h2>
                <p className={styles.usageNote}>
                  在“账户”页接入账户后，这里会显示每个账户的剩余额度、周期与重置时间。
                </p>
              </div>
            ) : (
              groups.map((group) => (
                <PlatformBoard
                  key={group.platform}
                  platform={group.platform}
                  accounts={group.accounts}
                  desktop={desktop}
                  threshold={threshold}
                  lowQuotaEnabled={lowOn}
                  refresh={refresh}
                  ready={ready}
                />
              ))
            )}
          </div>
        </div>
      ) : (
        <div
          id="overview-panel-relay"
          role="tabpanel"
          aria-labelledby="overview-tab-relay"
        >
          <RelayQuotaBoard desktop={desktop} />
        </div>
      )}
    </section>
  );
}
