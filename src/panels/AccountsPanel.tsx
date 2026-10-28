import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, m } from 'motion/react';
import {
  ArrowClockwise,
  ArrowSquareOut,
  CheckCircle,
  CircleNotch,
  LinkSimple,
  LockSimple,
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
} from '../lib/platforms';
import { planTierLabel, sourceLabel } from '../lib/format';
import { useDialogA11y } from '../lib/useDialogA11y';
import { useAppState, type LoginAttempt } from '../lib/store';
import PlatformHeading from '../components/PlatformHeading';
import Toast from '../components/Toast';
import styles from '../App.module.css';

const AUTH_LABELS: Record<AuthState, string> = {
  unknown: '授权未确认',
  ok: '已授权',
  'needs-reauth': '需要重新授权',
};

type DialogPlatform =
  | 'codex'
  | 'bailian'
  | 'opencode-go'
  | 'commandcode'
  | 'antigravity';

/** 走浏览器授权流程的平台；其余平台在弹窗内直接提交凭据。 */
type LoginPlatform = 'codex' | 'bailian';

type DialogMode = 'add' | 'reauth';

interface PlatformChoice {
  /** 添加位与弹窗标题共用，中英文混排的间距在这里一次写清 */
  addLabel: string;
  /** 标题下方一行，说明平台与授权方式 */
  subtitle: string;
  /** 授权流程：分两步，标题说明做什么，正文说明怎么做 */
  steps: { title: string; text: string }[];
  /** 输入框下方的隐私说明，用一句话收住授权边界 */
  note: string;
  /** 账户名称输入框的占位文案 */
  namePlaceholder: string;
  /** 添加弹窗主按钮的文案 */
  authorizeLabel: string;
  /** 等待授权时的标题 */
  waitingTitle: string;
  /** 等待授权时的说明 */
  waitingText: string;
}

const PLATFORM_CHOICES: Record<DialogPlatform, PlatformChoice> = {
  codex: {
    addLabel: '添加 Codex 账户',
    subtitle: 'ChatGPT 订阅 · 官方浏览器授权',
    steps: [
      {
        title: '登录 ChatGPT',
        text: '在系统浏览器中完成 Codex 官方授权',
      },
      {
        title: '自动连接账户',
        text: '每个账户使用独立授权目录，不影响本机登录态',
      },
    ],
    note: '授权在本机完成，不读取或修改本机 Codex 的默认登录。',
    namePlaceholder: '留空则使用授权后的脱敏邮箱',
    authorizeLabel: '前往浏览器授权',
    waitingTitle: '在浏览器中完成授权',
    waitingText:
      '请登录要接入的 ChatGPT 账户并确认授权。如果浏览器已登录其他账户，请先切换账户或使用无痕窗口。',
  },
  bailian: {
    addLabel: '添加百炼账户',
    subtitle: '阿里云 · Token Plan / Coding Plan',
    steps: [
      {
        title: '登录阿里云',
        text: '在系统浏览器中登录要接入的阿里云账号',
      },
      {
        title: '自动连接账户',
        text: '控制台会话令牌经本机回环地址直接交给应用',
      },
    ],
    note: '授权信息仅在本机接收，不经过前端，也不写入外部工具的配置。',
    namePlaceholder: '留空则显示为“百炼账户”',
    authorizeLabel: '前往阿里云授权',
    waitingTitle: '登录阿里云百炼',
    waitingText:
      '请登录要接入的阿里云账号并确认授权，控制台会话令牌会通过本机回环地址直接交给应用。',
  },
  'opencode-go': {
    addLabel: '添加 OpenCode Go 账户',
    subtitle: 'Go / Go Plus · API Key',
    steps: [
      {
        title: '复制 API Key',
        text: '在 OpenCode 控制台订阅 Go 或 Go Plus 后复制 API Key',
      },
      {
        title: '保存到本机',
        text: '应用用它在后台读取 5 小时、每周与订阅月额度',
      },
    ],
    note: '密钥只保存在本机账户目录，不写入前端状态或日志。',
    namePlaceholder: '留空则显示为“OpenCode Go”',
    authorizeLabel: '验证并添加',
    waitingTitle: '接入 OpenCode Go',
    waitingText: '正在保存本机账户，无需在浏览器中操作。',
  },
  commandcode: {
    addLabel: '添加 Command Code 账户',
    subtitle: '订阅额度 · Provider API Key',
    steps: [
      {
        title: '复制 Provider API Key',
        text: '在 Command Code 控制台复制 Provider API Key（通常以 user_ 开头）',
      },
      {
        title: '保存到本机',
        text: '应用用它在后台读取滚动窗口与月度额度',
      },
    ],
    note: '密钥只保存在本机账户目录，不写入前端状态或日志。',
    namePlaceholder: '留空则显示为“Command Code”',
    authorizeLabel: '验证并添加',
    waitingTitle: '接入 Command Code',
    waitingText: '正在保存本机账户，无需在浏览器中操作。',
  },
  antigravity: {
    addLabel: '接入本机 Antigravity',
    subtitle: 'Gemini / Claude · 本机 agy 登录',
    steps: [
      {
        title: '确认已登录 agy',
        text: '需要已安装并登录 Antigravity CLI（agy 1.1.11 或更高版本）',
      },
      {
        title: '读取额度',
        text: '应用只调用 agy 官方的打印模式查询额度，不读取其登录令牌',
      },
    ],
    note: '接入后跟随本机 agy 登录态，不复制、不迁移其凭证。',
    namePlaceholder: '留空则显示为“Antigravity”',
    authorizeLabel: '接入本机 Antigravity',
    waitingTitle: '接入 Antigravity',
    waitingText: '正在读取本机 agy 登录态，无需在浏览器中操作。',
  },
};

