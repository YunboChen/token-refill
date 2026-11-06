import type { AccountState, QuotaSnapshot, QuotaWindow } from './desktop';
import type { AccountGroup } from './platforms';

/** 总览「需处理」纳入的三类问题。 */
export type AttentionReasonKey = 'reauth' | 'refreshFailed' | 'exhausted';

/**
 * 单个具体原因。额度用尽时一并带上服务提供的重置 / 到期时间，
 * 供卡片直接展示，不再二次解析快照。
 */
export type AttentionReason =
  | { key: 'reauth'; label: string }
  | { key: 'refreshFailed'; label: string }
  | {
      key: 'exhausted';
      label: string;
      resetAtEpoch: number | null;
      resetKind: 'reset' | 'expiry' | null;
    };

/** 一个账户的全部待处理原因；同一账户多个问题只算一个账户。 */
export interface AccountAttention {
  accountId: string;
  reasons: AttentionReason[];
}

export interface AttentionOptions {
  /** 设置页的自动刷新间隔，用于推算快照新鲜度。 */
  refreshMinutes: number;
  /** 当前时间（epoch 秒）。由调用方定时推进，避免每张卡各自计时。 */
  nowEpoch: number;
}

const REASON_LABELS: Record<AttentionReasonKey, string> = {
  reauth: '需要重新授权',
  refreshFailed: '刷新失败',
  exhausted: '额度已用尽',
};

export function attentionReasonLabel(key: AttentionReasonKey): string {
  return REASON_LABELS[key];
}

/**
 * 快照新鲜度窗口：两个自动刷新周期，至少 10 分钟。
 * 后台刷新可能被平台限流推迟，多留一个周期，避免刚刷新完就被判为过期。
 */
export function freshnessLimitSeconds(refreshMinutes: number): number {
  return Math.max(2 * refreshMinutes * 60, 600);
}

/** 快照是否仍能代表当前额度。 */
export function isSnapshotFresh(
  snapshot: QuotaSnapshot,
  options: AttentionOptions,
): boolean {
  return (
    options.nowEpoch - snapshot.checkedAtEpoch
    <= freshnessLimitSeconds(options.refreshMinutes)
  );
}

/**
 * 周期是否仍有效：有重置时间时以重置时间为准，否则看到期时间；
 * 两者都未知时无法判断，按有效处理。
 */
export function isWindowPeriodValid(
  window: QuotaWindow,
  nowEpoch: number,
): boolean {
  if (window.resetsAtEpoch !== null) return window.resetsAtEpoch > nowEpoch;
  if (window.expiresAtEpoch !== null) return window.expiresAtEpoch > nowEpoch;
  return true;
}

interface ExhaustedDetail {
  exhausted: boolean;
  /** 所有用尽窗口中最早的重置 / 到期时间。 */
  resetAtEpoch: number | null;
  resetKind: 'reset' | 'expiry' | null;
}

/**
 * 额度用尽必须同时满足：快照新鲜、周期未结束、剩余比例为 0。
 * 旧快照里的 0% 只代表上次成功查询时的状态，不能当作当前额度用尽。
 */
function exhaustedDetail(
  account: AccountState,
  options: AttentionOptions,
): ExhaustedDetail {
  let exhausted = false;
  let resetAtEpoch: number | null = null;
  let resetKind: 'reset' | 'expiry' | null = null;

  for (const product of account.products) {
    const snapshot = product.snapshot;
    if (snapshot === null || !isSnapshotFresh(snapshot, options)) continue;

    for (const group of snapshot.groups) {
      for (const window of group.windows) {
        if (window.remainingPercent !== 0) continue;
        if (!isWindowPeriodValid(window, options.nowEpoch)) continue;

        exhausted = true;
        const epoch = window.resetsAtEpoch ?? window.expiresAtEpoch;
        if (epoch === null) continue;
        if (resetAtEpoch === null || epoch < resetAtEpoch) {
          resetAtEpoch = epoch;
          resetKind = window.resetsAtEpoch !== null ? 'reset' : 'expiry';
        }
      }
    }
  }

  return { exhausted, resetAtEpoch, resetKind };
}

/** 单个账户的待处理原因；没有问题时返回 null。 */
export function classifyAccount(
  account: AccountState,
  options: AttentionOptions,
): AccountAttention | null {
  const reasons: AttentionReason[] = [];

  if (account.authState === 'needs-reauth') {
    reasons.push({ key: 'reauth', label: REASON_LABELS.reauth });
  }
  if (account.products.some((product) => product.query.lastError !== null)) {
    reasons.push({ key: 'refreshFailed', label: REASON_LABELS.refreshFailed });
  }
  const exhausted = exhaustedDetail(account, options);
  if (exhausted.exhausted) {
    reasons.push({
      key: 'exhausted',
      label: REASON_LABELS.exhausted,
      resetAtEpoch: exhausted.resetAtEpoch,
      resetKind: exhausted.resetKind,
    });
  }

  return reasons.length > 0
    ? { accountId: account.accountId, reasons }
    : null;
}

/** 全部待处理账户；同一账户只出现一次。 */
export function attentionAccounts(
  accounts: readonly AccountState[],
  options: AttentionOptions,
): AccountAttention[] {
  const result: AccountAttention[] = [];
  for (const account of accounts) {
    const attention = classifyAccount(account, options);
    if (attention) result.push(attention);
  }
  return result;
}

/** 待处理账户数（按账户计数，不按原因计数）。 */
export function countAttentionAccounts(
  accounts: readonly AccountState[],
  options: AttentionOptions,
): number {
  return attentionAccounts(accounts, options).length;
}

/**
 * 需处理视图的分组：保持平台与账户的既有顺序，只留下有待处理问题的账户，
 * 并去掉因此变空的分组。原分组数组不会被修改。
 */
export function filterGroupsByAttention(
  groups: readonly AccountGroup[],
  attention: readonly AccountAttention[],
): AccountGroup[] {
  const ids = new Set(attention.map((item) => item.accountId));
  return groups
    .map((group) => ({
      ...group,
      accounts: group.accounts.filter((account) => ids.has(account.accountId)),
    }))
    .filter((group) => group.accounts.length > 0);
}
