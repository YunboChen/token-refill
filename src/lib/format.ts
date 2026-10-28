import type { AccountState, QuotaWindow } from './desktop';

/** 分钟数转中文时长；额度窗口标题与中转站窗口共用。 */
export function durationLabel(mins: number | null): string {
  if (mins === null) return '';
  if (mins < 60) return `${mins} 分钟`;
  if (mins < 1440) return `${Math.round(mins / 60)} 小时`;
  if (mins === 10080) return '1 周';
  return `${Math.round(mins / 1440)} 天`;
}

/** 总览的窗口标题：「订阅月」统一称「月额度」。 */
export function quotaWindowLabel(
  mins: number | null,
  name: string | null = null,
): string {
  if (name) {
    // 「订阅月」与其它窗口统一成「月额度」，不再保留「订阅」二字。
    return name === '订阅月' ? '月额度' : `${name}额度`;
  }
  const duration = durationLabel(mins);
  return duration ? `${duration}额度` : '额度';
}

/** 折叠行里的窗口名更短：1 周写作「周」，订阅月写作「月」。 */
export function compactQuotaWindowLabel(window: QuotaWindow): string {
  if (window.windowName) {
    if (window.windowName === '订阅月') return '月';
    return window.windowName.replace(/^1\s*周$/, '周');
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