const DIALOG_PLATFORMS = Object.keys(PLATFORM_CHOICES) as DialogPlatform[];

/** 需要用户填写 API Key 的平台；其余非登录平台直接复用本机登录态。 */
const API_KEY_PLATFORMS: DialogPlatform[] = ['opencode-go', 'commandcode'];

function isApiKeyPlatform(
  platform: DialogPlatform,
): platform is 'opencode-go' | 'commandcode' {
  return API_KEY_PLATFORMS.includes(platform);
}

function isDialogPlatform(platform: string): platform is DialogPlatform {
  return (DIALOG_PLATFORMS as string[]).includes(platform);
}

/** 添加入口文案；未知平台退回通用文案，只在平台已开放接入时使用。 */
function addAccountLabel(platform: string): string {
  return isDialogPlatform(platform)
    ? PLATFORM_CHOICES[platform].addLabel
    : '添加账户';
}

type DialogState =
  | {
      kind: 'add-form';
      platform: DialogPlatform;
      mode: DialogMode;
      /** 重新授权时更新的是已有账户的凭据，而不是新建账户。 */
      accountId?: string;
    }
  | {
      kind: 'login';
      platform: LoginPlatform;
      mode: DialogMode;
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
    addApiKeyAccount,
    replaceApiKey,
    addAntigravityAccount,
    cancelLogin,
    renameAccount,
    removeAccount,
    openExternal,
  } = useAppState();
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [draftName, setDraftName] = useState('');
  const [draftKey, setDraftKey] = useState('');
  // 操作失败只弹一次 toast，不占弹窗或页面的布局。
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  // 改名与移除都在弹窗里确认，提交期间锁住按钮避免重复请求。
  const [acting, setActing] = useState(false);

  // 平台与登录命令的映射集中一处：新增浏览器授权平台时 Record 会强制补齐，
  // 不再在 startAdd / startReauth 里写平台三元表达式。
  const loginCommands: Record<
    LoginPlatform,
    (accountId?: string, displayName?: string) => Promise<LoginAttempt>
  > = {
    codex: beginLogin,
    bailian: beginBailianLogin,
  };

  // 每次发起授权递增。弹窗在启动请求返回前关闭、或整个面板卸载时，
  // 晚到的结果按过期处理并立刻取消，不在 Rust 侧留下等待中的流程。
  const startAttemptRef = useRef(0);

  useEffect(() => () => {
    startAttemptRef.current += 1;
  }, []);

  // 授权流程由 Rust 后台线程持有，界面消失时必须显式取消：无论是关闭弹窗、
  // 切换到结果状态，还是离开账户页，否则后台要等满超时才能再次发起授权。
  const pendingLoginId =
    dialog?.kind === 'login' && dialog.phase === 'waiting'
      ? dialog.start.loginId
      : null;

  useEffect(() => {
    if (!pendingLoginId) return;
    return () => {
      void cancelLogin(pendingLoginId);
    };
  }, [pendingLoginId, cancelLogin]);

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

  const closeDialog = () => {
    // 作废尚未返回的启动请求；已在等待的登录由 pendingLoginId 的清理逻辑取消。
    startAttemptRef.current += 1;
    // 请求若在关闭后才失败，不会再走回 startAdd 的复位分支。
    setStarting(false);
    setDialog(null);
  };

  // Esc 关闭、焦点圈定与关闭后的焦点归还；内容切到结果页时重新落定焦点。
  const { dialogRef, captureTrigger } = useDialogA11y(
    dialog !== null,
    dialog ? `${dialog.kind}:${'phase' in dialog ? dialog.phase : ''}` : '',
    closeDialog,
  );

  const startAdd = async (platform: LoginPlatform) => {
    const attempt = startAttemptRef.current + 1;
    startAttemptRef.current = attempt;
    setToastMessage(null);
    setStarting(true);
    const name = draftName.trim() || undefined;
    const { start, error } = await loginCommands[platform](undefined, name);
    if (attempt !== startAttemptRef.current) {
      // 弹窗在请求返回前已关闭：授权流程不该在后台继续等待。
      if (start) void cancelLogin(start.loginId);
      return;
    }
    setStarting(false);
    if (!start) {
      // 授权没能启动时弹一次 toast，添加弹窗保持打开，方便改完再试或取消。
      setToastMessage(error ?? '授权未能启动，请稍后重试。');
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

  // 添加弹窗的主按钮：浏览器授权、API Key 与本机登录态三种提交方式不同，
  // 平台入口已经决定走哪一条，这里只按平台分派。
  const submitAddForm = async (
    dialog: Extract<DialogState, { kind: 'add-form' }>,
  ) => {
    const platform = dialog.platform;
    setToastMessage(null);

    if (isApiKeyPlatform(platform)) {
      const key = draftKey.trim();
      if (!key) {
        setToastMessage('请填写 API Key。');
        return;
      }
      setStarting(true);
      const error =
        dialog.mode === 'reauth' && dialog.accountId
          ? await replaceApiKey(dialog.accountId, key)
          : await addApiKeyAccount(platform, draftName.trim() || undefined, key);
      setStarting(false);
      if (error) setToastMessage(error);
      else setDialog(null);
      return;
    }

    if (platform === 'antigravity') {
      setStarting(true);
      const error = await addAntigravityAccount(draftName.trim() || undefined);
      setStarting(false);
      if (error) setToastMessage(error);
      else setDialog(null);
      return;
    }

    // 到这里只剩浏览器授权平台；重新授权走 beginReauth，不会停在添加弹窗。
    await startAdd(platform);
  };

  // 浏览器授权的重新授权；API Key 账户改走添加弹窗替换密钥。
  const startReauth = async (
    accountId: string,
    platform: LoginPlatform,
  ) => {
    const attempt = startAttemptRef.current + 1;
    startAttemptRef.current = attempt;
    setToastMessage(null);
    setStarting(true);
    const { start, error } = await loginCommands[platform](accountId);
    if (attempt !== startAttemptRef.current) {
      if (start) void cancelLogin(start.loginId);
      return;
    }
    setStarting(false);
    if (!start) {
      // 重新授权没有前置弹窗，失败原因用 toast 提示，不落到页面或空弹窗上。
      setToastMessage(error ?? '授权未能启动，请稍后重试。');
      return;
    }
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

  const beginReauth = (account: AccountState, trigger: HTMLElement | null) => {
    if (!isDialogPlatform(account.platform)) return;
    // 焦点还原记住入口按钮，而不是稍后可能被 autoFocus 抢走的输入框。
    captureTrigger(trigger);
    if (isApiKeyPlatform(account.platform)) {
      setDraftName('');
      setDraftKey('');
      setToastMessage(null);
      setDialog({
        kind: 'add-form',
        platform: account.platform,
        mode: 'reauth',
        accountId: account.accountId,
      });
      return;
    }
    // Antigravity 跟随本机 agy 登录态，没有应用内重新授权入口。
    if (account.platform !== 'antigravity') {
      void startReauth(account.accountId, account.platform);
    }
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

  const openRenameDialog = (
    account: AccountState,
    trigger: HTMLElement | null,
  ) => {
    setActing(false);
    captureTrigger(trigger);
    setDialog({
      kind: 'rename',
      accountId: account.accountId,
      value: account.displayName,
    });
  };

  const openRemoveDialog = (
    account: AccountState,
    trigger: HTMLElement | null,
  ) => {
    setActing(false);
    captureTrigger(trigger);
    setDialog({
      kind: 'remove',
      accountId: account.accountId,
      displayName: account.displayName,
    });
  };

  const groups = accountPlatformGroups(state.accounts);
  const unavailable = pendingPlatforms();

  const openAddDialog = (
    platform: DialogPlatform,
    trigger: HTMLElement | null,
  ) => {
    captureTrigger(trigger);
    setDraftName('');
    setDraftKey('');
    setToastMessage(null);
    setDialog({ kind: 'add-form', platform, mode: 'add' });
  };

  // 分组平台是普通字符串，入口处收窄一次；未登记平台的添加位不会渲染。
  const openAddForPlatform = (
    platform: string,
    trigger: HTMLElement | null,
  ) => {
    if (!isDialogPlatform(platform)) return;
    openAddDialog(platform, trigger);
  };

  // 平台由入口决定：从哪个平台分组点进来，就只给哪个平台添加账户，
  // 弹窗内不再提供切换，避免加到别的分组里。
  const choice =
    dialog && 'platform' in dialog
      ? PLATFORM_CHOICES[dialog.platform]
      : PLATFORM_CHOICES.codex;

  return (
    <section>
      <div className={styles.platformStack}>
        {groups.map((group) => (
          <section className={styles.platformGroup} key={group.platform}>
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
                // Antigravity 跟随本机 agy 登录态，没有应用内重新授权入口。
                const canReauth = managed && account.platform !== 'antigravity';
                // 本机 Codex 账户跟随 CLI 登录态，不能移除。
                const canRemove = !(account.platform === 'codex' && !managed);
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
                          {sourceLabel(account)}
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
                      {managed || canRemove ? (
                        <div className={styles.accountActions}>
                          {canReauth ? (
                            <button
                              type="button"
                              className={styles.miniButton}
                              onClick={(event) =>
                                beginReauth(account, event.currentTarget)
                              }
                              disabled={!desktop}
                              aria-label="重新授权"
                              title="重新授权"
                            >
                              <LinkSimple size={14} />
                            </button>
                          ) : null}
                          <button
                            type="button"
                            className={styles.miniButton}
                            onClick={(event) => openRenameDialog(account, event.currentTarget)}
                            disabled={!desktop}
                            aria-label="重命名"
                            title="重命名"
                          >
                            <PencilSimple size={14} />
                          </button>
                          {canRemove ? (
                            <button
                              type="button"
                              className={styles.miniButton}
                              onClick={(event) => openRemoveDialog(account, event.currentTarget)}
                              disabled={!desktop}
                              aria-label="移除账户"
                              title="移除账户"
                              data-danger
                            >
                              <Trash size={14} />
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                    </div>
                  </article>
                );
              })}

              {/* 添加位补在网格末尾：单账户时也能撑满一行，不必回到页头找入口 */}
              {group.meta.available && isDialogPlatform(group.platform) ? (
                <button
                  type="button"
                  className={styles.accountAddTile}
                  onClick={(event) => openAddForPlatform(group.platform, event.currentTarget)}
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
          <section className={styles.platformGroup} key={meta.key}>
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

      {toastMessage ? (
        <Toast
          message={toastMessage}
          onDismiss={() => setToastMessage(null)}
        />
      ) : null}

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
                ref={dialogRef}
                tabIndex={-1}
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
                      <h2 id="account-dialog-title">
                        {dialog.mode === 'reauth' ? '更新账户凭据' : choice.addLabel}
                      </h2>
                      <p className={styles.dialogSubtitle}>{choice.subtitle}</p>
                    </header>
                    {/* 重新授权只替换凭据，不再重复说明接入流程。 */}
                    {dialog.mode === 'add' ? (
                      <ol className={styles.dialogSteps} role="list">
                        {choice.steps.map((step, index) => (
                          <li className={styles.dialogStep} key={step.title}>
                            <span className={styles.stepMarker} aria-hidden="true">
                              {index + 1}
                            </span>
                            <div className={styles.stepBody}>
                              <h3 className={styles.stepTitle}>{step.title}</h3>
                              <p className={styles.stepText}>{step.text}</p>
                            </div>
                          </li>
                        ))}
                      </ol>
                    ) : null}
                    <div className={styles.dialogFieldGroup}>
                      {dialog.mode === 'add' ? (
                        <label className={styles.field}>
                          <span className={styles.fieldLabel}>账户名称（可选）</span>
                          <input
                            className={styles.textInput}
                            value={draftName}
                            onChange={(event) => setDraftName(event.target.value)}
                            placeholder={choice.namePlaceholder}
                            maxLength={60}
                          />
                        </label>
                      ) : null}
                      {isApiKeyPlatform(dialog.platform) ? (
                        <label className={styles.field}>
                          <span className={styles.fieldLabel}>
                            {dialog.platform === 'opencode-go'
                              ? 'OpenCode Go API Key'
                              : 'Command Code Provider API Key'}
                          </span>
                          <input
                            className={styles.textInput}
                            type="password"
                            value={draftKey}
                            onChange={(event) => setDraftKey(event.target.value)}
                            placeholder={
                              dialog.platform === 'opencode-go' ? 'sk-…' : 'user_…'
                            }
                            autoComplete="off"
                            spellCheck={false}
                            autoFocus={dialog.mode === 'reauth'}
                          />
                        </label>
                      ) : null}
                      <p className={styles.dialogNote}>
                        <LockSimple size={13} aria-hidden />
                        {choice.note}
                      </p>
                    </div>
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
                        className={styles.button}
                        onClick={() => void submitAddForm(dialog)}
                        disabled={starting}
                      >
                        {starting ? (
                          isApiKeyPlatform(dialog.platform)
                            ? '正在验证…'
                            : dialog.platform === 'antigravity'
                              ? '正在接入…'
                              : '正在启动…'
                        ) : (
                          <>
                            {dialog.mode === 'reauth'
                              ? '验证并更新'
                              : choice.authorizeLabel}
                            {!isApiKeyPlatform(dialog.platform)
                            && dialog.platform !== 'antigravity' ? (
                              <ArrowSquareOut size={13} aria-hidden />
                            ) : null}
                          </>
                        )}
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
                      <h2 id="account-dialog-title">
                        {dialog.phase === 'succeeded'
                          ? '授权完成'
                          : dialog.phase === 'failed'
                            ? '授权失败'
                            : dialog.phase === 'canceled'
                              ? '已取消授权'
                              : dialog.mode === 'reauth'
                                ? '重新授权'
                                : choice.waitingTitle}
                      </h2>
                      <p className={styles.dialogSubtitle}>{choice.subtitle}</p>
                    </header>

                    {dialog.phase === 'waiting' ? (
                      <>
                        <p className={styles.usageNote}>{choice.waitingText}</p>
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
                            <ArrowSquareOut size={13} aria-hidden />
                          </button>
                        </div>
                      </>
                    ) : dialog.phase === 'succeeded' ? (
                      <>
                        <div className={styles.dialogSuccess}>
                          <CheckCircle
                            size={18}
                            weight="fill"
                            className={styles.dialogSuccessIcon}
                            aria-hidden
                          />
                          <div className={styles.dialogSuccessBody}>
                            <p className={styles.dialogSuccessTitle}>
                              账户已成功接入。
                            </p>
                            <p className={styles.dialogSuccessText}>
                              额度数据正在后台刷新，稍后将自动显示。
                            </p>
                          </div>
                        </div>
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