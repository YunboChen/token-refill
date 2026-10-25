import styles from '../App.module.css';

/** 轻量折线箭头：与图标包的填充三角相比更接近总览卡的克制风格。 */
export default function ChevronIcon({
  direction,
}: {
  direction: 'down' | 'right';
}) {
  return (
    <svg
      className={styles.chevronIcon}
      viewBox="0 0 16 16"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={
          direction === 'down'
            ? 'M4.5 6.5 8 10l3.5-3.5'
            : 'M6.5 4.5 10 8l-3.5 3.5'
        }
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}