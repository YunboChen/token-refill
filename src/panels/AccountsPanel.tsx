import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, m } from 'motion/react';
import {
  ArrowClockwise,
  LinkSimple,
  PencilSimple,
  Plus,
  Trash,
  UsersThree,
  X,
} from '@phosphor-icons/react';
import {
  onAccountLoginChanged,
  type AccountState,
  type AuthState,
  type LoginStart,
  type ProductState,
} from '../lib/desktop';
import { groupAccountsByPlatform, pendingPlatforms } from '../lib/platforms';
import { useAppState } from '../lib/store';
import PlatformHeading from '../components/PlatformHeading';
import styles from '../App.module.css';

const AUTH_LABELS: Record<AuthState, string> = {
  unknown: '授权未确认',
  ok: '已授权',
  'needs-reauth': '需要重新授权',
};

type DialogPlatform = 'codex' | 'bailian';

const PLATFORM_CHOICES: {
  key: DialogPlatform;
  name: string;
  hint: string;
  note: string;
}[] = [
  {
    key: 'codex',
    name: 'Codex',
    hint: 'ChatGPT 订阅 · 官方浏览器授权',
    note: '接下来会打开系统浏览器完成 Codex 官方授权。每个账户使用独立授权目录，不会读取或修改本机 Codex 的默认登录。',
  },
  {
    key: 'bailian',
    name: '百炼',
    hint: 'Token Plan · Coding Plan · 阿里云控制台',
    note: '接下来会打开阿里云百炼控制台登录页。登录成功后，控制台会话令牌会通过本机回环地址直接交给应用，不经过前端，也不会写入外部工具的配置。',
  },
];

type DialogState =
  | { kind: 'add-form'; platform: DialogPlatform }
  | {
      kind: 'login';
      platform: DialogPlatform;
      mode: 'add' | 'reauth';
      start: LoginStart;
      phase: 'waiting' | 'succeeded' | 'failed' | 'canceled';
      message: string | null;
    };

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

