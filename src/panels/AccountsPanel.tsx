import { UsersThree } from '@phosphor-icons/react';
import type { AuthState } from '../lib/desktop';
import { useAppState } from '../lib/store';
import styles from '../App.module.css';

const AUTH_LABELS: Record<AuthState, string> = {
  unknown: '授权未确认',
  ok: '已授权',
  'needs-reauth': '需要重新授权',
};

const PLATFORM_LABELS: Record<string, string> = {
  codex: 'Codex',
};

function latestSnapshotEpoch(account: {
  products: { snapshot: { checkedAtEpoch: number } | null }[];
}): number | null {
  return account.products.reduce<number | null>((latest, product) => {
    const current = product.snapshot?.checkedAtEpoch ?? null;
    if (current === null) return latest;
    return latest === null ? current : Math.max(latest, current);
  }, null);
}

function checkedAtLabel(epoch: number | null): string {
  if (epoch === null) return '尚未获取额度';
  return `最近更新 ${new Date(epoch * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })}`;
}

export default function AccountsPanel() {
  const { state, ready } = useAppState();

  if (ready && state.accounts.length === 0) {
    return (
      <section>
        <div className={styles.empty}>
          <UsersThree size={30} weight="duotone" className={styles.emptyIcon} />
          <p className={styles.emptyTitle}>还没有托管账户</p>
          <p className={styles.usageNote}>
            账户记录由 Rust 管理；添加、改名、重新授权与移除依赖各平台的独立授权方案，
            验证完成后再启用。
          </p>
          <button type="button" className={styles.buttonGhost} disabled title="授权方案验证后启用">
            添加账户
          </button>
        </div>
      </section>
    );
  }

  return (
    <section>
      <h2 className={styles.groupTitle}>已接入账户</h2>
      <article className={`${styles.card} ${styles.listCard}`}>
        {state.accounts.map((account) => (
          <div className={styles.pendingRow} key={account.accountId}>
            <div className={styles.pendingHead}>
              <h3>{account.displayName}</h3>
              <span className={styles.planTag}>{AUTH_LABELS[account.authState]}</span>
            </div>
            <p className={styles.usageNote}>
              {PLATFORM_LABELS[account.platform] ?? account.platform}
              {' · '}
              {account.products.map((product) => product.displayName).join('、')}
            </p>
            <p className={styles.usageNote}>
              {checkedAtLabel(latestSnapshotEpoch(account))}
            </p>
          </div>
        ))}
      </article>

      <p className={`${styles.settingHint} ${styles.groupNote}`}>
        当前阶段只接入本机 Codex 登录态，不读取或保存登录凭证。添加其他账户、改名、
        重新授权与移除仍需各平台的独立授权能力验证后再启用。
      </p>
      <div className={styles.accountsActions}>
        <button type="button" className={styles.buttonGhost} disabled title="授权方案验证后启用">
          添加账户
        </button>
      </div>
    </section>
  );
}
