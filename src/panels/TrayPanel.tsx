import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  ArrowClockwise,
  ArrowSquareOut,
  Gauge,
  PlugsIcon,
  Power,
} from '@phosphor-icons/react';
import { Reorder, useDragControls, type DragControls } from 'motion/react';
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
  groupLabel,
  planTierLabel,
  quotaWindowLabel,
  relativeUpdateLabel,
  resetLabel,
  stampLabel,
} from '../lib/format';
import { platformMeta, type AccountGroup } from '../lib/platforms';
import {
  RELAY_SECTION_KEY,
  useOverviewOrder,
  useRelayOrder,
} from '../lib/useOrdering';
import { useRelay } from '../lib/relay';
import {
  RELAY_PROVIDER_LABELS,
  formatRelayMoney,
  relayHost,
  relayPercent,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import ChevronIcon from '../components/ChevronIcon';
import PlatformLogo from '../components/PlatformLogo';
import ReorderHandle from '../components/ReorderHandle';
import styles from './TrayPanel.module.css';

interface QuotaRow {
  key: string;
  label: string;
  groupName: string | null;
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

/** 账户最近一次成功查询的时间，用于头部「刚刚更新 / N 分钟前」。 */
function accountUpdated(account: AccountState): number | null {
  const epochs = account.products.flatMap((product) =>
    product.snapshot ? [product.snapshot.checkedAtEpoch] : [],
  );
  return epochs.length > 0 ? Math.max(...epochs) : null;
}

function accountRows(
  account: AccountState,
  threshold: number,
  lowQuotaEnabled: boolean,
): QuotaRow[] {
  const multipleProducts = account.products.length > 1;
  const rows: QuotaRow[] = [];

  account.products.forEach((product) => {
    // 多套餐账户（如百炼）在每类套餐的第一行标注套餐名；单套餐多分组账户（如 Antigravity）在每组第一行标注组名。
    let productLead = multipleProducts;
    const groups = (product.snapshot?.groups ?? []).filter(
      (group) => group.windows.length > 0,
    );
    const multipleGroups = groups.length > 1;
    groups.forEach((group, groupIndex) => {
      let groupLead = multipleGroups;
      group.windows.forEach((window) => {
        const percent = window.remainingPercent;
        const currentGroup = groupLabel(group.limitName);
        const eyebrowParts = [
          productLead ? product.displayName : null,
          groupLead ? currentGroup : null,
        ].filter(Boolean);
        rows.push({
          key: `${product.productId}-${groupIndex}-${window.windowId}`,
          label: quotaWindowLabel(window.windowDurationMins, window.windowName),
          groupName: currentGroup,
          eyebrow: eyebrowParts.length > 0 ? eyebrowParts.join(' · ') : null,
          percent,
          meta: [amountText(window), resetLabel(window.resetsAtEpoch)].filter(
            (part): part is string => part !== null,
          ),
          low: lowQuotaEnabled && percent !== null && percent <= threshold,
        });
        productLead = false;
        groupLead = false;
      });
    });
  });

  return rows;
}

function QuotaMeter({
  account,
  row,
}: {
  account: AccountState;
  row: QuotaRow;
}) {
  const fullLabel = row.groupName ? `${row.groupName} · ${row.label}` : row.label;
  return (
    <div className={styles.quotaRow} data-low={row.low || undefined}>
      <span className={styles.quotaLabel} title={fullLabel}>
        {row.label}
      </span>
      <div
        className={styles.bar}
        role="progressbar"
        aria-label={`${account.displayName} ${fullLabel}剩余比例`}
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
            style={{ width: `${Math.min(100, Math.max(0, row.percent))}%` }}
          />
        ) : null}
      </div>
      <span
        className={styles.percent}
        data-low={row.low || undefined}
        data-unknown={row.percent === null || undefined}
      >
        {row.percent !== null ? `剩余 ${Math.round(row.percent)}%` : '剩余 —'}
      </span>
      {row.meta.length > 0 ? (
        <span className={styles.meta} title={row.meta.join(' · ')}>
          {row.meta.join(' · ')}
        </span>
      ) : null}
    </div>
  );
}

