import { Fragment, useEffect, useState } from 'react';
import { ArrowClockwiseIcon, PlugsIcon } from '@phosphor-icons/react';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import type { KeyQuota, KeyQuotaWindow, RelayAccount } from '../lib/desktop';
import { clockLabel, resetLabel } from '../lib/format';
import { useRelay } from '../lib/relay';
import {
  RELAY_MODE_LABELS,
  RELAY_ORIGIN_LABELS,
  RELAY_PROVIDER_LABELS,
  formatRelayMoney,
  relayExpiryText,
  relayHost,
  relayPercent,
  relayWindowLabel,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import ChevronIcon from './ChevronIcon';
import styles from '../App.module.css';

/** 两列额度各自需要的展示数据；未知与「未提供」不折算成 0。 */
interface QuotaCell {
  label: string;
  caption: string | null;
  value: string;
  muted: boolean;
  note: string | null;
  percent: number | null;
  windows: KeyQuotaWindow[];
  windowsTitle: string;
}

function joinNote(...parts: (string | null)[]): string {
  return parts.filter(Boolean).join(' · ');
}

/** 「已用 / 总额」只在对应字段存在时出现，缺失的一侧不猜测。 */
function moneyNote(
  used: number | null,
  total: number | null,
  currency: string | null,
): string | null {
  if (used !== null && total !== null) {
    return `已用 ${formatRelayMoney(used, currency)} / 总额 ${formatRelayMoney(total, currency)}`;
  }
  if (used !== null) return `已用 ${formatRelayMoney(used, currency)}`;
  if (total !== null) return `总额 ${formatRelayMoney(total, currency)}`;
  return null;
}

function moneyValue(
  remaining: number | null,
  currency: string | null,
): Pick<QuotaCell, 'value' | 'muted'> {
  return remaining === null
    ? { value: '未提供', muted: true }
    : { value: formatRelayMoney(remaining, currency), muted: false };
}

/** 密钥自身额度：只有 key 口径才有金额，其余口径明确写「未提供」。 */
function keyCell(quota: KeyQuota): QuotaCell {
  const expiry = `到期时间：${relayExpiryText(quota.expiresAtEpoch)}`;
  const base = {
    label: '密钥额度',
    caption: null,
    windowsTitle: '限速窗口',
  };

  if (quota.mode !== 'key') {
    return {
      ...base,
      value: '未提供',
      muted: true,
      note: joinNote('当前来源未返回', expiry),
      percent: null,
      windows: [],
    };
  }

  if (quota.unlimited) {
    return {
      ...base,
      value: '不限额',
      muted: false,
      note: joinNote('未设置上限', expiry),
      percent: null,
      windows: quota.windows,
    };
  }

  return {
    ...base,
    ...moneyValue(quota.remainingUsd, quota.currency),
    note: joinNote(
      quota.remainingUsd === null ? '当前来源未返回' : null,
      moneyNote(quota.usedUsd, quota.limitUsd, quota.currency),
      expiry,
    ),
    percent: relayPercent(quota.remainingUsd, quota.limitUsd),
    windows: quota.windows,
  };
}

/**
 * 账户额度：优先展示账户级数据；Sub2API 的钱包 / 订阅口径把账户金额放在
 * 密钥快照本身，没有单独的 account 字段，按 mode 落到这里。
 */
function accountCell(quota: KeyQuota): QuotaCell {
  const expiry =
    quota.expiresAtEpoch === null
      ? null
      : `到期时间：${relayExpiryText(quota.expiresAtEpoch)}`;
  const base = {
    label: '账户额度',
    windowsTitle: '订阅窗口',
  };

  if (quota.account) {
    const account = quota.account;
    return {
      ...base,
      caption:
        account.source === 'access-token'
          ? '账户余额 · 访问令牌'
          : '账户余额',
      ...moneyValue(account.remainingUsd, quota.currency),
      note: moneyNote(account.usedUsd, account.totalUsd, quota.currency),
      percent: relayPercent(account.remainingUsd, account.totalUsd),
      windows: account.windows,
    };
  }

  switch (quota.mode) {
    case 'wallet':
      return {
        ...base,
        caption: '钱包余额',
        ...moneyValue(quota.remainingUsd, quota.currency),
        note:
          quota.remainingUsd === null ? '当前来源未返回钱包余额' : null,
        percent: null,
        windows: [],
      };
    case 'subscription':
      return {
        ...base,
        caption: '订阅额度',
        value: quota.unlimited
          ? '不限额'
          : moneyValue(quota.remainingUsd, quota.currency).value,
        muted: !quota.unlimited && quota.remainingUsd === null,
        note: joinNote(
          quota.unlimited ? '平台未设置总额度上限' : null,
          expiry,
        ),
        percent: null,
        windows: quota.windows,
      };
    case 'unrestricted':
      return {
        ...base,
        caption: '账户余额',
        value: quota.unlimited
          ? '不限额'
          : moneyValue(quota.remainingUsd, quota.currency).value,
        muted: !quota.unlimited && quota.remainingUsd === null,
        note: joinNote(
          quota.unlimited ? '平台未设置总额度上限' : null,
          expiry,
        ),
        percent: null,
        windows: [],
      };
    default:
      return {
        ...base,
        caption: '账户余额',
        value: '未提供',
        muted: true,
        note: '当前来源未返回账户额度',
        percent: null,
        windows: [],
      };
  }
}

/**
 * 收起后的简易额度：只保留两列金额，与订阅额度的折叠摘要保持同一密度。
 * 「未提供 / 不限额」直接写状态，不补 0。
 */
function relayQuotaSummary(quota: KeyQuota): string {
  const key = keyCell(quota);
  const account = accountCell(quota);
  const compact = (label: string, cell: QuotaCell): string => {
    if (cell.value === '未提供') return `${label}未提供`;
    if (cell.value === '不限额') return `${label}不限额`;
    return `${label} ${cell.value}`;
  };
  const accountLabel = quota.account
    ? '账户'
    : quota.mode === 'wallet'
      ? '钱包'
      : quota.mode === 'subscription'
        ? '订阅'
        : '账户';
  return `${compact('密钥', key)} · ${compact(accountLabel, account)}`;
}

function WindowList({
  windows,
  title,
  threshold,
  lowQuotaEnabled,
  currency,
}: {
  windows: KeyQuotaWindow[];
  title: string;
  threshold: number;
  lowQuotaEnabled: boolean;
  currency: string | null;
}) {
  if (windows.length === 0) return null;
  return (
    <div className={styles.relayWindows}>
      <p className={styles.relaySectionLabel}>{title}</p>
      {windows.map((item) => {
        const low =
          lowQuotaEnabled
          && item.remainingPercent !== null
          && item.remainingPercent <= threshold;
        return (
          <div className={styles.relayWindow} key={item.windowId}>
            <div className={styles.relayWindowHead}>
              <span>{relayWindowLabel(item)}</span>
              <span className={styles.quotaMeta}>
                {item.limitUsd !== null
                  ? `${formatRelayMoney(item.remainingUsd, currency)} / ${formatRelayMoney(item.limitUsd, currency)}`
                  : formatRelayMoney(item.usedUsd, currency)}
                {item.remainingPercent !== null
                  ? ` · 剩余 ${item.remainingPercent}%`
                  : ''}
              </span>
            </div>
            {item.remainingPercent !== null ? (
              <div
                className={styles.relayBar}
                data-low={low || undefined}
                role="progressbar"
                aria-label={`${relayWindowLabel(item)}剩余额度比例`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={item.remainingPercent}
              >
                <span
                  className={styles.relayBarFill}
                  style={{ width: `${item.remainingPercent}%` }}
                />
              </div>
            ) : null}
            <span className={styles.quotaMeta}>
              {resetLabel(item.resetsAtEpoch)}
              {item.usedUsd !== null
                ? ` · 已用 ${formatRelayMoney(item.usedUsd, currency)}`
                : ''}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** 剩余比例条：数值贴右，低于提醒阈值时转危险色。 */
function QuotaMeter({
  percent,
  threshold,
  lowQuotaEnabled,
  label,
}: {
  percent: number;
  threshold: number;
  lowQuotaEnabled: boolean;
  label: string;
}) {
  const low = lowQuotaEnabled && percent <= threshold;
  return (
    <>
      <div className={styles.relayMeterRow}>
        <div
          className={styles.relayBar}
          data-low={low || undefined}
          role="progressbar"
          aria-label={`${label}剩余比例`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          aria-valuetext={`剩余 ${percent}%`}
        >
          <span
            className={styles.relayBarFill}
            style={{ width: `${percent}%` }}
          />
        </div>
        <span className={styles.relayMeterValue}>{percent}%</span>
      </div>
      {low ? (
        <span className={styles.quotaLowNote}>
          低于提醒阈值 {threshold}%
        </span>
      ) : null}
    </>
  );
}

function QuotaCellView({
  cell,
  threshold,
  lowQuotaEnabled,
  currency,
}: {
  cell: QuotaCell;
  threshold: number;
  lowQuotaEnabled: boolean;
  currency: string | null;
}) {
  return (
    <section className={styles.relayQuotaCell} aria-label={cell.label}>
      <p className={styles.relayQuotaLabel}>{cell.label}</p>
      {cell.caption ? (
        <p className={styles.relayQuotaCaption}>{cell.caption}</p>
      ) : null}
      <p
        className={styles.relayAmount}
        data-muted={cell.muted || undefined}
      >
        {cell.value}
      </p>
      {cell.percent !== null ? (
        <QuotaMeter
          percent={cell.percent}
          threshold={threshold}
          lowQuotaEnabled={lowQuotaEnabled}
          label={cell.caption ?? cell.label}
        />
      ) : null}
      {cell.note ? <p className={styles.quotaMeta}>{cell.note}</p> : null}
      <WindowList
        windows={cell.windows}
        title={cell.windowsTitle}
        threshold={threshold}
        lowQuotaEnabled={lowQuotaEnabled}
        currency={currency}
      />
    </section>
  );
}

function RelayQuotaCard({
  account,
  desktop,
  refreshing,
  open,
  threshold,
  lowQuotaEnabled,
  onToggle,
  onRefresh,
}: {
  account: RelayAccount;
  desktop: boolean;
  refreshing: boolean;
  open: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  onToggle: () => void;
  onRefresh: () => void;
}) {
  const quota = account.quota;
  const host = relayHost(account.baseUrl);
  const reducedMotion = useReducedMotion();
  const bodyId = `relay-quota-${account.id}`;
  const summary = quota ? relayQuotaSummary(quota) : null;
  // 来源、额度口径、密钥名与套餐名可能重复（如站点把套餐名也写成「钱包余额」），
  // 只保留第一次出现的文案。
  const metaParts = [
    RELAY_ORIGIN_LABELS[account.origin],
    quota && quota.mode !== 'key' ? RELAY_MODE_LABELS[quota.mode] : null,
    quota?.keyName ?? null,
    quota?.planName ?? null,
  ].filter(
    (part, index, list): part is string =>
      Boolean(part) && list.indexOf(part) === index,
  );

  return (
    <article className={styles.accountCard} aria-live="polite">
      <header className={styles.accountHead}>
        <div className={styles.accountTitleBlock}>
          <div className={styles.accountNameRow}>
            <h3 className={styles.accountName} title={account.baseUrl}>
              {host}
            </h3>
            {/* 平台类型用全局中性标签，和账户名同排；来源与额度口径留在下一行。 */}
            <span className={styles.planTag}>
              {RELAY_PROVIDER_LABELS[account.provider]}
            </span>
          </div>
          <p className={styles.accountMetaLine}>
            {metaParts.map((part, index) => (
              <Fragment key={`${part}-${index}`}>
                {index > 0 ? (
                  <span className={styles.metaDot} aria-hidden="true">
                    ·
                  </span>
                ) : null}
                <span className={styles.accountHint} title={part}>
                  {part}
                </span>
              </Fragment>
            ))}
            {quota && !quota.valid ? (
              <span className={styles.authStatus} data-state="needs-reauth">
                <span className={styles.authDot} aria-hidden="true" />
                密钥不可用
              </span>
            ) : null}
          </p>
        </div>
        {!open && summary ? (
          <span className={styles.quotaSummary} title={summary}>
            {summary}
          </span>
        ) : null}
        <div className={styles.accountTools}>
          {/* 与订阅额度一致：更新时间只在展开时出现，收起后右侧只剩刷新和箭头 */}
          {open && account.checkedAtEpoch !== null ? (
            <span className={styles.updatedAt}>
              更新于 {clockLabel(account.checkedAtEpoch)}
              {refreshing ? ' · 刷新中' : ''}
            </span>
          ) : null}
          <button
            type="button"
            className={styles.refreshButton}
            onClick={onRefresh}
            disabled={!desktop || refreshing}
            aria-label={`刷新 ${host} 的额度`}
            title={desktop ? '重新识别并刷新额度' : '浏览器预览无法调用 Rust'}
          >
            <ArrowClockwiseIcon
              size={16}
              weight="regular"
              className={refreshing ? styles.spin : undefined}
            />
          </button>
          {quota ? (
            <button
              type="button"
              className={styles.chevronButton}
              onClick={onToggle}
              aria-expanded={open}
              aria-controls={bodyId}
              aria-label={open ? `收起 ${host} 的额度` : `展开 ${host} 的额度`}
              title={open ? '收起额度' : '展开额度'}
            >
              <ChevronIcon direction={open ? 'down' : 'right'} />
            </button>
          ) : null}
        </div>
      </header>

      {account.lastError ? (
        <p className={styles.noticeStale} role="status">
          上次刷新失败：{account.lastError}
          {account.checkedAtEpoch !== null
            ? ' 当前显示上一次成功的数据。'
            : null}
        </p>
      ) : null}

      {!quota ? (
        <p className={styles.accountNote}>
          {refreshing
            ? '正在识别上游并查询额度…'
            : '尚未获取额度。可在“中转站”页检查密钥，或为 New API 账户补充访问令牌。'}
        </p>
      ) : (
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
                <div className={styles.relayQuotaGrid}>
                  <QuotaCellView
                    cell={keyCell(quota)}
                    threshold={threshold}
                    lowQuotaEnabled={lowQuotaEnabled}
                    currency={quota.currency}
                  />
                  <QuotaCellView
                    cell={accountCell(quota)}
                    threshold={threshold}
                    lowQuotaEnabled={lowQuotaEnabled}
                    currency={quota.currency}
                  />
                </div>
                {quota.accountError ? (
                  <p className={styles.accountNote}>{quota.accountError}</p>
                ) : null}
              </div>
            </m.div>
          ) : null}
        </AnimatePresence>
      )}
    </article>
  );
}

/** 总览的「中转站账户额度」标签页：只读展示额度，操作入口留在中转站管理页。 */
export default function RelayQuotaBoard({ desktop }: { desktop: boolean }) {
  const {
    accounts,
    scanning,
    loaded,
    error,
    refreshingId,
    refresh,
    ensureLoaded,
  } = useRelay();
  const { state } = useAppState();
  const threshold = state.preferences.lowQuotaThreshold;
  const lowQuotaEnabled = state.preferences.lowQuotaEnabled;
  // 默认全部展开；只记录被收起的账户，刷新列表时保留用户的折叠选择。
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());

  // 只在真正切到该标签页时扫描，避免启动或停留在订阅额度时额外查询中转站。
  useEffect(() => {
    if (desktop) ensureLoaded();
  }, [desktop, ensureLoaded]);

  const toggle = (id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  if (!desktop) {
    return (
      <div className={styles.overviewEmpty}>
        <span className={styles.emptyGlyph} aria-hidden="true">
          <PlugsIcon size={24} weight="duotone" />
        </span>
        <h2 className={styles.emptyTitle}>浏览器预览无法读取中转站额度</h2>
        <p className={styles.usageNote}>
          请在桌面应用内查看（
          <code className={styles.codeChip}>pnpm tauri dev</code>）。
        </p>
      </div>
    );
  }

  return (
    <div className={styles.relayQuotaView}>
      {error ? (
        <p
          className={`${styles.noticeDanger} ${styles.globalNotice}`}
          role="alert"
        >
          {error}
        </p>
      ) : null}

      {accounts.length === 0 ? (
        <div className={styles.overviewEmpty}>
          <span className={styles.emptyGlyph} aria-hidden="true">
            <PlugsIcon size={24} weight="duotone" />
          </span>
          <h2 className={styles.emptyTitle}>还没有可查看的中转站额度</h2>
          <p className={styles.usageNote}>
            {!loaded || scanning
              ? '正在扫描 Claude Code 与 Codex 的本地配置，并查询中转站额度…'
              : '在“中转站”页添加账户或扫描本地配置后，这里会显示额度、窗口与到期时间。'}
          </p>
        </div>
      ) : (
        <>
          {/* 与订阅额度的平台分组共用标题样式：小尺寸图形、名称 + 说明、右侧计数。 */}
          <div className={styles.platformSectionHead}>
            <div className={styles.platformHeading}>
              <span
                className={styles.platformBadge}
                data-active
                aria-hidden="true"
              >
                <PlugsIcon size={20} weight="duotone" />
              </span>
              <div className={styles.platformIdentity}>
                <div className={styles.platformNameRow}>
                  <h2 className={styles.platformName}>中转站账户</h2>
                </div>
                <p className={styles.platformDescription}>
                  账户余额与密钥额度
                </p>
              </div>
              <div className={styles.platformAside}>
                <span className={styles.platformCount}>
                  {accounts.length} 个账户
                </span>
              </div>
            </div>
          </div>
          <div className={styles.accountStack}>
            {accounts.map((account) => (
              <RelayQuotaCard
                key={account.id}
                account={account}
                desktop={desktop}
                refreshing={refreshingId === account.id}
                open={!collapsed.has(account.id)}
                threshold={threshold}
                lowQuotaEnabled={lowQuotaEnabled}
                onToggle={() => toggle(account.id)}
                onRefresh={() => void refresh(account.id)}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}