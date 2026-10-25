import { useEffect, useRef, useState, type SyntheticEvent } from 'react';
import {
  ArrowClockwiseIcon,
  KeyIcon,
  PlugsIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import type { RelayAccount } from '../lib/desktop';
import { clockLabel } from '../lib/format';
import { useRelay } from '../lib/relay';
import {
  RELAY_MODE_LABELS,
  RELAY_ORIGIN_LABELS,
  RELAY_PROVIDER_LABELS,
  relayHost,
} from '../lib/relayFormat';
import { useAppState } from '../lib/store';
import styles from '../App.module.css';

function RelayManageCard({
  account,
  desktop,
  refreshing,
  confirming,
  tokenEditing,
  savingToken,
  onRefresh,
  onAskRemove,
  onCancelRemove,
  onRemove,
  onBeginTokenEdit,
  onCancelTokenEdit,
  onSubmitToken,
}: {
  account: RelayAccount;
  desktop: boolean;
  refreshing: boolean;
  confirming: boolean;
  tokenEditing: boolean;
  savingToken: boolean;
  onRefresh: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
  onRemove: () => void;
  onBeginTokenEdit: () => void;
  onCancelTokenEdit: () => void;
  onSubmitToken: (token: string, userId: string) => void;
}) {
  const quota = account.quota;
  const host = relayHost(account.baseUrl);
  const tokenInput = useRef<HTMLInputElement>(null);
  const userIdInput = useRef<HTMLInputElement>(null);

  return (
    <article className={styles.accountCard} aria-live="polite">
      <header className={styles.accountHead}>
        <div className={styles.accountTitleBlock}>
          <h3 className={styles.accountName} title={account.baseUrl}>
            {host}
          </h3>
          <p className={styles.accountMetaLine}>
            <span className={styles.planTag}>
              {RELAY_ORIGIN_LABELS[account.origin]}
            </span>
            {/* 平台名与来源、额度口径同属账户元信息，统一用中性标签，不再单独强调。 */}
            <span className={styles.planTag}>
              {RELAY_PROVIDER_LABELS[account.provider]}
            </span>
            {quota ? (
              <span className={styles.planTag}>
                {RELAY_MODE_LABELS[quota.mode]}
              </span>
            ) : null}
            {quota?.keyName ? (
              <span className={styles.accountHint}>{quota.keyName}</span>
            ) : null}
            {quota?.planName ? (
              <span className={styles.accountHint}>{quota.planName}</span>
            ) : null}
            {quota && !quota.valid ? (
              <span className={styles.authStatus} data-state="needs-reauth">
                <span className={styles.authDot} aria-hidden="true" />
                密钥不可用
              </span>
            ) : null}
            <span className={styles.accountHint}>
              {account.persisted ? '凭证已保存' : '凭证仅本次运行'}
            </span>
            {account.hasAccountToken ? (
              <span className={styles.accountHint}>已配置访问令牌</span>
            ) : null}
          </p>
        </div>
        <div className={styles.accountTools}>
          {account.checkedAtEpoch !== null ? (
            <span className={styles.updatedAt}>
              更新于 {clockLabel(account.checkedAtEpoch)}
              {refreshing ? ' · 刷新中' : ''}
            </span>
          ) : null}
          {account.provider === 'new-api' ? (
            <button
              type="button"
              className={styles.miniButton}
              onClick={onBeginTokenEdit}
              disabled={!desktop || savingToken}
              aria-label={`为 ${host} 设置访问令牌`}
              title="补充 New API 访问令牌以读取完整账户额度"
            >
              <KeyIcon size={15} aria-hidden />
            </button>
          ) : null}
          <button
            type="button"
            className={styles.refreshButton}
            onClick={onRefresh}
            disabled={!desktop || refreshing}
            aria-label={`刷新 ${host} 的额度`}
            title={desktop ? '重新识别并刷新额度' : '浏览器预览无法调用 Rust'}
          >
            <ArrowClockwiseIcon
              size={16}
              weight="regular"
              className={refreshing ? styles.spin : undefined}
            />
          </button>
          <button
            type="button"
            className={styles.miniButton}
            onClick={onAskRemove}
            aria-label={`移除 ${host}`}
            data-danger
          >
            <TrashIcon size={15} aria-hidden />
          </button>
        </div>
      </header>

      {confirming ? (
        <div className={styles.inlineConfirm}>
          <span className={styles.usageNote}>
            移除后会删除本应用保存的密钥与访问令牌。
          </span>
          <div className={styles.inlineConfirmActions}>
            <button
              type="button"
              className={styles.dangerButton}
              onClick={onRemove}
            >
              移除
            </button>
            <button
              type="button"
              className={styles.buttonGhost}
              onClick={onCancelRemove}
            >
              取消
            </button>
          </div>
        </div>
      ) : null}

      {tokenEditing ? (
        <form
          className={styles.relayTokenForm}
          onSubmit={(event) => {
            event.preventDefault();
            const token = tokenInput.current?.value.trim() ?? '';
            const userId = userIdInput.current?.value.trim() ?? '';
            if (token) onSubmitToken(token, userId);
          }}
        >
          <label className={styles.field}>
            <span className={styles.fieldLabel}>New API 访问令牌</span>
            <input
              ref={tokenInput}
              className={styles.textInput}
              type="password"
              placeholder="旧版单令牌或个人访问令牌"
              disabled={!desktop || savingToken}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label className={styles.field}>
            <span className={styles.fieldLabel}>用户 ID（旧版令牌需要）</span>
            <input
              ref={userIdInput}
              className={styles.textInput}
              type="text"
              inputMode="numeric"
              placeholder="New-Api-User"
              defaultValue={account.newApiUserId ?? ''}
              disabled={!desktop || savingToken}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <button
            type="submit"
            className={styles.button}
            disabled={savingToken}
          >
            {savingToken ? '保存中…' : '保存并刷新'}
          </button>
          <button
            type="button"
            className={styles.buttonGhost}
            onClick={onCancelTokenEdit}
            disabled={savingToken}
          >
            取消
          </button>
        </form>
      ) : null}

      {account.lastError ? (
        <p className={styles.noticeStale} role="status">
          上次刷新失败：{account.lastError}
          {account.checkedAtEpoch !== null
            ? ' 当前显示上一次成功的数据。'
            : null}
        </p>
      ) : null}

      {!quota ? (
        <p className={styles.accountNote}>
          {refreshing
            ? '正在识别上游并查询额度…'
            : '尚未获取额度，可点击右侧刷新重试。'}
        </p>
      ) : null}
    </article>
  );
}

export default function RelayPanel() {
  const { desktop } = useAppState();
  const {
    accounts,
    skipped,
    scanning,
    loaded,
    error,
    refreshingId,
    savingTokenId,
    ensureLoaded,
    rescan: rescanAccounts,
    addAccount,
    refresh,
    remove,
    saveAccountToken,
  } = useRelay();
  const [baseUrl, setBaseUrl] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [formNotice, setFormNotice] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [tokenEditing, setTokenEditing] = useState<string | null>(null);
  const keyInput = useRef<HTMLInputElement>(null);
  const accountTokenInput = useRef<HTMLInputElement>(null);
  const userIdInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (desktop) ensureLoaded();
  }, [desktop, ensureLoaded]);

  const submit = async (event: SyntheticEvent<HTMLFormElement>) => {
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
    setFormNotice(null);
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
    } finally {
      setSubmitting(false);
    }
  };

  const saveToken = async (id: string, token: string, userId: string) => {
    const ok = await saveAccountToken(id, token, userId || undefined);
    if (ok) setTokenEditing(null);
  };

  const removeAccount = async (id: string) => {
    const ok = await remove(id);
    if (ok) {
      setConfirming(null);
      setTokenEditing(null);
    }
  };

  const message = formError ?? error;

  return (
    <section>
      <div className={styles.overviewStack}>
        <form className={styles.card} onSubmit={submit}>
          <div className={styles.relayForm}>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>服务地址</span>
              <input
                className={styles.textInput}
                type="url"
                inputMode="url"
                placeholder="https://api.example.com"
                value={baseUrl}
                onChange={(event) => setBaseUrl(event.target.value)}
                disabled={!desktop || submitting}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>API Key</span>
              <input
                ref={keyInput}
                className={styles.textInput}
                type="password"
                placeholder="sk-..."
                disabled={!desktop || submitting}
                autoComplete="off"
                spellCheck={false}
                aria-describedby="relay-key-note"
              />
            </label>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>访问令牌（可选，New API）</span>
              <input
                ref={accountTokenInput}
                className={styles.textInput}
                type="password"
                placeholder="用于读取账户余额与订阅"
                disabled={!desktop || submitting}
                autoComplete="off"
                spellCheck={false}
                aria-describedby="relay-key-note"
              />
            </label>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>用户 ID（旧版令牌需要）</span>
              <input
                ref={userIdInput}
                className={styles.textInput}
                type="text"
                inputMode="numeric"
                placeholder="New-Api-User"
                disabled={!desktop || submitting}
                autoComplete="off"
                spellCheck={false}
                aria-describedby="relay-key-note"
              />
            </label>
            <button
              type="submit"
              className={styles.button}
              disabled={!desktop || submitting || scanning}
            >
              {submitting ? '识别中…' : '识别并查询'}
            </button>
            <button
              type="button"
              className={styles.buttonGhost}
              onClick={() => void rescanAccounts()}
              disabled={!desktop || submitting || scanning}
            >
              {scanning ? '扫描中…' : '重新扫描本地配置'}
            </button>
          </div>
          <p className={styles.usageNote} id="relay-key-note">
            添加账户后，额度会显示在总览的「中转站账户额度」；本页负责维护账户与凭证。
            <br />
            打开本页或切换到总览的中转站额度时会读取 Claude Code 的{' '}
            <code className={styles.codeChip}>settings.json</code> 与 Codex 的{' '}
            <code className={styles.codeChip}>config.toml</code>
            ，自动识别其中的中转站；识别不出来时也可以手动填写。自动尝试 New API 与
            Sub2API 的额度接口，无需先选择平台。
            <br />
            手动填写的地址、密钥与访问令牌保存在本应用数据目录的{' '}
            <code className={styles.codeChip}>relay-profiles/</code>
            ，不会进入状态文件或界面；自动扫描到的配置只在本次运行使用。账户额度方面：New
            API 需要 v1.0.0-rc.45 及以上并在密钥上开启「允许查询用户额度」，或填写访问令牌（v1.0.0-rc.41
            及以上，需 profile:read、订阅还需 wallet:read）；旧版单令牌还需要同时填写面板个人设置里的用户 ID（New-Api-User）。部分站点会直接在密钥响应中返回账户额度
            （quota_scope=account），无需额外配置。Sub2API 在密钥无总额度、无限速且属于普通分组时
            返回账户钱包余额，订阅分组返回账户订阅额度。
          </p>
          {skipped.length > 0 ? (
            <div className={styles.relaySkipped}>
              <p className={styles.usageNote}>
                以下本地配置未识别为 New API / Sub2API：
              </p>
              <ul>
                {skipped.map((item, index) => (
                  <li key={`${item.origin}-${item.baseUrl}-${index}`}>
                    <span>
                      {RELAY_ORIGIN_LABELS[item.origin]} · {item.baseUrl}
                    </span>
                    <span className={styles.quotaMeta}>{item.reason}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          {!desktop ? (
            <p className={styles.accountNote}>
              浏览器预览无法调用 Rust，请在桌面应用内使用（
              <code className={styles.codeChip}>pnpm tauri dev</code>）。
            </p>
          ) : null}
          {message ? (
            <p className={styles.noticeDanger} role="alert">
              {message}
            </p>
          ) : null}
          {formNotice ? (
            <p className={styles.accountNote} role="status">
              {formNotice}
            </p>
          ) : null}
        </form>

        {accounts.length === 0 ? (
          <div className={styles.overviewEmpty}>
            <span className={styles.emptyGlyph} aria-hidden="true">
              <PlugsIcon size={24} weight="duotone" />
            </span>
            <h2 className={styles.emptyTitle}>还没有中转站账户</h2>
            <p className={styles.usageNote}>
              {desktop && (!loaded || scanning)
                ? '正在扫描 Claude Code 与 Codex 的本地配置…'
                : '没有从本地配置或手动输入中识别到中转站。在上方填入地址与 API Key，或重新扫描本地配置。'}
            </p>
          </div>
        ) : (
          <div className={styles.accountStack}>
            {accounts.map((account) => (
              <RelayManageCard
                key={account.id}
                account={account}
                desktop={desktop}
                refreshing={refreshingId === account.id}
                confirming={confirming === account.id}
                tokenEditing={tokenEditing === account.id}
                savingToken={savingTokenId === account.id}
                onRefresh={() => void refresh(account.id)}
                onAskRemove={() => setConfirming(account.id)}
                onCancelRemove={() => setConfirming(null)}
                onRemove={() => void removeAccount(account.id)}
                onBeginTokenEdit={() => setTokenEditing(account.id)}
                onCancelTokenEdit={() => setTokenEditing(null)}
                onSubmitToken={(token, userId) =>
                  void saveToken(account.id, token, userId)
                }
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
