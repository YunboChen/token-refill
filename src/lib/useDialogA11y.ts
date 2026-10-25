import { useCallback, useEffect, useRef, type RefObject } from 'react';

const FOCUSABLE = [
  'button:not([disabled])',
  '[href]',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => element.getAttribute('aria-hidden') !== 'true',
  );
}

/**
 * 弹窗键盘行为：打开时把焦点交给弹窗，Tab 在弹窗内循环，Esc 关闭，
 * 关闭后把焦点还给触发元素。contentKey 变化（如等待页换成结果页）时重新落定焦点。
 *
 * 打开弹窗前调用 captureTrigger(触发元素)。不要依赖「打开瞬间的 activeElement」：
 * 带 autoFocus 的输入框会在副作用之前接管焦点，导致还原到弹窗内部。
 */
export function useDialogA11y(
  open: boolean,
  contentKey: string,
  onClose: () => void,
): {
  dialogRef: RefObject<HTMLDivElement | null>;
  captureTrigger: (element?: HTMLElement | null) => void;
} {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  const wasOpen = useRef(false);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  const captureTrigger = useCallback((element?: HTMLElement | null) => {
    const target = element ?? document.activeElement;
    triggerRef.current = target instanceof HTMLElement ? target : null;
  }, []);

  // 关闭后把焦点还给入口；触发元素已被移除时不再强行聚焦。
  useEffect(() => {
    if (!open && wasOpen.current) {
      const trigger = triggerRef.current;
      triggerRef.current = null;
      if (trigger?.isConnected) trigger.focus();
    }
    wasOpen.current = open;
  }, [open]);

  // 打开或内容切换时落定焦点；已有 autoFocus 元素时保持不动。
  useEffect(() => {
    if (!open) return;
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (!dialog.contains(document.activeElement)) dialog.focus();
  }, [open, contentKey]);

  // 挂在 document 上而不是遮罩上：内容切换导致焦点落回 body 时，Esc 仍然有效。
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== 'Tab') return;

      const dialog = dialogRef.current;
      if (!dialog) return;
      const items = focusableWithin(dialog);
      if (items.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }

      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      const inside = active instanceof Node && dialog.contains(active);

      if (!inside) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
        return;
      }
      if (event.shiftKey && (active === first || active === dialog)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || active === dialog)) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [open]);

  return { dialogRef, captureTrigger };
}