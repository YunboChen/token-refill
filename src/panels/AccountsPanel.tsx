import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, m } from 'motion/react';
import {
  ArrowClockwise,
  CircleNotch,
  LinkSimple,
  PencilSimple,
  Plus,
  Trash,
  X,
} from '@phosphor-icons/react';
import {
  onAccountLoginChanged,
  type AccountState,
  type AuthState,
  type LoginStart,
  type ProductState,
} from '../lib/desktop';
import {
  accountPlatformGroups,
  pendingPlatforms,
  platformMeta,
} from '../lib/platforms';
import { useAppState } from '../lib/store';
import PlatformHeading from '../components/PlatformHeading';
import PlatformLogo from '../components/PlatformLogo';
import styles from '../App.module.css';

const AUTH_LABELS: Record<AuthState, string> = {
  unknown: '授权未确认',
  ok: '已授权',
  'needs-reauth': '需要重新授权',
};

type DialogPlatform = 'codex' | 'bailian';

const PLATFORM_CHOICES: {
  key: DialogPlatform;
  /** 添加位与弹窗标题共用，中英文混排的间距在这里一次写清 */
  addLabel: string;
  /** 标题下方一行，说明平台与授权方式 */
  subtitle: string;
  /** 授权流程要点：分条展示，避免整段文字堆在弹窗里 */
  steps: string[];
}[] = [
  {
    key: 'codex',
    addLabel: '添加 Codex 账户',
    subtitle: 'ChatGPT 订阅 · 官方浏览器授权',
    steps: [
      '在系统浏览器中完成 Codex 官方授权',
      '每个账户使用独立授权目录',
      '不读取或修改本机 Codex 的默认登录',
    ],
  },
  {
    key: 'bailian',
    addLabel: '添加百炼账户',
    subtitle: '阿里云 · Token Plan / Coding Plan',
    steps: [
      '在系统浏览器中登录要接入的阿里云账号',
      '控制台会话令牌经本机回环地址直接交给应用',
      '不经过前端，也不写入外部工具的配置',
    ],
  },
];

/** 添加入口文案；未知平台退回通用文案，只在平台已开放接入时使用。 */
function addAccountLabel(platform: string): string {
  return (
    PLATFORM_CHOICES.find((item) => item.key === platform)?.addLabel ?? '添加账户'
  );
}

type DialogState =
  | { kind: 'add-form'; platform: DialogPlatform }
  | {
      kind: 'login';
      platform: DialogPlatform;
      mode: 'add' | 'reauth';
      start: LoginStart;
      phase: 'waiting' | 'succeeded' | 'failed' | 'canceled';
      message: string | null;
    }
  | { kind: 'rename'; accountId: string; value: string }
  | { kind: 'remove'; accountId: string; displayName: string };

function checkedAtLabel(epoch: number | null): string {
  if (epoch === null) return '尚未获取额度';
  return `最近更新 ${new Date(epoch * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })}`;
}

function planTierLabel(tier: string): string {
  return tier[0].toUpperCase() + tier.slice(1);
}

function planStateLabel(product: ProductState): string {
  if (product.active === false) return '未开通';
  if (product.query.refreshing) return '查询中…';
  if (product.snapshot) return checkedAtLabel(product.snapshot.checkedAtEpoch);
  if (product.query.lastError) return '查询失败';
  return '待查询';
}

/** 套餐状态的语义，用于给右侧时间/提示上色，不参与文案。 */
function planStateKey(
  product: ProductState,
): 'inactive' | 'busy' | 'error' | 'ok' | 'idle' {
  if (product.active === false) return 'inactive';
  if (product.query.refreshing) return 'busy';
  if (product.snapshot) return 'ok';
  if (product.query.lastError) return 'error';
  return 'idle';
}

/** 账户徽标取名称首字符，西文统一大写，与平台徽标区分开。 */
function accountMark(name: string): string {
  const first = Array.from(name.trim())[0];
  if (!first) return '?';
  return /[a-z]/i.test(first) ? first.toUpperCase() : first;
}

