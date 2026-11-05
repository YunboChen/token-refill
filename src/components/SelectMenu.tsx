import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import ChevronIcon from './ChevronIcon';
import styles from '../App.module.css';

export interface SelectMenuOption {
  value: string;
  label: string;
  /** 可选的平台图标等前置图形。 */
  icon?: ReactNode;
}

interface SelectMenuProps {
  value: string;
  options: SelectMenuOption[];
  onChange: (value: string) => void;
  /** 按钮与列表的无障碍名称。 */
  label: string;
  disabled?: boolean;
  /** 附加到外层容器的类名，例如右侧对齐。 */
  className?: string;
  /** 菜单与触发按钮的对齐方向。 */
  align?: 'start' | 'end';
}

/**
 * 自绘下拉菜单：系统原生 `<select>` 在不同平台上外观与弹出层差异明显，
 * 这里统一成应用内的按钮 + 列表。保持焦点在按钮上，用方向键移动高亮，
 * Enter / 空格选中，Esc 关闭。
 */
export default function SelectMenu({
  value,
  options,
  onChange,
  label,
  disabled,
  className,
  align = 'start',
}: SelectMenuProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const listId = useId();

  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const selected = options[selectedIndex] ?? null;
  // 只有一个选项时没有可展开的内容，保持控件外观但不弹菜单。
  const interactive = options.length > 1;

  useEffect(() => {
    if (!open) return;
    setActiveIndex(selectedIndex);
  }, [open, selectedIndex]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, [open]);

  useEffect(() => {
    if (!disabled) return;
    setOpen(false);
  }, [disabled]);

  const choose = (index: number) => {
    const option = options[index];
    if (!option) return;
    onChange(option.value);
    setOpen(false);
    buttonRef.current?.focus();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        if (!open) {
          setOpen(true);
        } else {
          setActiveIndex((index) => Math.min(index + 1, options.length - 1));
        }
        break;
      case 'ArrowUp':
        event.preventDefault();
        if (!open) {
          setOpen(true);
        } else {
          setActiveIndex((index) => Math.max(index - 1, 0));
        }
        break;
      case 'Home':
        if (open) {
          event.preventDefault();
          setActiveIndex(0);
        }
        break;
      case 'End':
        if (open) {
          event.preventDefault();
          setActiveIndex(options.length - 1);
        }
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        if (open) {
          choose(activeIndex);
        } else {
          setOpen(true);
        }
        break;
      case 'Escape':
        if (open) {
          event.preventDefault();
          setOpen(false);
        }
        break;
      case 'Tab':
        setOpen(false);
        break;
      default:
        break;
    }
  };

  return (
    <div className={`${styles.selectMenu} ${className ?? ''}`} ref={wrapperRef}>
      {interactive ? (
        <button
          type="button"
          ref={buttonRef}
          className={styles.selectMenuTrigger}
          onClick={() => setOpen((current) => !current)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          data-open={open || undefined}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={interactive ? listId : undefined}
          aria-label={label}
        >
          {selected?.icon ? (
            <span className={styles.selectMenuIcon} aria-hidden>
              {selected.icon}
            </span>
          ) : null}
          <span className={styles.selectMenuValue}>
            {selected?.label ?? '—'}
          </span>
          <ChevronIcon direction="down" />
        </button>
      ) : (
        <span
          className={styles.selectMenuTrigger}
          data-static
          aria-label={`${label}：${selected?.label ?? ''}`}
        >
          {selected?.icon ? (
            <span className={styles.selectMenuIcon} aria-hidden>
              {selected.icon}
            </span>
          ) : null}
          <span className={styles.selectMenuValue}>
            {selected?.label ?? '—'}
          </span>
          <ChevronIcon direction="down" />
        </span>
      )}

      {interactive ? (
        <ul
          className={styles.selectMenuList}
          id={listId}
          role="listbox"
          aria-label={label}
          data-align={align}
          data-open={open || undefined}
        >
          {options.map((option, index) => (
            <li
              key={option.value}
              className={styles.selectMenuOption}
              role="option"
              aria-selected={option.value === value}
              data-active={index === activeIndex || undefined}
              data-selected={option.value === value || undefined}
              onPointerEnter={() => setActiveIndex(index)}
              onClick={() => choose(index)}
            >
              {option.icon ? (
                <span className={styles.selectMenuIcon} aria-hidden>
                  {option.icon}
                </span>
              ) : (
                <span className={styles.selectMenuIcon} aria-hidden />
              )}
              <span className={styles.selectMenuOptionLabel}>{option.label}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
