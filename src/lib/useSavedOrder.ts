import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * 本地先应用用户调整的顺序，再异步保存；保存成功后由状态事件对齐。
 * 连续拖动时忽略中间态，避免界面在两次保存之间闪回上一个位置；
 * 保存失败时回到后端仍持有的顺序。
 */
export function useSavedOrder<T>(
  persisted: T,
  keyOf: (value: T) => string,
  save: (value: T) => Promise<boolean>,
): [T, (next: T) => void] {
  const [order, setOrder] = useState<T>(persisted);
  const pending = useRef<T | null>(null);
  const persistedRef = useRef(persisted);
  persistedRef.current = persisted;
  const persistedKey = keyOf(persisted);

  useEffect(() => {
    if (pending.current !== null) {
      // 只接受自己最后一次提交的顺序；中间态事件不覆盖本地位置。
      if (keyOf(persisted) !== keyOf(pending.current)) return;
      pending.current = null;
    }
    setOrder(persisted);
    // persistedKey 已经覆盖顺序内容，刷新额度等无关状态变化不会重置排序。
  }, [persistedKey]);

  const commit = useCallback(
    (next: T) => {
      pending.current = next;
      setOrder(next);
      void save(next).then((saved) => {
        if (saved) return;
        if (pending.current !== next) return;
        pending.current = null;
        setOrder(persistedRef.current);
      });
    },
    [save],
  );

  return [order, commit];
}