export default function AccountsPanel() {
  const {
    state,
    ready,
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
  const [renaming, setRenaming] = useState<{ accountId: string; value: string } | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

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
    if (!renaming) return;
    const ok = await renameAccount(renaming.accountId, renaming.value);
    if (ok) setRenaming(null);
  };

  const confirmRemove = async (accountId: string) => {
    const ok = await removeAccount(accountId);
    if (ok) setConfirming(null);
  };

  const groups = groupAccountsByPlatform(state.accounts);
  const unavailable = pendingPlatforms();

  const openAddDialog = (platform: DialogPlatform = 'codex') => {
    setDraftName('');
    setDialog({ kind: 'add-form', platform });
  };

  const dialogPlatform =
    dialog?.kind === 'add-form' ? dialog.platform : dialog?.platform ?? 'codex';
  const choice =
    PLATFORM_CHOICES.find((item) => item.key === dialogPlatform) ?? PLATFORM_CHOICES[0];

  return (
    <section>
      {ready && state.accounts.length === 0 ? (
        <div className={styles.empty}>
          <span className={styles.emptyIconWrap}>
            <UsersThree size={22} weight="duotone" aria-hidden />
          </span>
          <p className={styles.emptyTitle}>还没有接入账户</p>
          <p className={styles.usageNote}>
            添加账户后，每个账户会使用独立的授权副本，互不影响。
          </p>
          <button
            type="button"
            className={styles.button}
            onClick={() => openAddDialog()}
            disabled={!desktop}
            title={desktop ? '添加账户' : '浏览器预览无法发起授权'}
          >
            <Plus size={15} weight="bold" aria-hidden />
            添加账户
          </button>
        </div>
      ) : null}

      <div className={styles.platformStack}>
        {groups.map((group) => (
          <article
            className={`${styles.card} ${styles.platformCard}`}
            key={group.platform}
          >
            <PlatformHeading
              embedded
              active
              platform={group.platform}
              count={group.accounts.length}
              action={
                group.meta.available ? (
                  <button
                    type="button"
                    className={styles.platformAddButton}
                    onClick={() =>
                      openAddDialog(group.platform === 'bailian' ? 'bailian' : 'codex')
                    }
                    disabled={!desktop}
                    title={
                      desktop
                        ? `添加 ${group.meta.name} 账户`
                        : '浏览器预览无法发起授权'
                    }
                  >
                    <Plus size={13} weight="bold" aria-hidden />
                    添加账户
                  </button>
                ) : null
              }
            />
            {group.accounts.map((account) => {
              const managed = account.source === 'managed';
              const busy = account.products.some((product) => product.query.refreshing);
              // 只列已开通的套餐；尚未确认（active === null）的也先展示。
              // 若平台确认全部未开通，则保留列表并标记为“未开通”。
              const activated = account.products.filter(
                (product) => product.active !== false,
              );
              const plans = activated.length > 0 ? activated : account.products;
              return (
                <div className={styles.accountRow} key={account.accountId}>
                  <div className={styles.accountRowHead}>
                    <div className={styles.accountTitle}>
                      <h3>{account.displayName}</h3>
                      <span
                        className={styles.authStatus}
                        data-state={account.authState}
                      >
                        <span className={styles.authDot} aria-hidden="true" />
                        {AUTH_LABELS[account.authState]}
                      </span>
                    </div>
                    <div className={styles.accountActions}>
                      <button
                        type="button"
                        className={styles.miniButton}
                        onClick={() => void refresh(account.accountId)}
                        disabled={!desktop || busy}
                        aria-label={`刷新 ${account.displayName} 的额度`}
                        title="刷新额度"
                      >
                        <ArrowClockwise
                          size={14}
                          className={busy ? styles.spin : undefined}
                        />
                      </button>
                      {managed ? (
                        <>
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
                            onClick={() =>
                              setRenaming({
                                accountId: account.accountId,
                                value: account.displayName,
                              })
                            }
                            disabled={!desktop}
                            aria-label="重命名"
                            title="重命名"
                          >
                            <PencilSimple size={14} />
                          </button>
                          <button
                            type="button"
                            className={styles.miniButton}
                            onClick={() => setConfirming(account.accountId)}
                            disabled={!desktop}
                            aria-label="移除账户"
                            title="移除账户"
                            data-danger
                          >
                            <Trash size={14} />
                          </button>
                        </>
                      ) : null}
                    </div>
                  </div>
                  <p className={styles.accountMeta}>
                    {account.platform === 'bailian'
                      ? '控制台授权'
                      : managed
                        ? '独立授权'
                        : '本机登录态'}
                    {account.identityHint ? ` · ${account.identityHint}` : ''}
                  </p>
                  <div className={styles.planList}>
                    {plans.map((product) => (
                      <div
                        className={styles.planRow}
                        key={product.productId}
                        data-inactive={product.active === false || undefined}
                      >
                        <span className={styles.planRowName}>
                          {product.displayName}
                        </span>
                        {product.planTier ? (
                          <span className={styles.planTag}>
                            {planTierLabel(product.planTier)}
                          </span>
                        ) : null}
                        <span className={styles.planRowState}>
                          {planStateLabel(product)}
                        </span>
                      </div>
                    ))}
                  </div>

                  {renaming?.accountId === account.accountId ? (
                    <form
                      className={styles.inlineForm}
                      onSubmit={(event) => {
                        event.preventDefault();
                        void saveRename();
                      }}
                    >
                      <input
                        className={styles.textInput}
                        value={renaming.value}
                        onChange={(event) =>
                          setRenaming({ ...renaming, value: event.target.value })
                        }
                        maxLength={60}
                        aria-label="账户名称"
                        autoFocus
                      />
                      <button type="submit" className={styles.button} disabled={!desktop}>
                        保存
                      </button>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={() => setRenaming(null)}
                      >
                        取消
                      </button>
                    </form>
                  ) : null}

                  {confirming === account.accountId ? (
                    <div className={styles.inlineConfirm}>
                      <span className={styles.usageNote}>
                        移除后本应用会删除该账户在本地的授权副本；
                        外部工具与百炼账号的登录状态不受影响。
                      </span>
                      <div className={styles.inlineConfirmActions}>
                        <button
                          type="button"
                          className={styles.dangerButton}
                          onClick={() => void confirmRemove(account.accountId)}
                          disabled={!desktop}
                        >
                          确认移除
                        </button>
                        <button
                          type="button"
                          className={styles.buttonGhost}
                          onClick={() => setConfirming(null)}
                        >
                          取消
                        </button>
                      </div>
                    </div>
                  ) : null}
                </div>
              );
            })}
          </article>
        ))}

        {unavailable.map((meta) => (
          <article
            key={meta.key}
            className={`${styles.card} ${styles.platformCard} ${styles.platformCardMuted}`}
          >
            <PlatformHeading
              embedded
              platform={meta.key}
              status="待接入"
              active={false}
            />
            <p className={styles.plannedNote}>
              {meta.name}账户尚未开放接入。开放后会在这里单独管理，与现有账户互不影响。
            </p>
          </article>
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
                    <h2 id="account-dialog-title">添加账户</h2>
                    <div
                      className={styles.platformPicker}
                      role="group"
                      aria-label="选择平台"
                    >
                      {PLATFORM_CHOICES.map((item) => (
                        <button
                          type="button"
                          key={item.key}
                          className={styles.platformPick}
                          data-active={dialog.platform === item.key || undefined}
                          aria-pressed={dialog.platform === item.key}
                          onClick={() =>
                            setDialog({ kind: 'add-form', platform: item.key })
                          }
                        >
                          <span className={styles.platformPickName}>{item.name}</span>
                          <span className={styles.platformPickHint}>{item.hint}</span>
                        </button>
                      ))}
                    </div>
                    <p className={styles.usageNote}>{choice.note}</p>
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
                        autoFocus
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
                ) : (
                  <>
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

                    {dialog.phase === 'waiting' ? (
                      <>
                        <p className={styles.usageNote}>
                          {dialog.platform === 'bailian'
                            ? '已打开系统浏览器。请登录要接入的阿里云账号并确认授权；控制台会话令牌会通过本机回环地址直接交给应用。等待期间请保持本应用运行。'
                            : '已打开系统浏览器。请登录要接入的 ChatGPT 账户并确认授权。如果浏览器已登录其他账户，请先切换账户或使用无痕窗口。等待期间请保持本应用运行。'}
                        </p>
                        <p className={styles.authUrl}>{dialog.start.authUrl}</p>
                        <p className={styles.loginStatus}>等待授权完成…</p>
                        <div className={styles.dialogActions}>
                          <button
                            type="button"
                            className={styles.buttonGhost}
                            onClick={() => void openExternal(dialog.start.authUrl)}
                          >
                            重新打开授权页面
                          </button>
                          <button
                            type="button"
                            className={styles.buttonGhost}
                            onClick={closeDialog}
                          >
                            取消授权
                          </button>
                        </div>
                      </>
                    ) : dialog.phase === 'succeeded' ? (
                      <>
                        <p className={styles.noticeStale}>
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