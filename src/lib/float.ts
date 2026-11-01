import type {
  AccountState,
  AppState,
  KeyQuota,
  ProductState,
  QuotaWindow,
  RelayAccount,
} from './desktop';
import {
  compactQuotaWindowLabel,
  durationLabel,
  groupLabel,
  quotaWindowLabel,
  resetLabel,
} from './format';
import { platformMeta } from './platforms';
import { RELAY_MODE_LABELS, RELAY_PROVIDER_LABELS, relayHost, relayPercent } from './relayFormat';

/**
 * 悬浮球的数据推导：把账户套餐与中转站账户统一成可展示的“套餐选项”。
 * 这里只处理脱敏状态；密钥与令牌始终留在 Rust 侧。
 */

export type FloatSeverity = 'normal' | 'warning' | 'danger' | 'muted';

export interface FloatResolvedProduct {
  kind: 'product';
  key: string;
  account: AccountState;
  product: ProductState;
}

export interface FloatResolvedRelay {
  kind: 'relay';
  key: string;
  account: RelayAccount;
}

export type FloatSelection = FloatResolvedProduct | FloatResolvedRelay;

export interface FloatPlanOption {
  key: string;
  kind: 'product' | 'relay';
  /** 平台标识，用于品牌图形。 */
  platform: string;
  title: string;
  subtitle: string | null;
  /** 多周期取最低剩余比例；金额型中转站可能没有比例。 */
  percent: number | null;
  /** 球内的小周期标签，例如“周”“5小时”“余额”。 */
  period: string | null;
  /** 金额型中转站的剩余金额文案。 */
  amount: string | null;
  tier: string | null;
  available: boolean;
  refreshing: boolean;
  error: string | null;
}

export interface FloatWindowRow {
  key: string;
  label: string;
  group: string | null;
  percent: number | null;
  meta: string[];
  low: boolean;
}

export function productWindows(product: ProductState): QuotaWindow[] {
  return (product.snapshot?.groups ?? []).flatMap((group) => group.windows);
}

/** 多周期取最低剩余额度；没有任何比例时返回 null。 */
export function productPercent(product: ProductState): number | null {
  const values = productWindows(product)
    .map((window) => window.remainingPercent)
    .filter((value): value is number => value !== null);
  return values.length > 0 ? Math.min(...values) : null;
}

export function lowestWindow(product: ProductState): QuotaWindow | null {
  let best: QuotaWindow | null = null;
  for (const window of productWindows(product)) {
    if (window.remainingPercent === null) continue;
    if (
      best === null
      || (best.remainingPercent ?? 100) > window.remainingPercent
    ) {
      best = window;
    }
  }
  return best;
}

/** 中转站剩余比例：账户级数据优先，其次只有密钥额度可计算比例。 */
export function relayMeter(quota: KeyQuota | null): number | null {
  if (!quota) return null;
  if (quota.account) {
    return relayPercent(quota.account.remainingUsd, quota.account.totalUsd);
  }
  if (quota.mode === 'key') {
    return relayPercent(quota.remainingUsd, quota.limitUsd);
  }
  return null;
}

function relayAmount(quota: KeyQuota | null): string | null {
  if (!quota) return null;
  if (quota.unlimited) return '不限额';
  const value = quota.account
    ? quota.account.remainingUsd
    : quota.remainingUsd;
  if (value === null) return null;
  const currency = (quota.currency ?? 'USD').toUpperCase();
  const formatted = value.toFixed(2);
  if (currency === 'USD') return `$${formatted}`;
  if (currency === 'CNY' || currency === 'RMB') return `¥${formatted}`;
  return `${formatted} ${quota.currency ?? ''}`.trim();
}

function relayPeriod(quota: KeyQuota | null): string {
  if (!quota) return '余额';
  return quota.mode === 'key' && !quota.account ? '额度' : '余额';
}

