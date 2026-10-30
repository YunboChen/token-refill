import { useCallback } from 'react';
import type { AccountState, OverviewOrder, RelayAccount } from './desktop';
import { applySavedOrder, moveItemBy, savedOrderKey } from './order';
import { groupAccountsByPlatform } from './platforms';
import { useAppState } from './store';
import { useSavedOrder } from './useSavedOrder';

/** `useSavedOrder` 用它判断持久化顺序是否变化，比较的是顺序内容而不是对象身份。 */
function overviewOrderKey(order: OverviewOrder): string {
  return `${savedOrderKey(order.platforms)}\u0002${savedOrderKey(order.accounts)}`;
}

/**
 * 总览与菜单栏面板共用的平台 / 账户排序：
 * 先把账户按平台归组并按保存顺序排列，再提供拖动与键盘移动的提交入口。
 * 两个窗口写入同一份 Rust 状态，任一处调整都会随状态事件同步到另一处。
 */
export function useOverviewOrder(accounts: AccountState[]) {
  const { state, saveOverviewOrder } = useAppState();
  const [order, persistOrder] = useSavedOrder(
    state.overviewOrder,
    overviewOrderKey,
    saveOverviewOrder,
  );
  const groups = groupAccountsByPlatform(accounts, order);

  // 平台位置变化后按当前展示顺序一并提交账户顺序，
  // 否则未显式保存过的账户会按默认分组顺序重排。
  const commitPlatforms = useCallback(
    (platforms: string[]) => {
      persistOrder({
        platforms,
        accounts: groups.flatMap((group) =>
          group.accounts.map((account) => account.accountId),
        ),
      });
    },
    [groups, persistOrder],
  );

  const commitAccounts = useCallback(
    (platform: string, accountIds: string[]) => {
      persistOrder({
        platforms: groups.map((group) => group.platform),
        accounts: groups.flatMap((group) =>
          group.platform === platform
            ? accountIds
            : group.accounts.map((account) => account.accountId),
        ),
      });
    },
    [groups, persistOrder],
  );

  const movePlatform = useCallback(
    (platform: string, delta: -1 | 1) => {
      const platforms = groups.map((group) => group.platform);
      const next = moveItemBy(platforms, platforms.indexOf(platform), delta);
      if (next) commitPlatforms(next);
    },
    [groups, commitPlatforms],
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

  return { groups, commitPlatforms, commitAccounts, movePlatform, moveAccount };
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