export default function AccountsPanel() {
  const {
    state,
    desktop,
    refresh,
    beginLogin,
    beginBailianLogin,
    cancelLogin,
    renameAccount,
    removeAccount,
    openExternal,
  } = useAppState();
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [draftName, setDraftName] = useState('');
  const [starting, setStarting] = useState(false);
  // 改名与移除都在弹窗里确认，提交期间锁住按钮避免重复请求。
  const [acting, setActing] = useState(false);

  // 授权在 Rust 后台线程完成；这里只接收脱敏状态并更新对话框。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onAccountLoginChanged((event) => {
      if (disposed) return;
      setDialog((current) => {
        if (
          !current
          || current.kind !== 'login'
          || current.start.loginId !== event.loginId
        ) {
          return current;
        }
        return {
          ...current,
          phase: event.status,
          message: event.message,
        };
      });
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // 事件订阅失败时仍可通过重新打开窗口读取账户状态。
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  const startAdd = async (platform: DialogPlatform) => {
    setStarting(true);
    const name = draftName.trim() || undefined;
    const start =
      platform === 'bailian'
        ? await beginBailianLogin(undefined, name)
        : await beginLogin(undefined, name);
    setStarting(false);
    if (!start) {
      setDialog(null);
      return;
    }
    setDialog({
      kind: 'login',
      platform,
      mode: 'add',
      start,
      phase: 'waiting',
      message: null,
    });
    void openExternal(start.authUrl);
  };

  const startReauth = async (account: AccountState) => {
    const platform: DialogPlatform = account.platform === 'bailian' ? 'bailian' : 'codex';
    const start =
      platform === 'bailian'
        ? await beginBailianLogin(account.accountId)
        : await beginLogin(account.accountId);
    if (!start) return;
    setDialog({
      kind: 'login',
      platform,
      mode: 'reauth',
      start,
      phase: 'waiting',
      message: null,
    });
    void openExternal(start.authUrl);
  };

  const closeDialog = () => {
    if (dialog?.kind === 'login' && dialog.phase === 'waiting') {
      void cancelLogin(dialog.start.loginId);
    }
    setDialog(null);
  };

  const saveRename = async () => {
    if (dialog?.kind !== 'rename' || acting) return;
    const value = dialog.value.trim();
    if (!value) return;
    setActing(true);
    const ok = await renameAccount(dialog.accountId, value);
    setActing(false);
    if (ok) setDialog(null);
  };

  const confirmRemove = async () => {
    if (dialog?.kind !== 'remove' || acting) return;
    setActing(true);
    const ok = await removeAccount(dialog.accountId);
    setActing(false);
    if (ok) setDialog(null);
  };

  const openRenameDialog = (account: AccountState) => {
    setActing(false);
    setDialog({
      kind: 'rename',
      accountId: account.accountId,
      value: account.displayName,
    });
  };

  const openRemoveDialog = (account: AccountState) => {
    setActing(false);
    setDialog({
      kind: 'remove',
      accountId: account.accountId,
      displayName: account.displayName,
    });
  };

  const groups = accountPlatformGroups(state.accounts);
  const unavailable = pendingPlatforms();

  const openAddDialog = (platform: DialogPlatform) => {
    setDraftName('');
    setDialog({ kind: 'add-form', platform });
  };

  // 平台由入口决定：从哪个平台分组点进来，就只给哪个平台添加账户，
  // 弹窗内不再提供切换，避免加到别的分组里。
  const dialogPlatform =
    dialog && 'platform' in dialog ? dialog.platform : null;
  const choice =
    PLATFORM_CHOICES.find((item) => item.key === dialogPlatform) ?? PLATFORM_CHOICES[0];

  return (
    <section>
      <div className={styles.platformStack}>
        {groups.map((group) => (
          <section className={styles.platformPanel} key={group.platform}>
            <div className={styles.platformSectionHead}>
              <PlatformHeading
                active
                platform={group.platform}
                count={group.accounts.length > 0 ? group.accounts.length : undefined}
              />
            </div>

            <div className={styles.accountGrid}>
              {group.accounts.map((account) => {
                const managed = account.source === 'managed';
                const busy = account.products.some(
                  (product) => product.query.refreshing,
                );
                // 只列已开通的套餐；尚未确认（active === null）的也先展示。
                // 若平台确认全部未开通，则保留列表并标记为“未开通”。
                const activated = account.products.filter(
                  (product) => product.active !== false,
                );
                const plans = activated.length > 0 ? activated : account.products;
                return (
                  <article className={styles.accountTile} key={account.accountId}>
                    <div className={styles.accountIdentityCell}>
                      <span className={styles.accountMark} aria-hidden="true">
                        {accountMark(account.displayName)}
                      </span>
                      <div className={styles.accountIdentity}>
                        <div className={styles.accountNameRow}>
                          <h3 className={styles.accountName}>
                            {account.displayName}
                          </h3>
                          <span
                            className={styles.authStatus}
                            data-state={account.authState}
                          >
                            <span className={styles.authDot} aria-hidden="true" />
                            {AUTH_LABELS[account.authState]}
                          </span>
                        </div>
                        <p className={styles.accountMeta}>
                          {account.platform === 'bailian'
                            ? '控制台授权'
                            : managed
                              ? '独立授权'
                              : '本机登录态'}
                          {account.identityHint ? ` · ${account.identityHint}` : ''}
                        </p>
                      </div>
                    </div>

                    {plans.length > 0 ? (
                      <div className={styles.planPanel}>
                        {plans.map((product) => {
                          const state = planStateKey(product);
                          return (
                            <div
                              className={styles.planRow}
                              key={product.productId}
                              data-inactive={
                                product.active === false || undefined
                              }
                            >
                              <span className={styles.planRowMain}>
                                <span className={styles.planRowName}>
                                  {product.displayName}
                                </span>
                                {product.planTier ? (
                                  <span className={styles.planTier}>
                                    {planTierLabel(product.planTier)}
                                  </span>
                                ) : null}
                              </span>
                              <span
                                className={styles.planRowState}
                                data-state={state}
                              >
                                {planStateLabel(product)}
                              </span>
                            </div>
                          );
                        })}
                      </div>
                    ) : (
                      <p className={styles.accountEmptyNote}>
                        该账户暂无已接入的套餐。
                      </p>
                    )}

                    <div className={styles.accountFoot}>
                      <button
                        type="button"
                        className={styles.accountRefresh}
                        onClick={() => void refresh(account.accountId)}
                        disabled={!desktop || busy}
                        aria-label={`刷新 ${account.displayName} 的额度`}
                        title="刷新额度"
                      >
                        <ArrowClockwise
                          size={13}
                          className={busy ? styles.spin : undefined}
                          aria-hidden
                        />
                        刷新
                      </button>
                      {managed ? (
                        <div className={styles.accountActions}>
                          <button
                            type="button"
                            className={styles.miniButton}
                            onClick={() => void startReauth(account)}
                            disabled={!desktop}
                            aria-label="重新授权"
                            title="重新授权"
                          >
                            <LinkSimple size={14} />
                          </button>
                          <button
                            type="button"
                            className={styles.miniButton}
                            onClick={() => openRenameDialog(account)}
                            disabled={!desktop}
                            aria-label="重命名"
                            title="重命名"
                          >
                            <PencilSimple size={14} />
                          </button>
                          <button
                            type="button"
                            className={styles.miniButton}
                            onClick={() => openRemoveDialog(account)}
                            disabled={!desktop}
                            aria-label="移除账户"
                            title="移除账户"
                            data-danger
                          >
                            <Trash size={14} />
                          </button>
                        </div>
                      ) : null}
                    </div>
                  </article>
                );
              })}

              {/* 添加位补在网格末尾：单账户时也能撑满一行，不必回到页头找入口 */}
              {group.meta.available ? (
                <button
                  type="button"
                  className={styles.accountAddTile}
                  onClick={() =>
                    openAddDialog(group.platform === 'bailian' ? 'bailian' : 'codex')
                  }
                  disabled={!desktop}
                  title={
                    desktop
                      ? addAccountLabel(group.platform)
                      : '浏览器预览无法发起授权'
                  }
                >
                  <span className={styles.accountAddIcon} aria-hidden="true">
                    <Plus size={15} weight="bold" />
                  </span>
                  <span className={styles.accountAddLabel}>
                    {addAccountLabel(group.platform)}
                  </span>
                </button>
              ) : null}
            </div>
          </section>
        ))}

        {unavailable.map((meta) => (
          <section className={styles.platformSection} key={meta.key}>
            <div className={styles.platformSectionHead}>
              <PlatformHeading
                platform={meta.key}
                status="待接入"
                active={false}
              />
            </div>
            <p className={styles.plannedNote}>
              {meta.name}账户尚未开放接入。开放后会在这里单独管理，与现有账户互不影响。
            </p>
          </section>
        ))}
      </div>

      {/* 用 portal 挂到 body：面板容器带进场 transform，会成为 fixed 后代的包含块，
          就地渲染会让蒙层只盖住内容区。 */}
      {createPortal(
        <AnimatePresence>
          {dialog ? (
            <m.div
              key="account-dialog"
              className={styles.dialogBackdrop}
              role="presentation"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              // 退场单独给定时长：蒙层比弹窗多留一点，避免弹窗还没退干净背景就亮了。
              exit={{ opacity: 0, transition: { duration: 0.34, ease: 'easeIn' } }}
              transition={{ duration: 0.16, ease: 'easeOut' }}
              onMouseDown={(event) => {
                if (event.target === event.currentTarget) closeDialog();
              }}
            >
              <m.div
                className={styles.dialog}
                role="dialog"
                aria-modal="true"
                aria-labelledby="account-dialog-title"
                initial={{ opacity: 0, scale: 0.96, y: 10 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                // 与进场对称：退回进场起点并缓入淡出，退场才有可感知的“收回”动作。
                exit={{
                  opacity: 0,
                  scale: 0.96,
                  y: 10,
                  transition: { duration: 0.3, ease: 'easeIn' },
                }}
                transition={{ type: 'spring', stiffness: 460, damping: 34 }}
              >
                <button
                  type="button"
                  className={styles.dialogClose}
                  onClick={closeDialog}
                  aria-label="关闭"
                >
                  <X size={16} />
                </button>

                {dialog.kind === 'add-form' ? (
                  <>
                    <header className={styles.dialogHeader}>
                      <span
                        className={styles.platformBadge}
                        data-active
                        aria-hidden="true"
                      >
                        <PlatformLogo
                          platform={dialog.platform}
                          fallback={platformMeta(dialog.platform).mark}
                        />
                      </span>
                      <div className={styles.dialogHeading}>
                        <h2 id="account-dialog-title">{choice.addLabel}</h2>
                        <p className={styles.dialogSubtitle}>{choice.subtitle}</p>
                      </div>
                    </header>
                    <ul className={styles.dialogSteps} role="list">
                      {choice.steps.map((step) => (
                        <li className={styles.dialogStep} key={step}>
                          {step}
                        </li>
                      ))}
                    </ul>
                    <label className={styles.field}>
                      <span className={styles.fieldLabel}>账户名称（可选）</span>
                      <input
                        className={styles.textInput}
                        value={draftName}
                        onChange={(event) => setDraftName(event.target.value)}
                        placeholder={
                          dialog.platform === 'bailian'
                            ? '留空则显示为“百炼账户”'
                            : '留空则使用授权后的脱敏邮箱'
                        }
                        maxLength={60}
                      />
                    </label>
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={() => setDialog(null)}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className={styles.button}
                        onClick={() => void startAdd(dialog.platform)}
                        disabled={starting}
                      >
                        {starting
                          ? '正在启动…'
                          : dialog.platform === 'bailian'
                            ? '前往阿里云授权'
                            : '前往浏览器授权'}
                      </button>
                    </div>
                  </>
                ) : dialog.kind === 'rename' ? (
                  <form
                    className={styles.dialogForm}
                    onSubmit={(event) => {
                      event.preventDefault();
                      void saveRename();
                    }}
                  >
                    <h2 id="account-dialog-title">重命名账户</h2>
                    <input
                      className={styles.textInput}
                      value={dialog.value}
                      onChange={(event) => {
                        const value = event.target.value;
                        setDialog((current) =>
                          current?.kind === 'rename'
                            ? { ...current, value }
                            : current,
                        );
                      }}
                      maxLength={60}
                      aria-label="账户名称"
                      autoFocus
                    />
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                      >
                        取消
                      </button>
                      <button
                        type="submit"
                        className={styles.button}
                        disabled={
                          acting || !desktop || dialog.value.trim().length === 0
                        }
                      >
                        {acting ? '保存中…' : '保存'}
                      </button>
                    </div>
                  </form>
                ) : dialog.kind === 'remove' ? (
                  <>
                    <h2 id="account-dialog-title">移除账户</h2>
                    <p className={styles.usageNote}>
                      将移除「{dialog.displayName}」，并删除该账户在本地的授权副本；
                      外部工具与百炼账号的登录状态不受影响。
                    </p>
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className={styles.dangerButton}
                        onClick={() => void confirmRemove()}
                        disabled={acting || !desktop}
                      >
                        {acting ? '正在移除…' : '确认移除'}
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <header className={styles.dialogHeader}>
                      <span
                        className={styles.platformBadge}
                        data-active
                        aria-hidden="true"
                      >
                        <PlatformLogo
                          platform={dialog.platform}
                          fallback={platformMeta(dialog.platform).mark}
                        />
                      </span>
                      <div className={styles.dialogHeading}>
                        <h2 id="account-dialog-title">
                          {dialog.phase === 'succeeded'
                            ? '授权完成'
                            : dialog.phase === 'failed'
                              ? '授权失败'
                              : dialog.phase === 'canceled'
                                ? '已取消授权'
                                : dialog.mode === 'reauth'
                                  ? '重新授权'
                                  : dialog.platform === 'bailian'
                                    ? '登录阿里云百炼'
                                    : '在浏览器中完成授权'}
                        </h2>
                        <p className={styles.dialogSubtitle}>{choice.subtitle}</p>
                      </div>
                    </header>

                    {dialog.phase === 'waiting' ? (
                      <>
                        <p className={styles.usageNote}>
                          {dialog.platform === 'bailian'
                            ? '请登录要接入的阿里云账号并确认授权，控制台会话令牌会通过本机回环地址直接交给应用。'
                            : '请登录要接入的 ChatGPT 账户并确认授权。如果浏览器已登录其他账户，请先切换账户或使用无痕窗口。'}
                        </p>
                        <div className={styles.authBlock}>
                          <span className={styles.fieldLabel}>授权链接</span>
                          <p className={styles.authUrl}>{dialog.start.authUrl}</p>
                        </div>
                        <p className={styles.loginStatus}>
                          <CircleNotch
                            size={13}
                            className={styles.spin}
                            aria-hidden
                          />
                          等待授权完成…
                        </p>
                        <div className={styles.dialogActions}>
                          <button
                            type="button"
                            className={styles.buttonGhost}
                            onClick={closeDialog}
                          >
                            取消授权
                          </button>
                          <button
                            type="button"
                            className={styles.button}
                            onClick={() => void openExternal(dialog.start.authUrl)}
                          >
                            重新打开授权页面
                          </button>
                        </div>
                      </>
                    ) : dialog.phase === 'succeeded' ? (
                      <>
                        <p className={styles.usageNote}>
                          账户已接入，额度数据会在后台刷新后显示。
                        </p>
                        <div className={styles.dialogActions}>
                          <button
                            type="button"
                            className={styles.button}
                            onClick={() => setDialog(null)}
                          >
                            完成
                          </button>
                        </div>
                      </>
                    ) : (
                      <>
                        <p className={styles.noticeDanger}>
                          {dialog.message ?? '授权未完成，请重新尝试。'}
                        </p>
                        <div className={styles.dialogActions}>
                          <button
                            type="button"
                            className={styles.buttonGhost}
                            onClick={() => setDialog(null)}
                          >
                            关闭
                          </button>
                        </div>
                      </>
                    )}
                  </>
                )}
              </m.div>
            </m.div>
          ) : null}
        </AnimatePresence>,
        document.body,
      )}
    </section>
  );
}