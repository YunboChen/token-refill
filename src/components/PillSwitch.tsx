import { useId, type CSSProperties } from 'react';
import styles from '../App.module.css';

export interface PillSwitchOption {
  value: string;
  label: string;
}

/**
 * 应用统一的胶囊切换：与设置页的刷新间隔一致，用单个滑块按选项索引平移。
 * 滑块宽度按选项数均分，`equal` 让不同长度的标签也保持等宽，避免高亮
 * 与文字对不齐。
 */
export default function PillSwitch({
  value,
  options,
  onChange,
  label,
  equal,
}: {
  value: string;
  options: PillSwitchOption[];
  onChange: (value: string) => void;
  /** 单选组的无障碍名称。 */
  label: string;
  equal?: boolean;
}) {
  const name = useId();
  const index = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );

  return (
    <div
      className={`${styles.pills} ${equal ? styles.pillsEqual : ''}`}
      role="radiogroup"
      aria-label={label}
      style={
        {
          '--pill-index': index,
          '--pill-count': options.length,
        } as CSSProperties
      }
    >
      <span className={styles.pillThumb} aria-hidden="true" />
      {options.map((option) => (
        <label className={styles.pill} key={option.value}>
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={option.value === value}
            onChange={() => onChange(option.value)}
            aria-label={option.label}
          />
          <span className={styles.pillText}>{option.label}</span>
        </label>
      ))}
    </div>
  );
}
