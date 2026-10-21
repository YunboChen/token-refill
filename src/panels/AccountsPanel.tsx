import { UsersThree } from '@phosphor-icons/react';
import styles from '../App.module.css';

export default function AccountsPanel() {
  return (
    <section>
      <h1 className={styles.panelTitle}>账户</h1>
      <p className={styles.subtitle}>管理已接入的账户、授权状态与关联套餐。</p>

      <div className={styles.empty}>
        <UsersThree size={30} weight="duotone" className={styles.emptyIcon} />
        <p className={styles.emptyTitle}>还没有托管账户</p>
        <p className={styles.usageNote}>
          添加、改名、重新授权与移除依赖账户模型和授权流程，对应后端契约仍在设计中，本版本尚未启用。
          Codex 额度目前通过本机 CLI 直接查询，并在总览展示；接入账户体系后将按账户归类到这里。
        </p>
        <button type="button" className={styles.buttonGhost} disabled title="账户后端尚未实现">
          添加账户
        </button>
      </div>
    </section>
  );
}
