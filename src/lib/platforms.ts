import type { AccountState } from './desktop';

/**
 * 平台是账户的上层分类：同一平台下的多个账户共享接入方式与额度口径，
 * 不同平台之间不做合并展示。新增平台时只需在这里补充元数据与顺序。
 */
export interface PlatformMeta {
  key: string;
  /** 界面上展示的平台名 */
  name: string;
  /** 平台归属厂商，用于区分同名套餐 */
  vendor: string;
  /** 平台徽标中的单字标记 */
  mark: string;
  /** 一句话说明该平台展示什么额度 */
  description: string;
  /** 是否已经可以发起新账户授权 */
  available: boolean;
}

const PLATFORMS: Record<string, PlatformMeta> = {
  codex: {
    key: 'codex',
    name: 'Codex',
    vendor: 'OpenAI',
    mark: 'C',
    description: 'ChatGPT 订阅额度 · 本机登录与独立授权',
    available: true,
  },
  bailian: {
    key: 'bailian',
    name: '百炼',
    vendor: '阿里云',
    mark: '百',
    description: 'Token Plan 个人版 · Coding Plan 中国站',
    available: true,
  },
};

/** 已知平台的固定展示顺序；未知平台按名称排在最后。 */
const PLATFORM_ORDER = ['codex', 'bailian'];

export function platformMeta(platform: string): PlatformMeta {
  return (
    PLATFORMS[platform] ?? {
      key: platform,
      name: platform,
      vendor: '其他平台',
      mark: platform.slice(0, 1).toUpperCase(),
      description: '',
      available: false,
    }
  );
}

/** 已登记但尚未开放接入的平台，按固定顺序返回。 */
export function pendingPlatforms(): PlatformMeta[] {
  return PLATFORM_ORDER.map((platform) => PLATFORMS[platform]).filter(
    (meta): meta is PlatformMeta => meta !== undefined && !meta.available,
  );
}

export interface AccountGroup {
  platform: string;
  meta: PlatformMeta;
  accounts: AccountState[];
}

function buildGroups(
  accounts: AccountState[],
  includeEmptyAvailable: boolean,
): AccountGroup[] {
  const grouped = new Map<string, AccountState[]>();
  for (const account of accounts) {
    const list = grouped.get(account.platform);
    if (list) list.push(account);
    else grouped.set(account.platform, [account]);
  }

  const known = PLATFORM_ORDER.filter(
    (platform) =>
      grouped.has(platform)
      || (includeEmptyAvailable && PLATFORMS[platform]?.available === true),
  );
  const unknown = [...grouped.keys()]
    .filter((platform) => !PLATFORM_ORDER.includes(platform))
    .sort();

  return [...known, ...unknown].map((platform) => ({
    platform,
    meta: platformMeta(platform),
    accounts: grouped.get(platform) ?? [],
  }));
}

/** 按平台归组，保持已知平台的固定顺序，避免账户增删导致分组跳动。 */
export function groupAccountsByPlatform(accounts: AccountState[]): AccountGroup[] {
  return buildGroups(accounts, false);
}

/**
 * 账户页的分组：已开放接入的平台即使还没有账户也保留空分组，
 * 这样每个平台都有自己的添加入口，不必借已接入的平台进入再切换类型。
 */
export function accountPlatformGroups(accounts: AccountState[]): AccountGroup[] {
  return buildGroups(accounts, true);
}