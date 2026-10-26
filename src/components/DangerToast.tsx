import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import styles from '../App.module.css';

/** 瞬时错误提示的默认停留时间；与设置页的保存失败提示一致。 */
const DEFAULT_DURATION = 4000;

interface DangerToastProps {
  message: string;
  onDismiss: () => void;
  durationMs?: number;
}

/**
 * 窗口底部居中的错误 toast：不占布局、自动消失，用于操作失败这类瞬时反馈。
 *
 * 渲染到 body 上，避免被面板的进场 transform 或弹窗蒙层限制；层级高于弹窗
 * 蒙层，弹窗打开时也能看见。
 */
export default function DangerToast({
  message,
  onDismiss,
  durationMs = DEFAULT_DURATION,
}: DangerToastProps) {
  // 调用方普遍传内联箭头函数，用 ref 保存最新回调，避免每次渲染重置计时。
  const dismissRef = useRef(onDismiss);
  useEffect(() => {
    dismissRef.current = onDismiss;
  }, [onDismiss]);

  // 新消息到达时重新计时；卸载时清掉计时器。
  useEffect(() => {
    const timer = window.setTimeout(() => dismissRef.current(), durationMs);
    return () => window.clearTimeout(timer);
  }, [message, durationMs]);

  return createPortal(
    <p className={styles.toast} role="alert">
      {message}
    </p>,
    document.body,
  );
}