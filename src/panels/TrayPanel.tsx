import { useCallback, useEffect, useRef, useState } from 'react';
import { version as appVersion } from '../../package.json';
import {
  ArrowClockwise,
  ArrowUpRight,
  Gauge,
  Power,
} from '@phosphor-icons/react';
import wordmark from '../assets/logo-text.png';
import {
  hideTrayPanel,
  onThemeChanged,
  onTrayPanelShown,
  openMainWindow,
  quitApp,
  type AccountState,
  type KeyQuota,
  type QuotaWindow,
  type RelayAccount,
} from '../lib/desktop';
import { THEME_OPTIONS, type ThemePref } from '../lib/prefs';
import { applyTheme } from '../lib/theme';
import {
  clockLabel,
  planTierLabel,
  quotaWindowLabel,
  resetLabel,
} from '../lib/format';
import { groupAccountsByPlatform, platformMeta } from '../lib/platforms';
import { useRelay } from '../lib/relay';
import {
  RELAY_PROVIDER_LABELS,
  formatRelayMoney,
  relayHost,
  relayPercent,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import PlatformLogo from '../components/PlatformLogo';
import styles from './TrayPanel.module.css';

interface QuotaRow {
  key: string;
  label: string;
  eyebrow: string | null;
  percent: number | null;
  meta: string[];
  low: boolean;
}

/** 金额/次数补充说明；没有绝对值时退回已用比例。 */
function amountText(window: QuotaWindow): string | null {
  const money = (value: number) => `$${value.toFixed(2)}`;
  if (window.unit === 'usd') {
    if (window.usedAmount !== null && window.totalAmount !== null) {
      return `已用 ${money(window.usedAmount)} / ${money(window.totalAmount)}`;
    }
    if (window.remainingAmount !== null) {
      return `剩余 ${money(window.remainingAmount)}`;
    }
    return window.usedPercent !== null ? `已用 ${window.usedPercent}%` : null;
  }
  if (window.usedAmount !== null && window.totalAmount !== null) {
    const unit = window.unit === 'calls' ? '次' : window.unit;
    return `已用 ${window.usedAmount} / ${window.totalAmount} ${unit}`;
  }
  return window.usedPercent !== null ? `已用 ${window.usedPercent}%` : null;
}

/** 账户级档位只在所有套餐一致时展示；与总览的规则保持一致。 */
function accountPlanTier(account: AccountState): string | null {
  if (account.products.length === 0) return null;
  const first = account.products[0].planTier;
  if (!first) return null;
  return account.products.every((product) => product.planTier === first)
    ? first
    : null;
}

function accountRows(
  account: AccountState,
  threshold: number,
  lowQuotaEnabled: boolean,
): QuotaRow[] {
  const multipleProducts = account.products.length > 1;
  const rows: QuotaRow[] = [];

  account.products.forEach((product) => {
    const groups = (product.snapshot?.groups ?? []).filter(
      (group) => group.windows.length > 0,
    );
    groups.forEach((group, groupIndex) => {
      group.windows.forEach((window, windowIndex) => {
        const percent = window.remainingPercent;
        rows.push({
          key: `${product.productId}-${groupIndex}-${window.windowId}`,
          label: quotaWindowLabel(window.windowDurationMins, window.windowName),
          // 多套餐账户（如百炼）在每类套餐的第一行标注套餐名。
          eyebrow:
            multipleProducts && groupIndex === 0 && windowIndex === 0
              ? product.displayName
              : null,
          percent,
          meta: [amountText(window), resetLabel(window.resetsAtEpoch)].filter(
            (part): part is string => part !== null,
          ),
          low:
            lowQuotaEnabled && percent !== null && percent <= threshold,
        });
      });
    });
  });

  return rows;
}

function AccountBlock({
  account,
  threshold,
  lowQuotaEnabled,
}: {
  account: AccountState;
  threshold: number;
  lowQuotaEnabled: boolean;
}) {
  const rows = accountRows(account, threshold, lowQuotaEnabled);
  const tier = accountPlanTier(account);
  const refreshing = account.products.some(
    (product) => product.query.refreshing,
  );
  const failure = account.products.find(
    (product) => product.query.lastError !== null,
  )?.query.lastError;

  return (
    <div className={styles.account}>
      <div className={styles.accountHead}>
        <span className={styles.accountName} title={account.displayName}>
          {account.displayName}
        </span>
        <span className={styles.accountTags}>
          {tier ? <span className={styles.tag}>{planTierLabel(tier)}</span> : null}
          {account.authState === 'needs-reauth' ? (
            <span className={styles.authBadge}>重新授权</span>
          ) : null}
        </span>
      </div>

      {rows.length > 0 ? (
        rows.map((row) => (
          <div className={styles.quotaRow} key={row.key}>
            <div className={styles.quotaHead}>
              <span className={styles.quotaLabel}>
                {row.eyebrow ? (
                  <span className={styles.eyebrow}>{row.eyebrow}</span>
                ) : null}
                {row.label}
              </span>
              <span
                className={styles.percent}
                data-low={row.low || undefined}
                data-unknown={row.percent === null || undefined}
              >
                {row.percent !== null ? `${Math.round(row.percent)}%` : '—'}
              </span>
            </div>
            <div
              className={styles.bar}
              data-low={row.low || undefined}
              role="progressbar"
              aria-label={`${account.displayName} ${row.label}剩余比例`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={
                row.percent !== null ? Math.round(row.percent) : undefined
              }
              aria-valuetext={
                row.percent !== null
                  ? `剩余 ${Math.round(row.percent)}%`
                  : '剩余比例未知'
              }
            >
              {row.percent !== null ? (
                <span
                  className={styles.barFill}
                  style={{
                    width: `${Math.min(100, Math.max(0, row.percent))}%`,
                  }}
                />
              ) : null}
            </div>
            <span className={styles.meta} title={row.meta.join(' · ')}>
              {row.meta.join(' · ')}
            </span>
          </div>
        ))
      ) : (
        <p className={styles.note}>
          {refreshing
            ? '正在查询额度…'
            : failure
              ? '尚未获取额度'
              : '当前账号未返回额度数据'}
        </p>
      )}

      {failure ? (
        <p className={styles.errorLine} title={failure.message}>
          刷新失败：{failure.message}
        </p>
      ) : null}
    </div>
  );
}

/** 中转站的紧凑摘要：只保留密钥与账户两个金额口径。 */
function relaySummary(quota: KeyQuota): string {
  const parts: string[] = [];

  if (quota.mode === 'key') {
    const key = quota.unlimited
      ? '不限额'
      : quota.remainingUsd !== null
        ? formatRelayMoney(quota.remainingUsd, quota.currency)
        : null;
    if (key) parts.push(`密钥 ${key}`);
  }

  const accountLabel = quota.account
    ? '账户'
    : quota.mode === 'wallet'
      ? '钱包'
      : quota.mode === 'subscription'
        ? '订阅'
        : null;
  const accountValue = quota.account
    ? quota.account.remainingUsd !== null
      ? formatRelayMoney(quota.account.remainingUsd, quota.currency)
      : null
    : quota.unlimited
      ? '不限额'
      : quota.remainingUsd !== null
        ? formatRelayMoney(quota.remainingUsd, quota.currency)
        : null;
  if (accountLabel && accountValue) {
    parts.push(`${accountLabel} ${accountValue}`);
  }

  return parts.join(' · ') || '额度未提供';
}

function relayMeter(quota: KeyQuota): number | null {
  if (quota.account) {
    return relayPercent(quota.account.remainingUsd, quota.account.totalUsd);
  }
  if (quota.mode === 'key') {
    return relayPercent(quota.remainingUsd, quota.limitUsd);
  }
  return null;
}

function RelayBlock({
  account,
  threshold,
  lowQuotaEnabled,
  refreshing,
}: {
  account: RelayAccount;
  threshold: number;
  lowQuotaEnabled: boolean;
  refreshing: boolean;
}) {
  const quota = account.quota;
  const name = account.label ?? relayHost(account.baseUrl);
  const percent = quota ? relayMeter(quota) : null;
  const low =
    lowQuotaEnabled && percent !== null && percent <= threshold;

  return (
    <div className={styles.account}>
      <div className={styles.accountHead}>
        <span className={styles.accountName} title={account.baseUrl}>
          {name}
        </span>
        <span className={styles.accountTags}>
          <span className={styles.tag}>
            {RELAY_PROVIDER_LABELS[account.provider]}
          </span>
          {quota && !quota.valid ? (
            <span className={styles.authBadge}>密钥不可用</span>
          ) : null}
        </span>
      </div>

      {quota ? (
        <>
          <p className={styles.relaySummary}>{relaySummary(quota)}</p>
          {percent !== null ? (
            <div className={styles.relayMeterRow}>
              <div
                className={styles.bar}
                data-low={low || undefined}
                role="progressbar"
                aria-label={`${name}剩余额度比例`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={percent}
                aria-valuetext={`剩余 ${percent}%`}
              >
                <span
                  className={styles.barFill}
                  style={{ width: `${percent}%` }}
                />
              </div>
              <span
                className={styles.percent}
                data-low={low || undefined}
              >
                {percent}%
              </span>
            </div>
          ) : null}
        </>
      ) : (
        <p className={styles.note}>
          {refreshing ? '正在查询额度…' : '尚未获取额度'}
        </p>
      )}

      {account.lastError ? (
        <p className={styles.errorLine} title={account.lastError}>
          刷新失败：{account.lastError}
        </p>
      ) : null}
    </div>
  );
}

function latestUpdate(
  accounts: AccountState[],
  relays: RelayAccount[],
): number | null {
  const epochs = [
    ...accounts.flatMap((account) =>
      account.products.flatMap((product) =>
        product.snapshot ? [product.snapshot.checkedAtEpoch] : [],
      ),
    ),
    ...relays.flatMap((account) =>
      account.checkedAtEpoch !== null ? [account.checkedAtEpoch] : [],
    ),
  ];
  return epochs.length > 0 ? Math.max(...epochs) : null;
}

export default function TrayPanel() {
  const { state, ready, desktop, error, refresh } = useAppState();
  const {
    accounts: relayAccounts,
    loaded: relayLoaded,
    error: relayError,
    refreshingIds,
    ensureLoaded,
    refreshDue,
    refresh: refreshRelay,
  } = useRelay();
  const [refreshingAll, setRefreshingAll] = useState(false);
  // 面板在应用启动时就已经加载，但只有真正打开过才查询中转站额度。
  const [opened, setOpened] = useState(false);

  const threshold = state.preferences.lowQuotaThreshold;
  const lowQuotaEnabled = state.preferences.lowQuotaEnabled;
  const refreshMinutes = state.preferences.refreshMinutes;
  const groups = groupAccountsByPlatform(state.accounts);
  const updated = latestUpdate(state.accounts, relayAccounts);
  const busy =
    refreshingAll ||
    refreshingIds.size > 0 ||
    state.accounts.some((account) =>
      account.products.some((product) => product.query.refreshing),
    );

  // 打开面板时才读取中转站账户；面板常驻内存，事件监听只注册一次。
  const ensureLoadedRef = useRef(ensureLoaded);
  const refreshDueRef = useRef(refreshDue);
  const refreshMinutesRef = useRef(refreshMinutes);
  ensureLoadedRef.current = ensureLoaded;
  refreshDueRef.current = refreshDue;
  refreshMinutesRef.current = refreshMinutes;

  useEffect(() => {
    if (!desktop) return;
    ensureLoadedRef.current();

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onTrayPanelShown(() => {
      setOpened(true);
      ensureLoadedRef.current();
      // 先展示缓存，再按刷新间隔补查；不等待网络完成。
      void refreshDueRef.current(refreshMinutesRef.current);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  // 首次打开面板时中转站列表可能还没读完；读完后补一次过期检查。
  useEffect(() => {
    if (!desktop || !opened || !relayLoaded) return;
    void refreshDue(refreshMinutes);
  }, [desktop, opened, relayLoaded, refreshMinutes, refreshDue]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') void hideTrayPanel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  // 主题是前端本地偏好；主窗口切换后通过事件同步到面板。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onThemeChanged((theme) => {
      if (THEME_OPTIONS.includes(theme as ThemePref)) {
        applyTheme(theme as ThemePref);
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  const refreshAll = useCallback(async () => {
    if (!desktop || refreshingAll) return;
    setRefreshingAll(true);
    try {
      await Promise.all([
        refresh(),
        ...relayAccounts.map((account) => refreshRelay(account.id)),
      ]);
    } finally {
      setRefreshingAll(false);
    }
  }, [desktop, refreshingAll, refresh, relayAccounts, refreshRelay]);

  const hasAnything =
    state.accounts.length > 0 || relayAccounts.length > 0;

  return (
    <div className={styles.panel}>
      <header className={styles.header}>
        <div className={styles.brand}>
          <img className={styles.wordmark} src={wordmark} alt="Token Refill" />
          <span className={styles.updated}>
            {updated !== null
              ? `更新于 ${clockLabel(updated)}`
              : ready
                ? '尚未获取额度'
                : '正在读取…'}
            {busy ? ' · 刷新中' : ''}
          </span>
        </div>
        <button
          type="button"
          className={styles.iconButton}
          onClick={() => void refreshAll()}
          disabled={!desktop || busy}
          aria-label="刷新全部额度"
          title={desktop ? '刷新全部额度' : '浏览器预览无法调用桌面接口'}
        >
          <ArrowClockwise
            size={15}
            className={busy ? styles.spin : undefined}
          />
        </button>
      </header>

      <div className={styles.scroll}>
        {error ? (
          <p className={styles.notice} role="alert">
            {error}
          </p>
        ) : null}
        {relayError ? (
          <p className={styles.notice} role="alert">
            {relayError}
          </p>
        ) : null}

        {!desktop ? (
          <p className={styles.note}>
            浏览器预览无法调用桌面接口，请在应用内查看。
          </p>
        ) : !ready ? (
          <p className={styles.note}>正在读取账户与额度…</p>
        ) : !hasAnything ? (
          <div className={styles.empty}>
            <span className={styles.emptyGlyph} aria-hidden="true">
              <Gauge size={22} weight="duotone" />
            </span>
            <p className={styles.emptyTitle}>还没有可查看的额度</p>
            <p className={styles.note}>打开主窗口添加账户后，这里会显示额度。</p>
          </div>
        ) : (
          <>
            {groups.map((group) => (
              <section className={styles.section} key={group.platform}>
                <div className={styles.sectionHead}>
                  <span className={styles.platformBadge} aria-hidden="true">
                    <PlatformLogo
                      platform={group.platform}
                      fallback={group.meta.mark}
                    />
                  </span>
                  <span className={styles.sectionName}>
                    {platformMeta(group.platform).name}
                  </span>
                  <span className={styles.sectionCount}>
                    {group.accounts.length} 个账户
                  </span>
                </div>
                {group.accounts.map((account) => (
                  <AccountBlock
                    key={account.accountId}
                    account={account}
                    threshold={threshold}
                    lowQuotaEnabled={lowQuotaEnabled}
                  />
                ))}
              </section>
            ))}

            {relayLoaded && relayAccounts.length > 0 ? (
              <section className={styles.section}>
                <div className={styles.sectionHead}>
                  <span className={styles.sectionName}>中转站</span>
                  <span className={styles.sectionCount}>
                    {relayAccounts.length} 个账户
                  </span>
                </div>
                {relayAccounts.map((account) => (
                  <RelayBlock
                    key={account.id}
                    account={account}
                    threshold={threshold}
                    lowQuotaEnabled={lowQuotaEnabled}
                    refreshing={refreshingIds.has(account.id)}
                  />
                ))}
              </section>
            ) : null}
          </>
        )}
      </div>

      <footer className={styles.footer}>
        <span className={styles.version}>v{appVersion}</span>
        <div className={styles.footerActions}>
          <button
            type="button"
            className={styles.primaryButton}
            onClick={() => void openMainWindow()}
          >
            <ArrowUpRight size={13} weight="bold" aria-hidden />
            打开主窗口
          </button>
          <button
            type="button"
            className={styles.iconButton}
            onClick={() => void quitApp()}
            aria-label="退出 Token Refill"
            title="退出 Token Refill"
          >
            <Power size={15} />
          </button>
        </div>
      </footer>
    </div>
  );
}