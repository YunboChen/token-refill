import { useCallback, useMemo } from 'react';
import type { AccountState, OverviewOrder, RelayAccount } from './desktop';
import { applySavedOrder, moveItemBy, savedOrderKey } from './order';
import { groupAccountsByPlatform } from './platforms';
import { useAppState } from './store';
import { useSavedOrder } from './useSavedOrder';

/** 菜单栏面板里与平台分组并列的附加分组（中转站）；作为分组标识存进平台顺序。 */
export const RELAY_SECTION_KEY = 'relay';

/** 不属于平台分组的附加分组；只有它们的标识会在拖动平台时按原位保留。 */
const EXTRA_SECTION_KEYS = [RELAY_SECTION_KEY];

/** `useSavedOrder` 用它判断持久化顺序是否变化，比较的是顺序内容而不是对象身份。 */
function overviewOrderKey(order: OverviewOrder): string {
  return `${savedOrderKey(order.platforms)}\u0002${savedOrderKey(order.accounts)}`;
}

/**
 * 把一次平台拖动的结果写回保存顺序：保存顺序里可能还有附加分组（如中转站），
 * 它们不参与这次拖动，按“前面有几个平台”的槽位保留在原位置。
 */
function mergePlatformOrder(
  saved: readonly string[],
  platformKeys: readonly string[],
  nextPlatforms: readonly string[],
): string[] {
  const known = new Set(platformKeys);
  // 每个附加分组前面有多少个平台，决定它在新顺序里的位置。
  const slots = new Map<string, number>();
  let platformsBefore = 0;
  for (const key of saved) {
    if (known.has(key)) {
      platformsBefore += 1;
    } else if (EXTRA_SECTION_KEYS.includes(key) && !slots.has(key)) {
      slots.set(key, platformsBefore);
    }
  }
  if (slots.size === 0) return [...nextPlatforms];

  const result: string[] = [];
  nextPlatforms.forEach((platform, index) => {
    for (const [key, slot] of slots) {
      if (slot === index) result.push(key);
    }
    result.push(platform);
  });
  for (const [key, slot] of slots) {
    if (slot >= nextPlatforms.length) result.push(key);
  }
  return result;
}

/**
 * 总览与菜单栏面板共用的平台 / 账户排序：
 * 先把账户按平台归组并按保存顺序排列，再提供拖动与键盘移动的提交入口。
 * 菜单栏面板传入 `extraSections`（中转站）后，附加分组会与平台分组一起排序，
 * 顺序用同一个分组标识列表保存；两个窗口写入同一份 Rust 状态，
 * 任一处调整都会随状态事件同步到另一处。
 */
export function useOverviewOrder(
  accounts: AccountState[],
  extraSections: readonly string[] = [],
) {
  const { state, saveOverviewOrder } = useAppState();
  const [order, persistOrder] = useSavedOrder(
    state.overviewOrder,
    overviewOrderKey,
    saveOverviewOrder,
  );
  const groups = groupAccountsByPlatform(accounts, order);
  const platformKeys = groups.map((group) => group.platform);

  // 参与排序的分组标识：平台分组在前，附加分组按保存顺序一起排列。
  const sectionKeys = useMemo(
    () =>
      applySavedOrder(
        [...platformKeys, ...extraSections],
        order.platforms,
        (key) => key,
      ),
    [platformKeys, extraSections, order.platforms],
  );

  // 平台位置变化后按当前展示顺序一并提交账户顺序，
  // 否则未显式保存过的账户会按默认分组顺序重排。
  const commitSections = useCallback(
    (keys: string[]) =>
      persistOrder({
        platforms: keys,
        accounts: groups.flatMap((group) =>
          group.accounts.map((account) => account.accountId),
        ),
      }),
    [groups, persistOrder],
  );

  const commitPlatforms = useCallback(
    (platforms: string[]) =>
      commitSections(
        mergePlatformOrder(order.platforms, platformKeys, platforms),
      ),
    [commitSections, order.platforms, platformKeys],
  );

  const commitAccounts = useCallback(
    (platform: string, accountIds: string[]) => {
      persistOrder({
        // 分组顺序不变，但要把附加分组按原位保留下来。
        platforms: mergePlatformOrder(order.platforms, platformKeys, platformKeys),
        accounts: groups.flatMap((group) =>
          group.platform === platform
            ? accountIds
            : group.accounts.map((account) => account.accountId),
        ),
      });
    },
    [order.platforms, platformKeys, groups, persistOrder],
  );

  /** 分组级移动：同时覆盖平台分组与附加分组。 */
  const moveSection = useCallback(
    (key: string, delta: -1 | 1) => {
      const next = moveItemBy(sectionKeys, sectionKeys.indexOf(key), delta);
      if (next) commitSections(next);
    },
    [sectionKeys, commitSections],
  );

  const movePlatform = useCallback(
    (platform: string, delta: -1 | 1) => {
      const next = moveItemBy(
        platformKeys,
        platformKeys.indexOf(platform),
        delta,
      );
      if (next) commitPlatforms(next);
    },
    [platformKeys, commitPlatforms],
  );

  const moveAccount = useCallback(
    (platform: string, accountId: string, delta: -1 | 1) => {
      const group = groups.find((item) => item.platform === platform);
      if (!group) return;
      const accountIds = group.accounts.map((account) => account.accountId);
      const next = moveItemBy(accountIds, accountIds.indexOf(accountId), delta);
      if (next) commitAccounts(platform, next);
    },
    [groups, commitAccounts],
  );

  return {
    groups,
    sectionKeys,
    commitSections,
    commitPlatforms,
    commitAccounts,
    moveSection,
    movePlatform,
    moveAccount,
  };
}

/**
 * 中转站账户的排序；总览的「中转站账户额度」页与菜单栏面板共用。
 * 未参与排序的新账户排在已排序项之后，保持注册表顺序。
 */
export function useRelayOrder(accounts: RelayAccount[]) {
  const { state, saveRelayOrder } = useAppState();
  const [order, persistOrder] = useSavedOrder(
    state.relayOrder,
    savedOrderKey,
    saveRelayOrder,
  );
  const orderedAccounts = applySavedOrder(
    accounts,
    order,
    (account) => account.id,
  );

  const commitAccounts = useCallback(
    (ids: string[]) => persistOrder(ids),
    [persistOrder],
  );

  const moveAccount = useCallback(
    (id: string, delta: -1 | 1) => {
      const ids = orderedAccounts.map((account) => account.id);
      const next = moveItemBy(ids, ids.indexOf(id), delta);
      if (next) persistOrder(next);
    },
    [orderedAccounts, persistOrder],
  );

  return { orderedAccounts, commitAccounts, moveAccount };
}