interface AccountCardProps {
  account: AccountState;
  threshold: number;
  lowQuotaEnabled: boolean;
  canRefresh: boolean;
  onRefresh: (accountId: string) => void;
  /** 同平台存在多个账户时才提供排序手柄。 */
  showHandle: boolean;
  onMove: (delta: -1 | 1) => void;
}

function AccountCard({
  account,
  threshold,
  lowQuotaEnabled,
  canRefresh,
  onRefresh,
  showHandle,
  onMove,
  dragControls,
}: AccountCardProps & { dragControls: DragControls }) {
  const rows = accountRows(account, threshold, lowQuotaEnabled);
  const tier = accountPlanTier(account);
  const busy = account.products.some((product) => product.query.refreshing);
  const failure = account.products.find(
    (product) => product.query.lastError !== null,
  )?.query.lastError;

  return (
    <Reorder.Item
      as="article"
      value={account.accountId}
      className={styles.account}
      dragListener={false}
      dragControls={dragControls}
      layout="position"
      tabIndex={-1}
    >
      <div className={styles.accountHead}>
        <span className={styles.accountName} title={account.displayName}>
          {account.displayName}
        </span>
        {account.authState === 'needs-reauth' ? (
          <span className={styles.authBadge}>重新授权</span>
        ) : null}
        <span className={styles.accountTime}>
          {relativeUpdateLabel(accountUpdated(account))}
        </span>
        <button
          type="button"
          className={styles.refreshButton}
          onClick={() => onRefresh(account.accountId)}
          disabled={!canRefresh || busy}
          aria-label={`刷新 ${account.displayName} 的额度`}
          title={canRefresh ? '刷新额度' : '浏览器预览无法调用桌面接口'}
        >
          <ArrowClockwise size={13} className={busy ? styles.spin : undefined} />
        </button>
        {showHandle ? (
          <ReorderHandle
            label={`调整 ${account.displayName} 的显示顺序`}
            controls={dragControls}
            onMove={onMove}
          />
        ) : null}
      </div>

      {account.identityHint ? (
        <span className={styles.accountHint} title={account.identityHint}>
          {account.identityHint}
        </span>
      ) : null}
      {tier ? <span className={styles.tier}>{planTierLabel(tier)}</span> : null}

      {rows.length > 0 ? (
        rows.map((row) => (
          <div className={styles.quotaGroup} key={row.key}>
            {row.eyebrow ? (
              <span className={styles.eyebrow}>{row.eyebrow}</span>
            ) : null}
            <QuotaMeter account={account} row={row} />
          </div>
        ))
      ) : (
        <p className={styles.note}>
          {busy
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
    </Reorder.Item>
  );
}

/** 每张卡自持拖动控制，避免列表内多个手柄互相触发。 */
function SortableAccountCard(props: AccountCardProps) {
  const dragControls = useDragControls();
  return <AccountCard {...props} dragControls={dragControls} />;
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

interface RelayCardProps {
  account: RelayAccount;
  threshold: number;
  lowQuotaEnabled: boolean;
  refreshing: boolean;
  canRefresh: boolean;
  onRefresh: (id: string) => void;
  /** 同列表存在多个账户时才提供排序手柄。 */
  showHandle: boolean;
  onMove: (delta: -1 | 1) => void;
}

function RelayCard({
  account,
  threshold,
  lowQuotaEnabled,
  refreshing,
  canRefresh,
  onRefresh,
  showHandle,
  onMove,
  dragControls,
}: RelayCardProps & { dragControls: DragControls }) {
  const quota = account.quota;
  const name = account.label ?? relayHost(account.baseUrl);
  const percent = quota ? relayMeter(quota) : null;
  const low = lowQuotaEnabled && percent !== null && percent <= threshold;

  return (
    <Reorder.Item
      as="article"
      value={account.id}
      className={styles.account}
      dragListener={false}
      dragControls={dragControls}
      layout="position"
      tabIndex={-1}
    >
      <div className={styles.accountHead}>
        <span className={styles.accountName} title={account.baseUrl}>
          {name}
        </span>
        {quota && !quota.valid ? (
          <span className={styles.authBadge}>密钥不可用</span>
        ) : null}
        <span className={styles.accountTime}>
          {relativeUpdateLabel(account.checkedAtEpoch)}
        </span>
        <button
          type="button"
          className={styles.refreshButton}
          onClick={() => onRefresh(account.id)}
          disabled={!canRefresh || refreshing}
          aria-label={`刷新 ${name} 的额度`}
          title={canRefresh ? '刷新额度' : '浏览器预览无法调用桌面接口'}
        >
          <ArrowClockwise
            size={13}
            className={refreshing ? styles.spin : undefined}
          />
        </button>
        {showHandle ? (
          <ReorderHandle
            label={`调整 ${name} 的显示顺序`}
            controls={dragControls}
            onMove={onMove}
          />
        ) : null}
      </div>

      <span className={styles.accountHint}>
        {RELAY_PROVIDER_LABELS[account.provider]} · {relayHost(account.baseUrl)}
      </span>

      {quota ? (
        <>
          <p className={styles.relaySummary}>{relaySummary(quota)}</p>
          {percent !== null ? (
            <div className={styles.relayMeter} data-low={low || undefined}>
              <div
                className={styles.bar}
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
              <span className={styles.percent} data-low={low || undefined}>
                剩余 {percent}%
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
    </Reorder.Item>
  );
}

/** 每张中转站卡自持拖动控制，使刷新按钮与手柄互不干扰。 */
function SortableRelayCard(props: RelayCardProps) {
  const dragControls = useDragControls();
  return <RelayCard {...props} dragControls={dragControls} />;
}

/**
 * 中转站分组和平台分组并列，同样可以整组拖动；
 * 分组标识 `relay` 与平台键一起写进保存顺序。
 */
function RelaySection({
  accounts,
  open,
  onToggle,
  sortable,
  threshold,
  lowQuotaEnabled,
  refreshingIds,
  canRefresh,
  onRefresh,
  onMoveSection,
  onMoveAccount,
  onReorderAccounts,
}: {
  accounts: RelayAccount[];
  open: boolean;
  onToggle: () => void;
  /** 与平台分组并列时才提供分组排序手柄。 */
  sortable: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  refreshingIds: ReadonlySet<string>;
  canRefresh: boolean;
  onRefresh: (id: string) => void;
  onMoveSection: (delta: -1 | 1) => void;
  onMoveAccount: (id: string, delta: -1 | 1) => void;
  onReorderAccounts: (ids: string[]) => void;
}) {
  const dragControls = useDragControls();
  const bodyId = 'tray-section-relay';

  return (
    <Reorder.Item
      as="section"
      value={RELAY_SECTION_KEY}
      className={styles.section}
      dragListener={false}
      dragControls={dragControls}
      layout="position"
      tabIndex={-1}
    >
      <div className={styles.sectionHeadRow}>
        <button
          type="button"
          className={styles.sectionHead}
          onClick={onToggle}
          aria-expanded={open}
          aria-controls={bodyId}
        >
          <span className={styles.platformBadge} aria-hidden="true">
            <PlugsIcon size={15} />
          </span>
          <span className={styles.sectionName}>中转站</span>
          <span className={styles.sectionCount}>
            · {accounts.length} 个账户
          </span>
        </button>
        {sortable ? (
          <ReorderHandle
            label="调整 中转站 分组的显示顺序"
            controls={dragControls}
            onMove={onMoveSection}
          />
        ) : null}
        <span
          className={styles.chevron}
          data-collapsed={!open || undefined}
          onClick={onToggle}
          aria-hidden="true"
          title={open ? '收起分组' : '展开分组'}
        >
          <ChevronIcon direction="down" />
        </span>
      </div>
      <SectionBody id={bodyId} open={open}>
        <Reorder.Group
          as="div"
          axis="y"
          values={accounts.map((account) => account.id)}
          onReorder={onReorderAccounts}
          className={styles.cards}
        >
          {accounts.map((account) => (
            <SortableRelayCard
              key={account.id}
              account={account}
              threshold={threshold}
              lowQuotaEnabled={lowQuotaEnabled}
              refreshing={refreshingIds.has(account.id)}
              canRefresh={canRefresh}
              onRefresh={onRefresh}
              showHandle={accounts.length > 1}
              onMove={(delta) => onMoveAccount(account.id, delta)}
            />
          ))}
        </Reorder.Group>
      </SectionBody>
    </Reorder.Item>
  );
}

/**
 * 分组内容始终挂载，靠 grid-template-rows 0fr ↔ 1fr 做展开/收起过渡。
 * 收起后由 visibility: hidden 移出 Tab 顺序与读屏树，不依赖 inert 的支持情况。
 */
function SectionBody({
  id,
  open,
  children,
}: {
  id: string;
  open: boolean;
  children: ReactNode;
}) {
  return (
    <div className={styles.cardsWrap} id={id} data-open={open || undefined}>
      {children}
    </div>
  );
}

/**
 * 平台分组本身是可拖动项；拖动只从标题右侧的手柄开始，
 * 折叠按钮与箭头仍然只负责展开/收起。
 */
function SortableSection({
  group,
  open,
  onToggle,
  sortable,
  threshold,
  lowQuotaEnabled,
  canRefresh,
  onRefresh,
  onMoveSection,
  onMoveAccount,
  onReorderAccounts,
}: {
  group: AccountGroup;
  open: boolean;
  onToggle: () => void;
  /** 多个平台分组时才提供平台排序手柄。 */
  sortable: boolean;
  threshold: number;
  lowQuotaEnabled: boolean;
  canRefresh: boolean;
  onRefresh: (accountId: string) => void;
  onMoveSection: (delta: -1 | 1) => void;
  onMoveAccount: (accountId: string, delta: -1 | 1) => void;
  onReorderAccounts: (accountIds: string[]) => void;
}) {
  const dragControls = useDragControls();
  const platformName = platformMeta(group.platform).name;
  const bodyId = `tray-section-${group.platform}`;
  const accountIds = group.accounts.map((account) => account.accountId);

  return (
    <Reorder.Item
      as="section"
      value={group.platform}
      className={styles.section}
      dragListener={false}
      dragControls={dragControls}
      layout="position"
      tabIndex={-1}
    >
      <div className={styles.sectionHeadRow}>
        <button
          type="button"
          className={styles.sectionHead}
          onClick={onToggle}
          aria-expanded={open}
          aria-controls={bodyId}
        >
          <span className={styles.platformBadge} aria-hidden="true">
            <PlatformLogo platform={group.platform} fallback={group.meta.mark} />
          </span>
          <span className={styles.sectionName}>{platformName}</span>
          <span className={styles.sectionCount}>
            · {group.accounts.length} 个账户
          </span>
        </button>
        {sortable ? (
          <ReorderHandle
            label={`调整 ${platformName} 账户类型的显示顺序`}
            controls={dragControls}
            onMove={onMoveSection}
          />
        ) : null}
        <span
          className={styles.chevron}
          data-collapsed={!open || undefined}
          onClick={onToggle}
          aria-hidden="true"
          title={open ? '收起分组' : '展开分组'}
        >
          <ChevronIcon direction="down" />
        </span>
      </div>
      <SectionBody id={bodyId} open={open}>
        <Reorder.Group
          as="div"
          axis="y"
          values={accountIds}
          onReorder={onReorderAccounts}
          className={styles.cards}
        >
          {group.accounts.map((account) => (
            <SortableAccountCard
              key={account.accountId}
              account={account}
              threshold={threshold}
              lowQuotaEnabled={lowQuotaEnabled}
              canRefresh={canRefresh}
              onRefresh={onRefresh}
              showHandle={group.accounts.length > 1}
              onMove={(delta) => onMoveAccount(account.accountId, delta)}
            />
          ))}
        </Reorder.Group>
      </SectionBody>
    </Reorder.Item>
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
  const { state, appInfo, ready, desktop, error, stateFileNotice, refresh, resync } =
    useAppState();
  const {
    accounts: relayAccounts,
    loaded: relayLoaded,
    error: relayError,
    refreshingIds,
    ensureLoaded,
    reload,
    refreshDue,
    refresh: refreshRelay,
  } = useRelay();
  const [refreshingAll, setRefreshingAll] = useState(false);
  // 平台分组可以折叠，只作为看过一眼后的收纳，不写进设置。
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  // 面板在应用启动时就已经加载，但只有真正打开过才查询中转站额度。
  const [opened, setOpened] = useState(false);

  const threshold = state.preferences.lowQuotaThreshold;
  const lowQuotaEnabled = state.preferences.lowQuotaEnabled;
  const refreshMinutes = state.preferences.refreshMinutes;
  // 中转站分组与平台分组并列，读完后才参与分组排序。
  const relayVisible = relayLoaded && relayAccounts.length > 0;
  const extraSections = useMemo(
    () => (relayVisible ? [RELAY_SECTION_KEY] : []),
    [relayVisible],
  );
  // 与主窗口共用同一份保存顺序；在面板里调整会立即写回 Rust 状态。
  const {
    groups,
    sectionKeys,
    commitSections,
    commitAccounts,
    moveSection,
    moveAccount,
  } = useOverviewOrder(state.accounts, extraSections);
  const {
    orderedAccounts: orderedRelayAccounts,
    commitAccounts: commitRelayAccounts,
    moveAccount: moveRelayAccount,
  } = useRelayOrder(relayAccounts);
  const groupsByPlatform = useMemo(
    () => new Map(groups.map((group) => [group.platform, group])),
    [groups],
  );
  const updated = latestUpdate(state.accounts, relayAccounts);
  const accountCount = state.accounts.length + relayAccounts.length;
  const busy =
    refreshingAll ||
    refreshingIds.size > 0 ||
    state.accounts.some((account) =>
      account.products.some((product) => product.query.refreshing),
    );

  // 打开面板时才读取中转站账户；事件监听在本次窗口生命周期内只注册一次。
  const ensureLoadedRef = useRef(ensureLoaded);
  const reloadRef = useRef(reload);
  const resyncRef = useRef(resync);
  const refreshDueRef = useRef(refreshDue);
  const refreshMinutesRef = useRef(refreshMinutes);
  ensureLoadedRef.current = ensureLoaded;
  reloadRef.current = reload;
  resyncRef.current = resync;
  refreshDueRef.current = refreshDue;
  refreshMinutesRef.current = refreshMinutes;

  useEffect(() => {
    if (!desktop) return;
    ensureLoadedRef.current();

    // 面板只在用户点开时才创建，挂载即代表第一次打开。这样即使加载完成
    // 早于事件监听注册、错过了首个 `tray-panel-shown`，也不会漏掉首轮同步。
    setOpened(true);
    void resyncRef.current();
    void refreshDueRef.current(refreshMinutesRef.current);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onTrayPanelShown(() => {
      setOpened(true);
      // 事件通常已经同步；这里再用完整快照对齐一次，兜底隐藏期间漏掉的更新。
      void resyncRef.current();
      void reloadRef.current();
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

  const refreshAccount = useCallback(
    (accountId: string) => {
      void refresh(accountId);
    },
    [refresh],
  );

  const refreshRelayAccount = useCallback(
    (id: string) => {
      void refreshRelay(id);
    },
    [refreshRelay],
  );

  const toggleSection = useCallback((key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const hasAnything = state.accounts.length > 0 || relayAccounts.length > 0;

  return (
    <div className={styles.panel}>
      <header className={styles.header}>
        <div className={styles.brand}>
          <img className={styles.wordmark} src={wordmark} alt={appInfo?.name ?? 'Token Refill'} />
          <span className={styles.updated}>
            {accountCount > 0 ? `${accountCount} 个账户 · ` : ''}
            {updated !== null
              ? `更新于 ${stampLabel(updated)}`
              : ready
                ? '尚未获取额度'
                : '正在读取…'}
            {busy ? ' · 刷新中' : ''}
          </span>
        </div>
        <button
          type="button"
          className={styles.refreshAll}
          onClick={() => void refreshAll()}
          disabled={!desktop || busy}
          title={desktop ? '刷新全部额度' : '浏览器预览无法调用桌面接口'}
        >
          <ArrowClockwise
            size={13}
            className={busy ? styles.spin : undefined}
          />
          全部刷新
        </button>
      </header>

      <div className={styles.scroll}>
        {stateFileNotice ? (
          <p className={styles.notice} role="alert">
            {stateFileNotice}
          </p>
        ) : null}
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
            {sectionKeys.length > 0 ? (
              <Reorder.Group
                as="div"
                axis="y"
                values={sectionKeys}
                onReorder={commitSections}
                className={styles.sections}
              >
                {sectionKeys.map((key) => {
                  if (key === RELAY_SECTION_KEY) {
                    return (
                      <RelaySection
                        key={key}
                        accounts={orderedRelayAccounts}
                        open={!collapsed.has(key)}
                        onToggle={() => toggleSection(key)}
                        sortable={sectionKeys.length > 1}
                        threshold={threshold}
                        lowQuotaEnabled={lowQuotaEnabled}
                        refreshingIds={refreshingIds}
                        canRefresh={desktop}
                        onRefresh={refreshRelayAccount}
                        onMoveSection={(delta) => moveSection(key, delta)}
                        onMoveAccount={moveRelayAccount}
                        onReorderAccounts={commitRelayAccounts}
                      />
                    );
                  }
                  const group = groupsByPlatform.get(key);
                  if (!group) return null;
                  return (
                    <SortableSection
                      key={key}
                      group={group}
                      open={!collapsed.has(key)}
                      onToggle={() => toggleSection(key)}
                      sortable={sectionKeys.length > 1}
                      threshold={threshold}
                      lowQuotaEnabled={lowQuotaEnabled}
                      canRefresh={desktop}
                      onRefresh={refreshAccount}
                      onMoveSection={(delta) => moveSection(key, delta)}
                      onMoveAccount={(accountId, delta) =>
                        moveAccount(key, accountId, delta)
                      }
                      onReorderAccounts={(accountIds) =>
                        commitAccounts(key, accountIds)
                      }
                    />
                  );
                })}
              </Reorder.Group>
            ) : null}
          </>
        )}
      </div>

      <footer className={styles.footer}>
        <span className={styles.version}>
          {appInfo ? `v${appInfo.version}` : 'v—'}
        </span>
        <div className={styles.footerActions}>
          <button
            type="button"
            className={styles.openButton}
            onClick={() => void openMainWindow()}
          >
            <ArrowSquareOut size={14} weight="bold" aria-hidden />
            打开主窗口
          </button>
          <span className={styles.divider} aria-hidden="true" />
          <button
            type="button"
            className={styles.quitButton}
            onClick={() => void quitApp()}
            aria-label={`退出 ${appInfo?.name ?? 'Token Refill'}`}
            title={`退出 ${appInfo?.name ?? 'Token Refill'}`}
          >
            <Power size={14} aria-hidden />
            退出
          </button>
        </div>
      </footer>
    </div>
  );
}