function productTitle(account: AccountState, product: ProductState): string {
  const platform = platformMeta(account.platform).name;
  if (
    account.products.length > 1
    && !product.displayName.toLowerCase().startsWith(platform.toLowerCase())
  ) {
    return `${platform} ${product.displayName}`;
  }
  return product.displayName;
}

function productPeriod(product: ProductState): string | null {
  const window = lowestWindow(product);
  if (!window) return null;
  return compactQuotaWindowLabel(window).replace(/\s+/g, '');
}

function productError(product: ProductState): string | null {
  return product.query.lastError?.message ?? null;
}

export function resolveSelection(
  state: AppState,
  relays: RelayAccount[],
): FloatSelection | null {
  const key = state.floatingBall.selected;
  if (!key) return null;

  if (key.startsWith('product:')) {
    const rest = key.slice('product:'.length);
    const separator = rest.indexOf(':');
    if (separator < 0) return null;
    const accountId = rest.slice(0, separator);
    const productId = rest.slice(separator + 1);
    const account = state.accounts.find((item) => item.accountId === accountId);
    const product = account?.products.find((item) => item.productId === productId);
    if (!account || !product) return null;
    return {
      kind: 'product',
      key,
      account,
      product,
    };
  }

  if (key.startsWith('relay:')) {
    const id = key.slice('relay:'.length);
    const account = relays.find((item) => item.id === id);
    if (!account) return null;
    return { kind: 'relay', key, account };
  }
  return null;
}

export function planOptions(
  state: AppState,
  relays: RelayAccount[],
): FloatPlanOption[] {
  const options: FloatPlanOption[] = [];

  for (const account of state.accounts) {
    for (const product of account.products) {
      // active === false 表示平台确认未开通；不把这类套餐放进切换菜单。
      if (product.active === false) continue;
      const key = `product:${account.accountId}:${product.productId}`;
      options.push({
        key,
        kind: 'product',
        platform: account.platform,
        title: productTitle(account, product),
        subtitle: account.displayName,
        percent: productPercent(product),
        period: productPeriod(product),
        amount: null,
        tier: product.planTier,
        available: product.snapshot !== null,
        refreshing: product.query.refreshing,
        error: productError(product),
      });
    }
  }

  for (const account of relays) {
    const quota = account.quota;
    const host = relayHost(account.baseUrl);
    const title = account.label ?? host;
    const provider = RELAY_PROVIDER_LABELS[account.provider] ?? '中转站';
    options.push({
      key: `relay:${account.id}`,
      kind: 'relay',
      platform: 'relay',
      title,
      subtitle: account.label ? `${provider} · ${host}` : provider,
      percent: relayMeter(quota),
      period: relayPeriod(quota),
      amount: relayAmount(quota),
      tier: quota ? RELAY_MODE_LABELS[quota.mode] : null,
      available: quota !== null,
      refreshing: false,
      error: account.lastError,
    });
  }

  return options;
}

export function optionFromSelection(selection: FloatSelection | null): FloatPlanOption | null {
  if (!selection) return null;
  if (selection.kind === 'product') {
    const { account, product } = selection;
    return {
      key: selection.key,
      kind: 'product',
      platform: account.platform,
      title: productTitle(account, product),
      subtitle: account.displayName,
      percent: productPercent(product),
      period: productPeriod(product),
      amount: null,
      tier: product.planTier,
      available: product.snapshot !== null,
      refreshing: product.query.refreshing,
      error: productError(product),
    };
  }
  const { account } = selection;
  const quota = account.quota;
  const host = relayHost(account.baseUrl);
  return {
    key: selection.key,
    kind: 'relay',
    platform: 'relay',
    title: account.label ?? host,
    subtitle: account.label
      ? `${RELAY_PROVIDER_LABELS[account.provider] ?? '中转站'} · ${host}`
      : (RELAY_PROVIDER_LABELS[account.provider] ?? '中转站'),
    percent: relayMeter(quota),
    period: relayPeriod(quota),
    amount: relayAmount(quota),
    tier: quota ? RELAY_MODE_LABELS[quota.mode] : null,
    available: quota !== null,
    refreshing: false,
    error: account.lastError,
  };
}

