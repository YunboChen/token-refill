import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import {
  ArrowClockwiseIcon,
  InfoIcon,
  KeyIcon,
  LockSimple,
  PencilSimpleIcon,
  PlugsIcon,
  Plus,
  TrashIcon,
  WarningIcon,
  X,
} from '@phosphor-icons/react';
import type { RelayAccount } from '../lib/desktop';
import { clockLabel } from '../lib/format';
import { useRelay } from '../lib/relay';
import {
  RELAY_ORIGIN_LABELS,
  RELAY_PROVIDER_LABELS,
  relayCandidateSource,
  relayHost,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import { useDialogA11y } from '../lib/useDialogA11y';
import ChevronIcon from '../components/ChevronIcon';
import Toast from '../components/Toast';
import styles from '../App.module.css';

/** 页面级瞬时提示：导入成功用 success，部分失败或操作提醒用 danger。 */
type RelayNotice = { message: string; tone: 'success' | 'danger' };

type RelayDialog =
  | { kind: 'add' }
  | { kind: 'scan' }
  | {
      kind: 'edit';
      accountId: string;
      host: string;
      baseUrl: string;
      label: string | null;
      referenceSaved: boolean;
    }
  | { kind: 'token'; accountId: string; host: string; userId: string | null }
  | { kind: 'remove'; accountId: string; host: string };

/** 管理页的账户卡：身份、状态、操作三段，与账户页的网格卡同一结构。 */
function RelayAccountTile({
  account,
  desktop,
  refreshing,
  onRefresh,
  onBeginEdit,
  onBeginTokenEdit,
  onAskRemove,
}: {
  account: RelayAccount;
  desktop: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onBeginEdit: (trigger: HTMLElement) => void;
  onBeginTokenEdit: (trigger: HTMLElement) => void;
  onAskRemove: (trigger: HTMLElement) => void;
}) {
  const host = relayHost(account.baseUrl);
  // 设置过标题时以标题作为账户名，未设置时沿用服务地址。
  const name = account.label ?? host;
  const quota = account.quota;
  // 第二行只留名称与来源；设置了标题时补上服务地址，仍能一眼认出站点。
  // 凭证保存状态收成一个图标，全文放进悬浮提示，
  // 避免「来源已保存 · 启动与刷新时重新读取」这类说明把整行撑到截断。
  const meta = [RELAY_ORIGIN_LABELS[account.origin], account.label ? host : null]
    .filter(
      (part, index, list): part is string =>
        Boolean(part) && list.indexOf(part) === index,
    )
    .join(' · ');
  // 已保存的凭证重启后仍在；来源引用每次启动重新读取；两者都不是才算「仅本次运行」。
  // 手动添加的凭证默认就保存在本机，不再挂图标说明。
  const state: {
    title: string;
    detail: string;
    icon: ReactNode;
    attention?: boolean;
  } | null = account.persisted
    ? null
    : account.referenceSaved
      ? {
          title: '来源引用已保存',
          detail: `启动与刷新时按「${RELAY_ORIGIN_LABELS[account.origin]}」重新读取密钥`,
          icon: <InfoIcon size={13} aria-hidden />,
        }
      : {
          title: '凭证仅本次运行',
          detail: '重启后需要重新添加账户',
          icon: <WarningIcon size={13} aria-hidden />,
          attention: true,
        };

  // 管理页只说明额度是否已同步，金额与窗口留给总览，避免两页重复展示。
  const updated =
    account.checkedAtEpoch !== null
      ? `更新于 ${clockLabel(account.checkedAtEpoch)}`
      : null;
  const status = quota
    ? quota.valid
      ? ['额度已同步', updated, refreshing ? '刷新中' : null]
          .filter(Boolean)
          .join(' · ')
      : '密钥不可用'
    : refreshing
      ? '正在识别并查询额度…'
      : '尚未获取额度';

  return (
    <article className={styles.accountTile} aria-live="polite">
      <div className={styles.accountIdentityCell}>
        <span className={styles.accountMark} aria-hidden="true">
          <PlugsIcon size={17} weight="duotone" />
        </span>
        <div className={styles.accountIdentity}>
          <div className={styles.accountNameRow}>
            <h3 className={styles.accountName} title={account.baseUrl}>
              {name}
            </h3>
            <span className={styles.planTag}>
              {RELAY_PROVIDER_LABELS[account.provider]}
            </span>
          </div>
          <div className={styles.accountMetaRow}>
            <p className={styles.accountMeta} title={meta}>
              {meta}
            </p>
            {state ? (
              <>
                <span
                  className={styles.accountState}
                  data-attention={state.attention || undefined}
                  tabIndex={0}
                  aria-describedby={`relay-state-${account.id}`}
                >
                  <span className={styles.accountStateIcon} aria-hidden="true">
                    {state.icon}
                  </span>
                </span>
                <span
                  className={styles.accountHintPop}
                  id={`relay-state-${account.id}`}
                  role="tooltip"
                >
                  <span className={styles.accountHintTitle}>{state.title}</span>
                  <span className={styles.accountHintDetail}>{state.detail}</span>
                  {account.hasAccountToken ? (
                    <span className={styles.accountHintDetail}>
                      访问令牌已配置，可读取账户余额
                    </span>
                  ) : null}
                </span>
              </>
            ) : null}
          </div>
        </div>
      </div>

      <div className={styles.relayTileStatus}>
        {quota && !quota.valid ? (
          <span className={styles.authStatus} data-state="needs-reauth">
            <span className={styles.authDot} aria-hidden="true" />
            {status}
          </span>
        ) : (
          <p className={styles.relayTileMeta}>{status}</p>
        )}
        {account.lastError ? (
          <p
            className={styles.relayTileNotice}
            data-danger={!quota || undefined}
            title={account.lastError}
          >
            刷新失败：{account.lastError}
          </p>
        ) : null}
      </div>

      <div className={styles.accountFoot}>
        <button
          type="button"
          className={styles.accountRefresh}
          onClick={onRefresh}
          disabled={!desktop || refreshing}
          aria-label={`刷新 ${name} 的额度`}
          title={desktop ? '重新识别并刷新额度' : '浏览器预览无法调用 Rust'}
        >
          <ArrowClockwiseIcon
            size={13}
            className={refreshing ? styles.spin : undefined}
            aria-hidden
          />
          刷新
        </button>
        <div className={styles.accountActions}>
          <button
            type="button"
            className={styles.miniButton}
            onClick={(event) => onBeginEdit(event.currentTarget)}
            disabled={!desktop}
            aria-label={`编辑 ${name}`}
            title="编辑账户"
          >
            <PencilSimpleIcon size={14} aria-hidden />
          </button>
          {account.provider === 'new-api' ? (
            <button
              type="button"
              className={styles.miniButton}
              onClick={(event) => onBeginTokenEdit(event.currentTarget)}
              disabled={!desktop}
              data-configured={account.hasAccountToken || undefined}
              aria-label={
                account.hasAccountToken
                  ? `${host} 已配置访问令牌，点击更新`
                  : `为 ${host} 设置访问令牌`
              }
              title={
                account.hasAccountToken
                  ? '访问令牌已配置，点击更新'
                  : '设置访问令牌，读取账户余额'
              }
            >
              <KeyIcon
                size={14}
                weight={account.hasAccountToken ? 'fill' : 'regular'}
                aria-hidden
              />
            </button>
          ) : null}
          <button
            type="button"
            className={styles.miniButton}
            onClick={(event) => onAskRemove(event.currentTarget)}
            disabled={!desktop}
            aria-label={`移除 ${host}`}
            title="移除账户"
            data-danger
          >
            <TrashIcon size={14} aria-hidden />
          </button>
        </div>
      </div>
    </article>
  );
}

export default function RelayPanel() {
  const { desktop, state } = useAppState();
  const refreshMinutes = state.preferences.refreshMinutes;
  const {
    accounts,
    candidates,
    skipped,
    scanning,
    importing,
    loaded,
    error,
    refreshingIds,
    ensureLoaded,
    scanCandidates,
    importCandidates,
    refreshDue,
    addAccount,
    refresh,
    remove,
    setLabel,
    update,
    saveAccountToken,
    clearError,
  } = useRelay();
  const [dialog, setDialog] = useState<RelayDialog | null>(null);
  const [baseUrl, setBaseUrl] = useState('');
  // 标题不是凭证，可以放在组件状态里。
  const [label, setLabelValue] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [formNotice, setFormNotice] = useState<RelayNotice | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [savingEdit, setSavingEdit] = useState(false);
  const [savingToken, setSavingToken] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [skippedOpen, setSkippedOpen] = useState(false);
  // 扫描候选的勾选状态；只保存 id，密钥始终留在 Rust 侧。
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const reduceMotion = useReducedMotion();

  // 已在列表中的候选不可重复导入，全选只作用于其余候选。
  const selectableCandidates = candidates.filter(
    (candidate) => !candidate.alreadyAdded,
  );
  const selectableSelected = selectableCandidates.filter((candidate) =>
    selected.has(candidate.id),
  ).length;
  const allSelected =
    selectableCandidates.length > 0 &&
    selectableSelected === selectableCandidates.length;
  const partiallySelected = selectableSelected > 0 && !allSelected;

  // 敏感输入一律用 ref 读取，不进入组件状态、日志或持久化数据。
  const keyInput = useRef<HTMLInputElement>(null);
  const accountTokenInput = useRef<HTMLInputElement>(null);
  const userIdInput = useRef<HTMLInputElement>(null);
  const tokenInput = useRef<HTMLInputElement>(null);
  const tokenUserIdInput = useRef<HTMLInputElement>(null);
  const selectAllInput = useRef<HTMLInputElement>(null);

  // 部分勾选时用原生半选状态提示「再点一次才是全选」。
  useEffect(() => {
    if (selectAllInput.current) {
      selectAllInput.current.indeterminate = partiallySelected;
    }
  }, [partiallySelected, dialog]);

  useEffect(() => {
    if (desktop) ensureLoaded();
  }, [desktop, ensureLoaded]);

  // 进入页面后把超过刷新间隔的账户自动刷新一次；不打开页面就不发请求。
  useEffect(() => {
    if (!desktop || !loaded) return;
    void refreshDue(refreshMinutes);
  }, [desktop, loaded, refreshMinutes, refreshDue]);

  const closeDialog = () => {
    setDialog(null);
    setFormError(null);
    clearError();
  };

  const { dialogRef, captureTrigger } = useDialogA11y(
    dialog !== null,
    dialog
      ? `${dialog.kind}:${'accountId' in dialog ? dialog.accountId : ''}`
      : '',
    closeDialog,
  );

  const prepareDialog = (trigger?: HTMLElement | null) => {
    setFormError(null);
    setFormNotice(null);
    clearError();
    captureTrigger(trigger);
  };

  const openAddDialog = (trigger?: HTMLElement | null) => {
    prepareDialog(trigger);
    setBaseUrl('');
    setLabelValue('');
    if (keyInput.current) keyInput.current.value = '';
    if (accountTokenInput.current) accountTokenInput.current.value = '';
    if (userIdInput.current) userIdInput.current.value = '';
    setDialog({ kind: 'add' });
  };

  const runScan = async () => {
    setSelected(new Set());
    const found = await scanCandidates();
    if (!found) return;
    // 默认勾选尚未在列表中的候选；已在列表里的保持禁用，避免重复查询。
    setSelected(
      new Set(found.filter((item) => !item.alreadyAdded).map((item) => item.id)),
    );
  };

  const openScanDialog = (trigger?: HTMLElement | null) => {
    prepareDialog(trigger);
    setDialog({ kind: 'scan' });
    void runScan();
  };

  const toggleCandidate = (id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // 全选只在「全不选」与「全选可导入项」之间切换，已导入的候选不参与。
  const toggleAllCandidates = () => {
    setSelected(
      allSelected
        ? new Set()
        : new Set(selectableCandidates.map((candidate) => candidate.id)),
    );
  };

  const submitImport = async () => {
    if (importing) return;
    const ids = [...selected];
    if (ids.length === 0) {
      setFormError('请至少选择一个配置。');
      return;
    }
    setFormError(null);
    const report = await importCandidates(ids);
    if (!report) return;
    const failed = report.skipped.length;
    setDialog(null);
    setFormNotice({
      tone: failed === 0 ? 'success' : 'danger',
      message:
        failed === 0
          ? `已导入 ${ids.length} 个配置。`
          : `已导入 ${ids.length - failed} 个配置，${failed} 个未能识别，见下方列表。`,
    });
  };

  const openEditDialog = (account: RelayAccount, trigger: HTMLElement) => {
    prepareDialog(trigger);
    setBaseUrl(account.baseUrl);
    setLabelValue(account.label ?? '');
    if (keyInput.current) keyInput.current.value = '';
    setDialog({
      kind: 'edit',
      accountId: account.id,
      host: relayHost(account.baseUrl),
      baseUrl: account.baseUrl,
      label: account.label,
      referenceSaved: account.referenceSaved,
    });
  };

  const openTokenDialog = (account: RelayAccount, trigger: HTMLElement) => {
    prepareDialog(trigger);
    setDialog({
      kind: 'token',
      accountId: account.id,
      host: relayHost(account.baseUrl),
      userId: account.newApiUserId,
    });
  };

  const openRemoveDialog = (account: RelayAccount, trigger: HTMLElement) => {
    prepareDialog(trigger);
    setDialog({
      kind: 'remove',
      accountId: account.id,
      host: relayHost(account.baseUrl),
    });
  };

  const submitAdd = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const apiKey = keyInput.current?.value.trim() ?? '';
    if (!baseUrl.trim()) {
      setFormError('请填写服务地址。');
      return;
    }
    if (!apiKey) {
      setFormError('请填写 API Key。');
      return;
    }

    setSubmitting(true);
    setFormError(null);
    try {
      const accountToken = accountTokenInput.current?.value.trim() ?? '';
      const userId = userIdInput.current?.value.trim() ?? '';
      const account = await addAccount(
        baseUrl.trim(),
        apiKey,
        accountToken || undefined,
        userId || undefined,
        label.trim() || undefined,
      );
      if (!account) return;
      if (accountToken && account.provider !== 'new-api') {
        setFormNotice({
          tone: 'danger',
          message:
            '访问令牌只对 New API 生效；当前站点识别为 Sub2API，访问令牌未保存。',
        });
      }
      if (keyInput.current) keyInput.current.value = '';
      if (accountTokenInput.current) accountTokenInput.current.value = '';
      if (userIdInput.current) userIdInput.current.value = '';
      setBaseUrl('');
      setLabelValue('');
      setDialog(null);
    } finally {
      setSubmitting(false);
    }
  };

  const submitEdit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (dialog?.kind !== 'edit' || savingEdit) return;
    const titleChanged = label.trim() !== (dialog.label ?? '');
    setSavingEdit(true);
    setFormError(null);
    // 来源账户只允许改标题：地址与密钥始终应来自本地配置。
    if (dialog.referenceSaved) {
      const ok = !titleChanged || (await setLabel(dialog.accountId, label.trim()));
      setSavingEdit(false);
      if (ok) setDialog(null);
      return;
    }
    const url = baseUrl.trim();
    if (!url) {
      setSavingEdit(false);
      setFormError('请填写服务地址。');
      return;
    }
    // 密钥输入用 ref 读取，不进入组件状态、日志或持久化数据。
    const key = keyInput.current?.value.trim() ?? '';
    const credentialsChanged = url !== dialog.baseUrl || key !== '';
    if (!credentialsChanged) {
      // 只改标题时不必联网重新识别。
      const ok = !titleChanged || (await setLabel(dialog.accountId, label.trim()));
      setSavingEdit(false);
      if (ok) setDialog(null);
      return;
    }
    const ok = await update(dialog.accountId, url, key || undefined);
    if (ok && titleChanged) await setLabel(dialog.accountId, label.trim());
    setSavingEdit(false);
    if (ok) setDialog(null);
  };

  const submitToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (dialog?.kind !== 'token' || savingToken) return;
    const token = tokenInput.current?.value.trim() ?? '';
    if (!token) {
      setFormError('请填写访问令牌。');
      return;
    }

    setSavingToken(true);
    setFormError(null);
    const userId = tokenUserIdInput.current?.value.trim() ?? '';
    const ok = await saveAccountToken(
      dialog.accountId,
      token,
      userId || undefined,
    );
    setSavingToken(false);
    if (ok) setDialog(null);
  };

  const confirmRemove = async () => {
    if (dialog?.kind !== 'remove' || removing) return;
    setRemoving(true);
    setFormError(null);
    const ok = await remove(dialog.accountId);
    setRemoving(false);
    if (ok) setDialog(null);
  };

  const dialogMessage = formError ?? error;
  const busy = scanning || importing || submitting;

  return (
    <section>
      {error && !dialog ? (
        <p
          className={`${styles.noticeDanger} ${styles.accountNotice}`}
          role="alert"
        >
          {error}
        </p>
      ) : null}

      <section className={styles.platformGroup}>
        {/* 与账户页、总览的中转站额度共用同一套平台标题：小尺寸图形 + 名称/说明 + 右侧动作。 */}
        <div className={styles.platformSectionHead}>
          <div className={styles.platformHeading}>
            <span
              className={styles.platformBadge}
              data-active
              aria-hidden="true"
            >
              <PlugsIcon size={20} weight="duotone" />
            </span>
            <div className={styles.platformIdentity}>
              <div className={styles.platformNameRow}>
                <h2 className={styles.platformName}>中转站账户</h2>
              </div>
              <p className={styles.platformDescription}>
                New API / Sub2API · 账户与凭证管理
              </p>
            </div>
            <div className={styles.platformAside}>
              {accounts.length > 0 ? (
                <span className={styles.platformCount}>
                  {accounts.length} 个账户
                </span>
              ) : null}
              <button
                type="button"
                className={`${styles.testButton} ${styles.relayScanButton}`}
                onClick={(event) => {
                  setFormNotice(null);
                  openScanDialog(event.currentTarget);
                }}
                disabled={!desktop || busy}
                title={
                  desktop
                    ? '扫描 cc-switch 与 Claude Code / Codex 的本地配置'
                    : '浏览器预览无法调用 Rust'
                }
              >
                <ArrowClockwiseIcon
                  size={13}
                  className={scanning ? styles.spin : undefined}
                  aria-hidden
                />
                {scanning ? '扫描中…' : '扫描本地配置'}
              </button>
            </div>
          </div>
        </div>

        <div className={styles.accountGrid}>
          {accounts.map((account) => (
            <RelayAccountTile
              key={account.id}
              account={account}
              desktop={desktop}
              refreshing={refreshingIds.has(account.id)}
              onRefresh={() => void refresh(account.id)}
              onBeginEdit={(trigger) => openEditDialog(account, trigger)}
              onBeginTokenEdit={(trigger) => openTokenDialog(account, trigger)}
              onAskRemove={(trigger) => openRemoveDialog(account, trigger)}
            />
          ))}

          <button
            type="button"
            className={styles.accountAddTile}
            onClick={(event) => openAddDialog(event.currentTarget)}
            disabled={!desktop || busy}
            title={
              !desktop
                ? '浏览器预览无法调用 Rust'
                : busy
                  ? '正在处理本地配置，请稍候'
                  : '添加中转站账户'
            }
          >
            <span className={styles.accountAddIcon} aria-hidden="true">
              <Plus size={15} weight="bold" />
            </span>
            <span className={styles.accountAddLabel}>添加中转站账户</span>
          </button>
        </div>

        {accounts.length === 0 ? (
          <p className={styles.accountNote}>
            {desktop && !loaded
              ? '正在加载已保存的中转站账户…'
              : '还没有中转站账户。可以点击虚线卡片手动添加，或扫描 cc-switch / 本机配置。'}
          </p>
        ) : null}

        {formNotice ? (
          <Toast
            tone={formNotice.tone}
            message={formNotice.message}
            onDismiss={() => setFormNotice(null)}
          />
        ) : null}

        {skipped.length > 0 ? (
          <div className={styles.relaySkipped}>
            <button
              type="button"
              className={styles.relaySkippedToggle}
              aria-expanded={skippedOpen}
              aria-controls="relay-skipped-list"
              onClick={() => setSkippedOpen((open) => !open)}
            >
              <ChevronIcon direction={skippedOpen ? 'down' : 'right'} />
              {skippedOpen
                ? '收起未识别的本地配置'
                : `未识别的本地配置 ${skipped.length} 条`}
            </button>
            {skippedOpen ? (
              <ul id="relay-skipped-list">
                {skipped.map((item, index) => (
                  <li key={`${item.origin}-${item.baseUrl}-${index}`}>
                    <span>
                      {RELAY_ORIGIN_LABELS[item.origin]} · {item.baseUrl}
                    </span>
                    <span className={styles.quotaMeta}>{item.reason}</span>
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}

        {!desktop ? (
          <p className={styles.accountNote}>
            浏览器预览无法调用 Rust，请在桌面应用内使用（
            <code className={styles.codeChip}>pnpm tauri dev</code>）。
          </p>
        ) : null}
      </section>

      {/* portal 挂到 body：面板容器带进场 transform，会成为 fixed 后代的包含块。 */}
      {createPortal(
        <AnimatePresence>
          {dialog ? (
            <m.div
              key="relay-dialog"
              className={styles.dialogBackdrop}
              role="presentation"
              initial={reduceMotion ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={
                reduceMotion
                  ? { opacity: 1 }
                  : {
                      opacity: 0,
                      transition: { duration: 0.34, ease: 'easeIn' },
                    }
              }
              transition={
                reduceMotion ? { duration: 0 } : { duration: 0.16, ease: 'easeOut' }
              }
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
                aria-labelledby="relay-dialog-title"
                initial={
                  reduceMotion ? false : { opacity: 0, scale: 0.96, y: 10 }
                }
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={
                  reduceMotion
                    ? { opacity: 1 }
                    : {
                        opacity: 0,
                        scale: 0.96,
                        y: 10,
                        transition: { duration: 0.3, ease: 'easeIn' },
                      }
                }
                transition={
                  reduceMotion
                    ? { duration: 0 }
                    : { type: 'spring', stiffness: 460, damping: 34 }
                }
              >
                <button
                  type="button"
                  className={styles.dialogClose}
                  onClick={closeDialog}
                  aria-label="关闭"
                >
                  <X size={16} />
                </button>

                {dialog.kind === 'add' ? (
                  <form className={styles.dialogForm} onSubmit={submitAdd}>
                    <header className={styles.dialogHeader}>
                      <h2 id="relay-dialog-title">添加中转站账户</h2>
                      <p className={styles.dialogSubtitle}>
                        自动识别 New API 与 Sub2API
                      </p>
                    </header>
                    <div className={styles.dialogFieldGroup}>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>标题（可选）</span>
                        <input
                          className={styles.textInput}
                          type="text"
                          value={label}
                          onChange={(event) => setLabelValue(event.target.value)}
                          placeholder="例如：主力中转 / 团队账户"
                          maxLength={40}
                          disabled={submitting}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>服务地址</span>
                        <input
                          className={styles.textInput}
                          type="url"
                          inputMode="url"
                          placeholder="https://api.example.com"
                          value={baseUrl}
                          onChange={(event) => setBaseUrl(event.target.value)}
                          disabled={submitting}
                          autoComplete="off"
                          spellCheck={false}
                          autoFocus
                        />
                      </label>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>API Key</span>
                        <input
                          ref={keyInput}
                          className={styles.textInput}
                          type="password"
                          placeholder="sk-..."
                          disabled={submitting}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>
                          访问令牌（可选，New API）
                        </span>
                        <input
                          ref={accountTokenInput}
                          className={styles.textInput}
                          type="password"
                          placeholder="用于读取账户余额与订阅"
                          disabled={submitting}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>
                          用户 ID（旧版令牌）
                        </span>
                        <input
                          ref={userIdInput}
                          className={styles.textInput}
                          type="text"
                          inputMode="numeric"
                          placeholder="New-Api-User"
                          disabled={submitting}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <p className={styles.dialogNote}>
                        <LockSimple size={13} aria-hidden />
                        密钥与访问令牌只保存在本机 relay-profiles/，不会进入界面状态或日志。
                      </p>
                      <p className={styles.dialogNote}>
                        New API 需要密钥开启「允许查询用户额度」，或填写访问令牌。
                      </p>
                    </div>
                    {dialogMessage ? (
                      <p className={styles.noticeDanger} role="alert">
                        {dialogMessage}
                      </p>
                    ) : null}
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                        disabled={submitting}
                      >
                        取消
                      </button>
                      <button
                        type="submit"
                        className={styles.button}
                        disabled={submitting}
                      >
                        {submitting ? '识别中…' : '识别并添加'}
                      </button>
                    </div>
                  </form>
                ) : dialog.kind === 'edit' ? (
                  <form className={styles.dialogForm} onSubmit={submitEdit}>
                    <header className={styles.dialogHeader}>
                      <h2 id="relay-dialog-title">编辑中转站账户</h2>
                      <p className={styles.dialogSubtitle}>{dialog.host}</p>
                    </header>
                    <div className={styles.dialogFieldGroup}>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>标题（可选）</span>
                        <input
                          className={styles.textInput}
                          type="text"
                          value={label}
                          onChange={(event) => setLabelValue(event.target.value)}
                          placeholder="例如：主力中转 / 团队账户"
                          maxLength={40}
                          disabled={savingEdit}
                          autoComplete="off"
                          spellCheck={false}
                          autoFocus
                        />
                      </label>
                      <div className={styles.field}>
                        <span className={styles.fieldLabel}>
                          服务地址
                          {dialog.referenceSaved ? (
                            <span className={styles.fieldTag}>只读</span>
                          ) : null}
                        </span>
                        {dialog.referenceSaved ? (
                          <span
                            className={styles.readonlyValue}
                            title={baseUrl}
                          >
                            <LockSimple size={13} aria-hidden />
                            {baseUrl}
                          </span>
                        ) : (
                          <input
                            className={styles.textInput}
                            type="url"
                            inputMode="url"
                            placeholder="https://api.example.com"
                            value={baseUrl}
                            onChange={(event) => setBaseUrl(event.target.value)}
                            disabled={savingEdit}
                            autoComplete="off"
                            spellCheck={false}
                          />
                        )}
                      </div>
                      {!dialog.referenceSaved ? (
                        <label className={styles.field}>
                          <span className={styles.fieldLabel}>API Key</span>
                          <input
                            ref={keyInput}
                            className={styles.textInput}
                            type="password"
                            placeholder="留空则保持当前密钥"
                            disabled={savingEdit}
                            autoComplete="off"
                            spellCheck={false}
                          />
                        </label>
                      ) : null}
                      <p className={styles.dialogNote}>
                        <LockSimple size={13} aria-hidden />
                        {dialog.referenceSaved
                          ? '该账户由本地配置导入：服务地址与密钥在每次刷新前从源配置读取，这里只能修改标题。如需更换，请修改源配置后点击刷新。'
                          : 'API Key 留空则保持当前密钥；保存后立即重新识别并刷新额度。'}
                      </p>
                    </div>
                    {dialogMessage ? (
                      <p className={styles.noticeDanger} role="alert">
                        {dialogMessage}
                      </p>
                    ) : null}
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                        disabled={savingEdit}
                      >
                        取消
                      </button>
                      <button
                        type="submit"
                        className={styles.button}
                        disabled={savingEdit}
                      >
                        {savingEdit
                          ? '保存中…'
                          : dialog.referenceSaved
                            ? '保存标题'
                            : '保存并刷新'}
                      </button>
                    </div>
                  </form>
                ) : dialog.kind === 'token' ? (
                  <form className={styles.dialogForm} onSubmit={submitToken}>
                    <header className={styles.dialogHeader}>
                      <h2 id="relay-dialog-title">设置访问令牌</h2>
                      <p className={styles.dialogSubtitle}>
                        {dialog.host} · 读取账户余额与订阅
                      </p>
                    </header>
                    <div className={styles.dialogFieldGroup}>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>
                          New API 访问令牌
                        </span>
                        <input
                          ref={tokenInput}
                          className={styles.textInput}
                          type="password"
                          placeholder="个人访问令牌"
                          disabled={savingToken}
                          autoComplete="off"
                          spellCheck={false}
                          autoFocus
                        />
                      </label>
                      <label className={styles.field}>
                        <span className={styles.fieldLabel}>
                          用户 ID（旧版令牌需要）
                        </span>
                        <input
                          ref={tokenUserIdInput}
                          className={styles.textInput}
                          type="text"
                          inputMode="numeric"
                          placeholder="New-Api-User"
                          defaultValue={dialog.userId ?? ''}
                          disabled={savingToken}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </label>
                      <p className={styles.dialogNote}>
                        <LockSimple size={13} aria-hidden />
                        令牌只保存在本机 relay-profiles/，不会进入界面状态或日志。
                      </p>
                    </div>
                    {dialogMessage ? (
                      <p className={styles.noticeDanger} role="alert">
                        {dialogMessage}
                      </p>
                    ) : null}
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                        disabled={savingToken}
                      >
                        取消
                      </button>
                      <button
                        type="submit"
                        className={styles.button}
                        disabled={savingToken}
                      >
                        {savingToken ? '保存中…' : '保存并刷新'}
                      </button>
                    </div>
                  </form>
                ) : dialog.kind === 'scan' ? (
                  <>
                    <header className={styles.dialogHeader}>
                      <h2 id="relay-dialog-title">扫描本地配置</h2>
                      <p className={styles.dialogSubtitle}>
                        cc-switch 保存的 provider 与 Claude Code / Codex 的当前配置
                      </p>
                    </header>
                    {scanning ? (
                      <p className={styles.dialogNote} role="status">
                        <ArrowClockwiseIcon
                          size={13}
                          className={styles.spin}
                          aria-hidden
                        />
                        正在读取本地配置…
                      </p>
                    ) : candidates.length === 0 ? (
                      <p className={styles.dialogNote}>
                        没有找到可导入的中转站配置。
                      </p>
                    ) : (
                      <div className={styles.relayCandidateGroup}>
                        {selectableCandidates.length > 0 ? (
                          <div className={styles.relayCandidateToolbar}>
                            <label className={styles.relayCandidateSelectAll}>
                              <input
                                ref={selectAllInput}
                                type="checkbox"
                                checked={allSelected}
                                onChange={toggleAllCandidates}
                                disabled={importing}
                              />
                              <span>{allSelected ? '取消全选' : '全选'}</span>
                            </label>
                            <span className={styles.relayCandidateCount}>
                              已选 {selectableSelected} /{' '}
                              {selectableCandidates.length}
                            </span>
                          </div>
                        ) : null}
                        <div
                          className={styles.relayCandidateList}
                          role="group"
                          aria-label="扫描到的本地配置"
                        >
                          {candidates.map((candidate) => {
                            const source = relayCandidateSource(
                              candidate.origin,
                              candidate.app,
                            );
                            const disabled =
                              candidate.alreadyAdded || importing;
                            return (
                              <label
                                key={candidate.id}
                                className={styles.relayCandidate}
                                data-disabled={disabled || undefined}
                              >
                                <input
                                  type="checkbox"
                                  checked={selected.has(candidate.id)}
                                  onChange={() => toggleCandidate(candidate.id)}
                                  disabled={disabled}
                                />
                                <span className={styles.relayCandidateBody}>
                                  <span className={styles.relayCandidateName}>
                                    {candidate.label ?? source}
                                  </span>
                                  <span className={styles.relayCandidateMeta}>
                                    {[
                                      candidate.label ? source : null,
                                      relayHost(candidate.baseUrl),
                                      candidate.alreadyAdded ? '已在列表中' : null,
                                    ]
                                      .filter(Boolean)
                                      .join(' · ')}
                                  </span>
                                </span>
                              </label>
                            );
                          })}
                        </div>
                      </div>
                    )}
                    {dialogMessage ? (
                      <p className={styles.noticeDanger} role="alert">
                        {dialogMessage}
                      </p>
                    ) : null}
                    <p className={styles.dialogNote}>
                      <LockSimple size={13} aria-hidden />
                      扫描只读取 provider 地址与密钥，不发起网络请求；导入后才查询额度。
                      密钥不会写入 relay-profiles/，界面只保存来源引用，启动与刷新时重新读取源配置。
                    </p>
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                        disabled={importing}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={() => void runScan()}
                        disabled={scanning || importing}
                      >
                        重新扫描
                      </button>
                      <button
                        type="button"
                        className={styles.button}
                        onClick={() => void submitImport()}
                        disabled={scanning || importing || selected.size === 0}
                      >
                        {importing
                          ? '导入中…'
                          : `导入所选（${selected.size}）`}
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <h2 id="relay-dialog-title">移除中转站账户</h2>
                    <p className={styles.usageNote}>
                      将移除「{dialog.host}」，并删除本应用保存的密钥与访问令牌（如有）。
                      Claude Code / Codex 的本地配置与其他账户不受影响。
                    </p>
                    {dialogMessage ? (
                      <p className={styles.noticeDanger} role="alert">
                        {dialogMessage}
                      </p>
                    ) : null}
                    <div className={styles.dialogActions}>
                      <button
                        type="button"
                        className={styles.buttonGhost}
                        onClick={closeDialog}
                        disabled={removing}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className={styles.dangerButton}
                        onClick={() => void confirmRemove()}
                        disabled={removing}
                      >
                        {removing ? '正在移除…' : '确认移除'}
                      </button>
                    </div>
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