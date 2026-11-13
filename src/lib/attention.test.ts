/// <reference types="node" />
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  attentionAccounts,
  classifyAccount,
  countAttentionAccounts,
  filterGroupsByAttention,
  freshnessLimitSeconds,
  isSnapshotFresh,
  isWindowPeriodValid,
  type AttentionOptions,
} from './attention.ts';
import type {
  AccountState,
  ProductState,
  QuotaSnapshot,
  QuotaWindow,
} from './desktop.ts';
import type { AccountGroup, PlatformMeta } from './platforms.ts';

const NOW = 1_700_000_000;

function makeWindow(overrides: Partial<QuotaWindow> = {}): QuotaWindow {
  return {
    windowId: 'window',
    sourceWindowId: 'source',
    windowName: '5 小时',
    unit: 'percent',
    usedPercent: 0,
    remainingPercent: 100,
    usedAmount: null,
    totalAmount: null,
    remainingAmount: null,
    windowDurationMins: 300,
    resetsAtEpoch: NOW + 3600,
    expiresAtEpoch: null,
    ...overrides,
  };
}

function makeSnapshot(
  windows: QuotaWindow[],
  checkedAtEpoch = NOW,
): QuotaSnapshot {
  return {
    checkedAtEpoch,
    groups: [{ limitId: null, limitName: null, planType: null, windows }],
    capabilityGaps: [],
    resetCredits: null,
  };
}

function makeProduct(overrides: Partial<ProductState> = {}): ProductState {
  return {
    productId: 'product',
    kind: 'subscription',
    displayName: '订阅套餐',
    planTier: null,
    active: true,
    snapshot: makeSnapshot([makeWindow()]),
    query: { refreshing: false, lastAttemptEpoch: null, lastError: null },
    ...overrides,
  };
}

function makeAccount(overrides: Partial<AccountState> = {}): AccountState {
  return {
    accountId: 'account-1',
    platform: 'codex',
    displayName: '测试账户',
    authState: 'ok',
    source: 'managed',
    identityHint: null,
    credentialExpiresAtEpoch: null,
    products: [makeProduct()],
    ...overrides,
  };
}

function makeOptions(
  overrides: Partial<AttentionOptions> = {},
): AttentionOptions {
  return { refreshMinutes: 15, nowEpoch: NOW, ...overrides };
}

function makeMeta(key: string): PlatformMeta {
  return {
    key,
    name: key,
    vendor: '测试厂商',
    mark: key.slice(0, 1).toUpperCase(),
    description: '',
    available: true,
  };
}

test('同一账户的多个问题只计 1 个，并保留全部原因', () => {
  const account = makeAccount({
    authState: 'needs-reauth',
    products: [
      makeProduct({
        snapshot: makeSnapshot([makeWindow({ remainingPercent: 0 })]),
        query: {
          refreshing: false,
          lastAttemptEpoch: NOW,
          lastError: { code: 'network', message: '连接失败' },
        },
      }),
    ],
  });

  const attention = attentionAccounts([account], makeOptions());

  assert.equal(attention.length, 1);
  assert.equal(countAttentionAccounts([account], makeOptions()), 1);
  assert.deepEqual(
    attention[0].reasons.map((reason) => reason.key),
    ['reauth', 'refreshFailed', 'exhausted'],
  );
  assert.deepEqual(
    attention[0].reasons.map((reason) => reason.label),
    ['需要重新授权', '刷新失败', '额度已用尽'],
  );
});

test('重试成功并重新授权后，对应问题消失', () => {
  const failing = makeAccount({
    products: [
      makeProduct({
        query: {
          refreshing: false,
          lastAttemptEpoch: NOW,
          lastError: { code: 'network', message: '连接失败' },
        },
      }),
    ],
  });
  assert.notEqual(classifyAccount(failing, makeOptions()), null);

  const recovered = makeAccount({
    authState: 'ok',
    products: [makeProduct()],
  });
  assert.equal(classifyAccount(recovered, makeOptions()), null);
  assert.equal(countAttentionAccounts([recovered], makeOptions()), 0);
});

