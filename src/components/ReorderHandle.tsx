import { DotsSixVertical } from '@phosphor-icons/react';
import type { DragControls } from 'motion/react';
import styles from '../App.module.css';

/**
 * 排序手柄：指针拖动由 motion 的 Reorder 接管；聚焦后按上下方向键也能移动一格。
 * 只从手柄开始拖动，账户行的展开/收起与刷新按钮不受影响。
 */
export default function ReorderHandle({
  label,
  controls,
  onMove,
}: {
  label: string;
  controls: DragControls;
  onMove: (delta: -1 | 1) => void;
}) {
  return (
    <button
      type="button"
      className={styles.reorderHandle}
      onPointerDown={(event) => controls.start(event)}
      onKeyDown={(event) => {
        if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
        event.preventDefault();
        onMove(event.key === 'ArrowUp' ? -1 : 1);
      }}
      title="拖动调整顺序；聚焦后可用上下方向键"
      aria-label={label}
    >
      <DotsSixVertical size={16} aria-hidden />
    </button>
  );
}