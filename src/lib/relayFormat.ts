import type {
  KeyQuotaWindow,
  QuotaMode,
  QuotaProvider,
  RelayOrigin,
} from './desktop';
import { durationLabel } from './format';

/** 中转站页面共用的文案与金额格式化；不涉及任何凭证。 */

export const RELAY_PROVIDER_LABELS: Record<QuotaProvider, string> = {
  'new-api': 'New API',
  'sub2-api': 'Sub2API',
};

export const RELAY_MODE_LABELS: Record<QuotaMode, string> = {
  key: '密钥额度',
  subscription: '订阅额度',
  wallet: '钱包余额',
  unrestricted: '账户余额',
};

export const RELAY_ORIGIN_LABELS: Record<RelayOrigin, string> = {
  manual: '手动输入',
  'cc-switch': 'cc-switch 配置',
  'claude-code': 'Claude Code 配置',
  codex: 'Codex 配置',
};

/** 扫描候选的来源：cc-switch 还要区分 Claude Code / Codex 两套 provider。 */
export function relayCandidateSource(
  origin: RelayOrigin,
  app: string | null,
): string {
  if (origin === 'cc-switch') {
    return app === 'codex' ? 'cc-switch · Codex' : 'cc-switch · Claude Code';
  }
  return RELAY_ORIGIN_LABELS[origin];
}

export function relayErrorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return '查询失败，请检查服务地址与密钥。';
}

/** 卡片标题只显示主机名，完整地址留在 title 里。 */
export function relayHost(baseUrl: string): string {
  return baseUrl.replace(/^https?:\/\//, '');
}

export function formatRelayMoney(
  value: number | null,
  currency: string | null,
): string {
  if (value === null) return '—';
  switch ((currency ?? 'USD').toUpperCase()) {
    case 'USD':
      return `$${value.toFixed(2)}`;
    case 'CNY':
    case 'RMB':
      return `¥${value.toFixed(2)}`;
    default:
      return `${value.toFixed(2)} ${currency ?? ''}`.trim();
  }
}

export function relayWindowLabel(window: KeyQuotaWindow): string {
  const duration = durationLabel(window.durationMins);
  return duration ? `${duration}窗口` : window.windowId;
}

/** 到期时间只给日期；缺失时由调用方拼出「未提供」。 */
export function relayExpiryText(epoch: number | null): string {
  if (epoch === null) return '未提供';
  return new Date(epoch * 1000).toLocaleDateString([], {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  });
}

/** 剩余比例；总额缺失或为 0 时返回 null，由界面展示「未提供」而不是 0%。 */
export function relayPercent(
  remaining: number | null,
  total: number | null,
): number | null {
  if (remaining === null || total === null || total <= 0) return null;
  return Math.round(
    Math.min(100, Math.max(0, (remaining / total) * 100)),
  );
}