test('周期已过或快照过期的 0% 不算额度用尽', () => {
  const expiredReset = makeAccount({
    products: [
      makeProduct({
        snapshot: makeSnapshot([
          makeWindow({ remainingPercent: 0, resetsAtEpoch: NOW - 1 }),
        ]),
      }),
    ],
  });
  assert.equal(classifyAccount(expiredReset, makeOptions()), null);

  const expiredGrant = makeAccount({
    products: [
      makeProduct({
        snapshot: makeSnapshot([
          makeWindow({
            remainingPercent: 0,
            resetsAtEpoch: null,
            expiresAtEpoch: NOW - 1,
          }),
        ]),
      }),
    ],
  });
  assert.equal(classifyAccount(expiredGrant, makeOptions()), null);

  const stale = makeAccount({
    products: [
      makeProduct({
        snapshot: makeSnapshot(
          [makeWindow({ remainingPercent: 0 })],
          NOW - freshnessLimitSeconds(15) - 1,
        ),
      }),
    ],
  });
  assert.equal(classifyAccount(stale, makeOptions()), null);
});

test('新鲜且周期有效的 0% 计入用尽，并带出重置时间', () => {
  const account = makeAccount({
    products: [
      makeProduct({
        snapshot: makeSnapshot([
          makeWindow({ remainingPercent: 0, resetsAtEpoch: NOW + 1800 }),
        ]),
      }),
    ],
  });

  const attention = classifyAccount(account, makeOptions());
  assert.notEqual(attention, null);
  assert.equal(attention?.reasons.length, 1);
  const reason = attention?.reasons[0];
  assert.equal(reason?.key, 'exhausted');
  if (reason?.key === 'exhausted') {
    assert.equal(reason.resetAtEpoch, NOW + 1800);
    assert.equal(reason.resetKind, 'reset');
  }
});

test('快照新鲜度是两个刷新周期且至少 10 分钟', () => {
  assert.equal(freshnessLimitSeconds(5), 600);
  assert.equal(freshnessLimitSeconds(60), 7200);

  const atLimit = makeSnapshot([makeWindow()], NOW - 600);
  assert.equal(
    isSnapshotFresh(atLimit, makeOptions({ refreshMinutes: 5 })),
    true,
  );
  const pastLimit = makeSnapshot([makeWindow()], NOW - 601);
  assert.equal(
    isSnapshotFresh(pastLimit, makeOptions({ refreshMinutes: 5 })),
    false,
  );
});

test('周期有效性优先看重置时间，其次看到期时间', () => {
  assert.equal(
    isWindowPeriodValid(makeWindow({ resetsAtEpoch: null }), NOW),
    true,
  );
  assert.equal(
    isWindowPeriodValid(
      makeWindow({ resetsAtEpoch: null, expiresAtEpoch: NOW + 1 }),
      NOW,
    ),
    true,
  );
  assert.equal(
    isWindowPeriodValid(
      makeWindow({ resetsAtEpoch: null, expiresAtEpoch: NOW }),
      NOW,
    ),
    false,
  );
  assert.equal(
    isWindowPeriodValid(makeWindow({ resetsAtEpoch: NOW }), NOW),
    false,
  );
});

test('需处理筛选保持平台与账户顺序，不修改全部视图', () => {
  const healthy = makeAccount({ accountId: 'a1' });
  const reauth = makeAccount({
    accountId: 'a2',
    authState: 'needs-reauth',
  });
  const failed = makeAccount({
    accountId: 'b1',
    platform: 'bailian',
    products: [
      makeProduct({
        query: {
          refreshing: false,
          lastAttemptEpoch: NOW,
          lastError: { code: 'network', message: '连接失败' },
        },
      }),
    ],
  });

  const groups: AccountGroup[] = [
    { platform: 'codex', meta: makeMeta('codex'), accounts: [healthy, reauth] },
    {
      platform: 'bailian',
      meta: makeMeta('bailian'),
      accounts: [failed],
    },
  ];
  const attention = attentionAccounts([failed, reauth, healthy], makeOptions());
  const filtered = filterGroupsByAttention(groups, attention);

  assert.deepEqual(
    filtered.map((group) => group.platform),
    ['codex', 'bailian'],
  );
  assert.deepEqual(
    filtered[0].accounts.map((account) => account.accountId),
    ['a2'],
  );
  assert.deepEqual(
    filtered[1].accounts.map((account) => account.accountId),
    ['b1'],
  );
  // 「全部」视图使用的原分组保持完整且顺序不变。
  assert.deepEqual(
    groups[0].accounts.map((account) => account.accountId),
    ['a1', 'a2'],
  );
  assert.deepEqual(
    groups[1].accounts.map((account) => account.accountId),
    ['b1'],
  );
});
