/**
 * 用户保存的展示顺序只记录标识；这里统一按标识列表排序。
 * 未列出的项保持原有顺序并排在已列出项之后，避免新增条目打乱既有分组。
 */

export function savedOrderKey(keys: readonly string[]): string {
  return keys.join('\u0001');
}

export function applySavedOrder<T>(
  items: readonly T[],
  order: readonly string[],
  keyOf: (item: T) => string,
): T[] {
  if (order.length === 0) return [...items];
  const ranks = new Map<string, number>();
  order.forEach((key, index) => {
    if (!ranks.has(key)) ranks.set(key, index);
  });
  return [...items].sort((left, right) => {
    const leftRank = ranks.get(keyOf(left));
    const rightRank = ranks.get(keyOf(right));
    if (leftRank === undefined) return rightRank === undefined ? 0 : 1;
    if (rightRank === undefined) return -1;
    return leftRank - rightRank;
  });
}

/** 把数组中的一项前后移动一格；越界时返回 null。 */
export function moveItemBy<T>(
  items: readonly T[],
  index: number,
  delta: -1 | 1,
): T[] | null {
  const target = index + delta;
  if (index < 0 || target < 0 || target >= items.length) return null;
  const next = [...items];
  const [item] = next.splice(index, 1);
  next.splice(target, 0, item);
  return next;
}