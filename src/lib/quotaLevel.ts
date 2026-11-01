/**
 * 额度等级：主窗口、菜单栏快速面板与悬浮球共用同一套颜色层级。
 * 规则（剩余比例）：
 *   normal  高于 70%           主色
 *   warning 阈值 < 剩余 ≤ 70%  琥珀色
 *   danger  剩余 ≤ 阈值         红色（含 0%，额度耗尽）
 *   unknown 没有可靠比例        灰色
 * 关闭低额度提醒时不做警戒着色，但未知比例仍显示为灰色。
 */
export const QUOTA_WARNING_PERCENT = 70;

export type QuotaLevel = 'normal' | 'warning' | 'danger' | 'unknown';

export function quotaLevel(
  percent: number | null,
  threshold: number,
  lowQuotaEnabled: boolean,
): QuotaLevel {
  if (percent === null || !Number.isFinite(percent)) return 'unknown';
  if (!lowQuotaEnabled) return 'normal';
  if (percent <= threshold) return 'danger';
  if (percent <= QUOTA_WARNING_PERCENT) return 'warning';
  return 'normal';
}