export function severityFor(
  option: FloatPlanOption | null,
  threshold: number,
  lowQuotaEnabled: boolean,
): FloatSeverity {
  if (!option || !option.available || option.percent === null) {
    return option?.refreshing ? 'normal' : 'muted';
  }
  if (option.percent <= 0) return 'danger';
  if (lowQuotaEnabled && option.percent <= threshold) return 'warning';
  return 'normal';
}

/* ---------- 悬停详情 ---------- */

function amountText(window: QuotaWindow): string | null {
  const money = (value: number) => `$${value.toFixed(2)}`;
  if (window.unit === 'usd') {
    if (window.usedAmount !== null && window.totalAmount !== null) {
      return `已用 ${money(window.usedAmount)} / ${money(window.totalAmount)}`;
    }
    if (window.remainingAmount !== null) return `剩余 ${money(window.remainingAmount)}`;
    return window.usedPercent !== null ? `已用 ${window.usedPercent}%` : null;
  }
  if (window.usedAmount !== null && window.totalAmount !== null) {
    const unit = window.unit === 'calls' ? '次' : window.unit;
    return `已用 ${window.usedAmount} / ${window.totalAmount} ${unit}`;
  }
  return window.usedPercent !== null ? `已用 ${window.usedPercent}%` : null;
}

export function productRows(
  product: ProductState,
  threshold: number,
  lowQuotaEnabled: boolean,
): FloatWindowRow[] {
  const rows: FloatWindowRow[] = [];
  const groups = (product.snapshot?.groups ?? []).filter(
    (group) => group.windows.length > 0,
  );
  const multipleGroups = groups.length > 1;
  groups.forEach((group, groupIndex) => {
    group.windows.forEach((window) => {
      const percent = window.remainingPercent;
      rows.push({
        key: `${groupIndex}-${window.windowId}`,
        label: quotaWindowLabel(window.windowDurationMins, window.windowName),
        group: multipleGroups ? groupLabel(group.limitName) : null,
        percent,
        meta: [
          amountText(window),
          resetLabel(window.resetsAtEpoch),
        ].filter((part): part is string => part !== null),
        low: lowQuotaEnabled && percent !== null && percent <= threshold,
      });
    });
  });
  return rows;
}

export function relayRows(account: RelayAccount): FloatWindowRow[] {
  const quota = account.quota;
  if (!quota) return [];
  const rows: FloatWindowRow[] = [];
  const accountPercent = quota.account
    ? relayPercent(quota.account.remainingUsd, quota.account.totalUsd)
    : null;
  const amount = relayAmount(quota);
  rows.push({
    key: 'relay-summary',
    label: RELAY_MODE_LABELS[quota.mode] ?? '额度',
    group: null,
    percent: relayMeter(quota),
    meta: [
      amount,
      quota.account ? '账户余额' : null,
      quota.expiresAtEpoch
        ? `到期 ${new Date(quota.expiresAtEpoch * 1000).toLocaleDateString()}`
        : null,
    ].filter((part): part is string => part !== null),
    low: false,
  });
  if (accountPercent !== null && quota.account) {
    rows[0].percent = accountPercent;
  }
  for (const window of quota.windows) {
    rows.push({
      key: `relay-window-${window.windowId}`,
      label: durationLabel(window.durationMins) || window.windowId,
      group: null,
      percent: window.remainingPercent,
      meta: [
        window.remainingUsd !== null ? `剩余 ${window.remainingUsd.toFixed(2)}` : null,
        window.resetsAtEpoch
          ? resetLabel(window.resetsAtEpoch)
          : null,
      ].filter((part): part is string => part !== null),
      low: false,
    });
  }
  return rows;
}
