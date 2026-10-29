import type { AccountState, QuotaWindow } from './desktop';

/** 分钟数转中文时长；额度窗口标题与中转站窗口共用。 */
export function durationLabel(mins: number | null): string {
  if (mins === null) return '';
  if (mins < 60) return `${mins} 分钟`;
  if (mins < 1440) return `${Math.round(mins / 60)} 小时`;
  if (mins === 10080) return '1 周';
  return `${Math.round(mins / 1440)} 天`;
}

/** 规范化窗口名称：将英文周期名或带“窗口”后缀的名称转换为统一中文名。 */
export function normalizeWindowName(
  name: string | null = null,
  mins: number | null = null,
): string | null {
  if (!name) {
    if (mins === 300) return '5 小时';
    if (mins === 10080) return '1 周';
    return null;
  }
  const lower = name.toLowerCase();
  if (
    lower.includes('five')
    || lower.includes('5h')
    || name.startsWith('5 小时')
    || name.startsWith('5小时')
  ) {
    return '5 小时';
  }
  if (
    lower.includes('week')
    || name.startsWith('1 周')
    || name.startsWith('1周')
    || name === '周'
  ) {
    return '1 周';
  }
  if (lower.includes('month') || name.includes('月')) {
    return '订阅月';
  }
  if (lower.includes('day') || name.includes('天') || name.includes('日')) {
    return '1 天';
  }
  return name.replace(/(?:窗口|额度)$/, '');
}

/** 平台模型分组显示名称（如 Antigravity 下的 Gemini / Claude 模型分组）。 */
export function groupLabel(name: string | null | undefined): string | null {
  if (!name) return null;
  const trimmed = name.trim();
  if (/^gemini\s+models$/i.test(trimmed)) return 'Gemini 模型';
  if (/^claude\s+and\s+gpt\s+models$/i.test(trimmed)) return 'Claude 与 GPT 模型';
  return trimmed;
}

/** 总览的窗口标题：「订阅月」统一称「月额度」。 */
export function quotaWindowLabel(
  mins: number | null,
  name: string | null = null,
): string {
  const normalized = normalizeWindowName(name, mins);
  if (normalized) {
    // 「订阅月」与其它窗口统一成「月额度」，不再保留「订阅」二字。
    return normalized === '订阅月' ? '月额度' : `${normalized}额度`;
  }
  const duration = durationLabel(mins);
  return duration ? `${duration}额度` : '额度';
}

/** 折叠行里的窗口名更短：1 周写作「周」，订阅月写作「月」。 */
export function compactQuotaWindowLabel(window: QuotaWindow): string {
  const normalized = normalizeWindowName(
    window.windowName,
    window.windowDurationMins,
  );
  if (normalized) {
    if (normalized === '订阅月' || normalized === '月') return '月';
    return normalized.replace(/^1\s*周$/, '周');
  }
  if (window.windowDurationMins === 10080) return '周';
  return durationLabel(window.windowDurationMins) || '额度';
}

export function resetLabel(epoch: number | null): string {
  if (epoch === null) return '重置时间未知';
  const date = new Date(epoch * 1000);
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const now = new Date();
  if (date.toDateString() === now.toDateString()) return `重置于 ${time}`;
  const day = `${date.getMonth() + 1}月${date.getDate()}日`;
  return `重置于 ${day} ${time}`;
}

export function clockLabel(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

/** 菜单栏面板的刷新时间：带上日期，隔天打开也能看出数据是哪天的。 */
export function stampLabel(epoch: number): string {
  const date = new Date(epoch * 1000);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
}

/**
 * 单个账户的刷新时间：一分钟内算「刚刚」，一小时内按分钟，更早回到钟点或日期。
 * 数值来自本机时钟与最后一次查询时间的差值，时钟回拨时按「刚刚」处理。
 */
export function relativeUpdateLabel(epoch: number | null): string {
  if (epoch === null) return '尚未更新';
  const date = new Date(epoch * 1000);
  const minutes = Math.floor((Date.now() - epoch * 1000) / 60000);
  if (minutes < 1) return '刚刚更新';
  if (minutes < 60) return `${minutes} 分钟前`;
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (date.toDateString() === new Date().toDateString()) return time;
  return `${date.getMonth() + 1}/${date.getDate()} ${time}`;
}

export function planTierLabel(tier: string): string {
  return tier[0].toUpperCase() + tier.slice(1);
}

/** 账户接入方式；总览与账户页共用同一套说法。 */
export function sourceLabel(account: AccountState): string {
  if (account.platform === 'bailian') return '百炼控制台授权';
  if (account.platform === 'antigravity') return '本机 agy 登录';
  if (account.platform === 'opencode-go') {
    return account.source === 'managed' ? 'API Key 授权' : '本机 CLI 登录';
  }
  if (account.platform === 'commandcode') return 'API Key 授权';
  return account.source === 'managed' ? '独立授权' : '本机登录态';
}