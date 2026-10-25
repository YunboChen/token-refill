import { useEffect, useRef, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, m, useReducedMotion } from 'motion/react';
import {
  ArrowClockwiseIcon,
  KeyIcon,
  LockSimple,
  PlugsIcon,
  Plus,
  TrashIcon,
  X,
} from '@phosphor-icons/react';
import type { RelayAccount } from '../lib/desktop';
import { clockLabel } from '../lib/format';
import { useRelay } from '../lib/relay';
import {
  RELAY_ORIGIN_LABELS,
  RELAY_PROVIDER_LABELS,
  relayHost,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import { useDialogA11y } from '../lib/useDialogA11y';
import ChevronIcon from '../components/ChevronIcon';
import styles from '../App.module.css';

type RelayDialog =
  | { kind: 'add' }
  | { kind: 'token'; accountId: string; host: string; userId: string | null }
  | { kind: 'remove'; accountId: string; host: string };

/** 管理页的账户卡：身份、状态、操作三段，与账户页的网格卡同一结构。 */
function RelayAccountTile({
  account,
  desktop,
  refreshing,
  onRefresh,
  onBeginTokenEdit,
  onAskRemove,
}: {
  account: RelayAccount;
  desktop: boolean;
  refreshing: boolean;
  onRefresh: () => void;
  onBeginTokenEdit: (trigger: HTMLElement) => void;
  onAskRemove: (trigger: HTMLElement) => void;
}) {
  const host = relayHost(account.baseUrl);
  const quota = account.quota;
  const meta = [
    RELAY_ORIGIN_LABELS[account.origin],
    account.persisted ? '凭证已保存' : '凭证仅本次运行',
    account.hasAccountToken ? '访问令牌已配置' : null,
  ]
    .filter(Boolean)
    .join(' · ');

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
              {host}
            </h3>
            <span className={styles.planTag}>
              {RELAY_PROVIDER_LABELS[account.provider]}
            </span>
          </div>
          <p className={styles.accountMeta} title={meta}>
            {meta}
          </p>
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
          aria-label={`刷新 ${host} 的额度`}
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
          {account.provider === 'new-api' ? (
            <button
              type="button"
              className={styles.miniButton}
              onClick={(event) => onBeginTokenEdit(event.currentTarget)}
              disabled={!desktop}
              aria-label={`为 ${host} 设置访问令牌`}
              title="设置或更新访问令牌"
            >
              <KeyIcon size={14} aria-hidden />
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
    skipped,
    scanning,
    loaded,
    error,
    refreshingIds,
    ensureLoaded,
    rescan,
    refreshDue,
    addAccount,
    refresh,
    remove,
    saveAccountToken,
    clearError,
  } = useRelay();
  const [dialog, setDialog] = useState<RelayDialog | null>(null);
  const [baseUrl, setBaseUrl] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [formNotice, setFormNotice] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [savingToken, setSavingToken] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [skippedOpen, setSkippedOpen] = useState(false);
  const reduceMotion = useReducedMotion();

  // 敏感输入一律用 ref 读取，不进入组件状态、日志或持久化数据。
  const keyInput = useRef<HTMLInputElement>(null);
  const accountTokenInput = useRef<HTMLInputElement>(null);
  const userIdInput = useRef<HTMLInputElement>(null);
  const tokenInput = useRef<HTMLInputElement>(null);
  const tokenUserIdInput = useRef<HTMLInputElement>(null);

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
    if (keyInput.current) keyInput.current.value = '';
    if (accountTokenInput.current) accountTokenInput.current.value = '';
    if (userIdInput.current) userIdInput.current.value = '';
    setDialog({ kind: 'add' });
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
      );
      if (!account) return;
      if (accountToken && account.provider !== 'new-api') {
        setFormNotice(
          '访问令牌只对 New API 生效；当前站点识别为 Sub2API，访问令牌未保存。',
        );
      }
      if (keyInput.current) keyInput.current.value = '';
      if (accountTokenInput.current) accountTokenInput.current.value = '';
      if (userIdInput.current) userIdInput.current.value = '';
      setBaseUrl('');
      setDialog(null);
    } finally {
      setSubmitting(false);
    }
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
  const busy = scanning || submitting;

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
                onClick={() => {
                  setFormNotice(null);
                  void rescan();
                }}
                disabled={!desktop || busy}
                title={
                  desktop
                    ? '重新扫描 Claude Code 与 Codex 的本地配置'
                    : '浏览器预览无法调用 Rust'
                }
              >
                <ArrowClockwiseIcon
                  size={13}
                  className={scanning ? styles.spin : undefined}
                  aria-hidden
                />
                {scanning ? '扫描中…' : '重新扫描本地配置'}
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
                : scanning
                  ? '正在扫描本地配置，请稍候'
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
            {desktop && (!loaded || scanning)
              ? '正在扫描 Claude Code 与 Codex 的本地配置…'
              : '还没有中转站账户。可以点击虚线卡片手动添加，或重新扫描本机配置。'}
          </p>
        ) : null}

        {formNotice ? (
          <p className={styles.accountNote} role="status">
            {formNotice}
          </p>
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