/// <reference types="node" />
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  durationLabel,
  normalizeWindowName,
  quotaWindowLabel,
} from './format.ts';

test('durationLabel 对非整数时长保留一位小数', () => {
  assert.equal(durationLabel(null), '');
  assert.equal(durationLabel(30), '30 分钟');
  assert.equal(durationLabel(60), '1 小时');
  // 90 分钟不能显示成「2 小时」。
  assert.equal(durationLabel(90), '1.5 小时');
  assert.equal(durationLabel(125), '2.1 小时');
  assert.equal(durationLabel(1440), '1 天');
  assert.equal(durationLabel(2160), '1.5 天');
  assert.equal(durationLabel(10080), '1 周');
});

test('normalizeWindowName 按整词识别英文周期名', () => {
  assert.equal(normalizeWindowName('5-hour'), '5 小时');
  assert.equal(normalizeWindowName('5h'), '5 小时');
  assert.equal(normalizeWindowName('5 hours'), '5 小时');
  assert.equal(normalizeWindowName('five hour'), '5 小时');
  assert.equal(normalizeWindowName('weekly'), '1 周');
  assert.equal(normalizeWindowName('monthly'), '订阅月');
  assert.equal(normalizeWindowName('daily'), '1 天');
  assert.equal(normalizeWindowName('7 days'), '1 天');
});

test('normalizeWindowName 不把子串误判为周期', () => {
  // Monday / today / Sunday 含 day 子串但不是「日」窗口。
  assert.equal(normalizeWindowName('Monday'), 'Monday');
  assert.equal(normalizeWindowName('today'), 'today');
  assert.equal(normalizeWindowName('Sunday'), 'Sunday');
  // 15h 不是 5 小时窗口。
  assert.equal(normalizeWindowName('15h'), '15h');
  // 月份窗口按「月」识别，与 month 子串无关。
  assert.equal(normalizeWindowName('Monthly limit'), '订阅月');
});

test('normalizeWindowName 兼容带下划线的英文周期名', () => {
  assert.equal(normalizeWindowName('week_limit'), '1 周');
  assert.equal(normalizeWindowName('monthly_reset'), '订阅月');
});

test('normalizeWindowName 保留中文窗口名与缺省推导', () => {
  assert.equal(normalizeWindowName('5 小时窗口'), '5 小时');
  assert.equal(normalizeWindowName('1 周窗口'), '1 周');
  assert.equal(normalizeWindowName('订阅月'), '订阅月');
  assert.equal(normalizeWindowName('Cursor 模型'), 'Cursor 模型');
  assert.equal(normalizeWindowName(null, 300), '5 小时');
  assert.equal(normalizeWindowName(null, 10080), '1 周');
  assert.equal(normalizeWindowName(null), null);
});

test('quotaWindowLabel 统一「月额度」并补全周期单位', () => {
  assert.equal(quotaWindowLabel(300, '5 小时'), '5 小时额度');
  assert.equal(quotaWindowLabel(null, '订阅月'), '月额度');
  assert.equal(quotaWindowLabel(43200, null), '30 天额度');
  assert.equal(quotaWindowLabel(null, ''), '');
});
