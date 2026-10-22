import type { ReactNode } from 'react';
import { platformMeta } from '../lib/platforms';
import PlatformLogo from './PlatformLogo';
import styles from '../App.module.css';

interface PlatformHeadingProps {
  platform: string;
  /** 该平台下的账户数；未接入的平台不传。 */
  count?: number;
  /** 右侧状态文案，如「待接入」。 */
  status?: string;
  /** 覆盖平台默认说明。 */
  description?: string;
  /** 标题行右侧的操作按钮。 */
  action?: ReactNode;
  /** 渲染为卡片内的标题带，而不是独立标题行。 */
  embedded?: boolean;
  /** 平台是否已有账户；决定徽标是否使用强调色。 */
  active?: boolean;
}

/** 平台分组标题：徽标、名称、厂商、说明，以及账户数/状态/操作。 */
export default function PlatformHeading({
  platform,
  count,
  status,
  description,
  action,
  embedded = false,
  active,
}: PlatformHeadingProps) {
  const meta = platformMeta(platform);
  const isActive = active ?? meta.available;

  return (
    <div className={embedded ? styles.platformCardHead : styles.platformHeading}>
      <span
        className={styles.platformBadge}
        data-active={isActive || undefined}
        aria-hidden="true"
      >
        <PlatformLogo platform={platform} fallback={meta.mark} />
      </span>
      <div className={styles.platformIdentity}>
        <div className={styles.platformNameRow}>
          <h2 className={styles.platformName}>{meta.name}</h2>
          <span className={styles.platformVendor}>{meta.vendor}</span>
        </div>
        <p className={styles.platformDescription}>{description ?? meta.description}</p>
      </div>
      {count !== undefined || status || action ? (
        <div className={styles.platformAside}>
          {count !== undefined ? (
            <span className={styles.platformCount}>{count} 个账户</span>
          ) : null}
          {status ? <span className={styles.platformStatus}>{status}</span> : null}
          {action}
        </div>
      ) : null}
    </div>
  );
}