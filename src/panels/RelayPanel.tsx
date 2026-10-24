import { useEffect, useRef, useState, type SyntheticEvent } from 'react';
import { ArrowClockwiseIcon, KeyIcon, PlugsIcon, TrashIcon } from '@phosphor-icons/react';
import {
  discoverRelayAccounts,
  queryRelayAccount,
  refreshRelayAccount,
  removeRelayAccount,
  setRelayAccountToken,
  type AccountQuota,
  type KeyQuota,
  type KeyQuotaWindow,
  type QuotaMode,
  type QuotaProvider,
  type RelayAccount,
  type RelayOrigin,
  type SkippedConfig,
} from '../lib/desktop';
import { clockLabel, durationLabel, resetLabel } from '../lib/format';
import { useAppState } from '../lib/store';
import styles from '../App.module.css';

const PROVIDER_LABELS: Record<QuotaProvider, string> = {
  'new-api': 'New API',
  'sub2-api': 'Sub2API',
};

const MODE_LABELS: Record<QuotaMode, string> = {
  key: '密钥额度',
  subscription: '订阅额度',
  wallet: '钱包余额',
  unrestricted: '账户余额',
};

const ORIGIN_LABELS: Record<RelayOrigin, string> = {
  manual: '手动输入',
  'claude-code': 'Claude Code 配置',
  codex: 'Codex 配置',
};

function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return '查询失败，请检查服务地址与密钥。';
}

function formatMoney(value: number | null, currency: string | null): string {
  if (value === null) return '—';
  switch ((currency ?? 'USD').toUpperCase()) {
    case 'USD':
      return `$${value.toFixed(2)}`;
    case 'CNY':
    case 'RMB':
      return `¥${value.toFixed(2)}`;
    default:
      return `${value.toFixed(2)} ${currency ?? ''}`.trim();
  }
}

function relayWindowLabel(window: KeyQuotaWindow): string {
  const duration = durationLabel(window.durationMins);
  return duration ? `${duration}窗口` : window.windowId;
}

function expiryLabel(epoch: number | null): string {
  if (epoch === null) return '未提供到期时间';
  return `到期 ${new Date(epoch * 1000).toLocaleDateString([], {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  })}`;
}

function mainPercent(quota: KeyQuota): number | null {
  if (quota.limitUsd === null || quota.remainingUsd === null || quota.limitUsd <= 0) {
    return null;
  }
  return Math.round(
    Math.min(100, Math.max(0, (quota.remainingUsd / quota.limitUsd) * 100)),
  );
}

function WindowList({
  windows,
  threshold,
  currency,
  title,
}: {
  windows: KeyQuotaWindow[];
  threshold: number;
  currency: string | null;
  title?: string;
}) {
  if (windows.length === 0) return null;
  return (
    <div className={styles.relayWindows}>
      {title ? <p className={styles.relaySectionLabel}>{title}</p> : null}
      {windows.map((item) => (
        <div className={styles.relayWindow} key={item.windowId}>
          <div className={styles.relayWindowHead}>
            <span>{relayWindowLabel(item)}</span>
            <span className={styles.quotaMeta}>
              {item.limitUsd !== null
                ? `${formatMoney(item.remainingUsd, currency)} / ${formatMoney(item.limitUsd, currency)}`
                : formatMoney(item.usedUsd, currency)}
              {item.remainingPercent !== null
                ? ` · 剩余 ${item.remainingPercent}%`
                : ''}
            </span>
          </div>
          {item.remainingPercent !== null ? (
            <div
              className={styles.relayBar}
              data-low={item.remainingPercent <= threshold || undefined}
              role="progressbar"
              aria-label={`${relayWindowLabel(item)}剩余额度比例`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={item.remainingPercent}
            >
              <span
                className={styles.relayBarFill}
                style={{ width: `${item.remainingPercent}%` }}
              />
            </div>
          ) : null}
          <span className={styles.quotaMeta}>
            {resetLabel(item.resetsAtEpoch)}
            {item.usedUsd !== null ? ` · 已用 ${formatMoney(item.usedUsd, currency)}` : ''}
          </span>
        </div>
      ))}
    </div>
  );
}

function AccountSummary({
  account,
  currency,
}: {
  account: AccountQuota;
  currency: string | null;
}) {
  const source =
    account.source === 'access-token' ? '账户余额（访问令牌）' : '账户余额';
  return (
    <span>
      {' · '}
      {source} {formatMoney(account.remainingUsd, currency)}
      {account.usedUsd !== null
        ? ` · 已用 ${formatMoney(account.usedUsd, currency)}`
        : ''}
      {account.totalUsd !== null
        ? ` · 总额 ${formatMoney(account.totalUsd, currency)}`
        : ''}
    </span>
  );
}

function RelayCard({
  account,
  desktop,
  refreshing,
  confirming,
  tokenEditing,
  savingToken,
  threshold,
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
  threshold: number;
  onRefresh: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
  onRemove: () => void;
  onBeginTokenEdit: () => void;
  onCancelTokenEdit: () => void;
  onSubmitToken: (token: string, userId: string) => void;
}) {
  const quota = account.quota;
  const percent = quota ? mainPercent(quota) : null;
  const low = percent !== null && percent <= threshold;
  const host = account.baseUrl.replace(/^https?:\/\//, '');
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
            <span className={styles.planTag}>{ORIGIN_LABELS[account.origin]}</span>
            <span className={styles.providerTag}>
              {PROVIDER_LABELS[account.provider]}
            </span>
            {quota ? <span className={styles.planTag}>{MODE_LABELS[quota.mode]}</span> : null}
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
          <span className={styles.usageNote}>移除后会删除本应用保存的密钥与访问令牌。</span>
          <div className={styles.inlineConfirmActions}>
            <button type="button" className={styles.dangerButton} onClick={onRemove}>
              移除
            </button>
            <button type="button" className={styles.buttonGhost} onClick={onCancelRemove}>
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
          <button type="submit" className={styles.button} disabled={savingToken}>
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
          {account.checkedAtEpoch !== null ? ' 当前显示上一次成功的数据。' : null}
        </p>
      ) : null}

      {!quota ? (
        <p className={styles.accountNote}>
          {refreshing ? '正在识别上游并查询额度…' : '尚未获取额度。'}
        </p>
      ) : (
        <>
          <div className={styles.relaySummary}>
            <div>
              <p className={styles.relayAmount}>
                {quota.remainingUsd !== null
                  ? formatMoney(quota.remainingUsd, quota.currency)
                  : quota.unlimited
                    ? '不限额'
                    : '—'}
              </p>
              <p className={styles.quotaMeta}>
                <span>剩余额度</span>
                {!quota.unlimited && quota.limitUsd !== null ? (
                  <span>
                    {' · '}总额 {formatMoney(quota.limitUsd, quota.currency)} · 已用{' '}
                    {formatMoney(quota.usedUsd, quota.currency)}
                  </span>
                ) : null}
                {quota.account ? (
                  <AccountSummary account={quota.account} currency={quota.currency} />
                ) : null}
              </p>
            </div>
            {percent !== null ? (
              <div className={styles.relayBarWrap}>
                <div
                  className={styles.relayBar}
                  data-low={low || undefined}
                  role="progressbar"
                  aria-label="剩余额度比例"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={percent}
                  aria-valuetext={`剩余 ${percent}%`}
                >
                  <span className={styles.relayBarFill} style={{ width: `${percent}%` }} />
                </div>
                <span className={styles.relayScale}>
                  <span>剩余 {percent}%</span>
                  {low ? <span className={styles.quotaLowNote}>低于提醒阈值 {threshold}%</span> : null}
                </span>
              </div>
            ) : null}
          </div>

          <p className={styles.relayMetaLine}>
            <span>{expiryLabel(quota.expiresAtEpoch)}</span>
            {quota.unlimited ? <span> · 平台未设置总额度上限</span> : null}
          </p>

          <WindowList
            windows={quota.windows}
            threshold={threshold}
            currency={quota.currency}
          />
          {quota.account ? (
            <WindowList
              windows={quota.account.windows}
              threshold={threshold}
              currency={quota.currency}
              title="账户订阅"
            />
          ) : null}
          {quota.accountError ? (
            <p className={styles.accountNote}>{quota.accountError}</p>
          ) : null}
        </>
      )}
    </article>
  );
}

export default function RelayPanel() {
  const { desktop, state } = useAppState();
  const [accounts, setAccounts] = useState<RelayAccount[]>([]);
  const [baseUrl, setBaseUrl] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [formNotice, setFormNotice] = useState<string | null>(null);
  const [skipped, setSkipped] = useState<SkippedConfig[]>([]);
  const [scanning, setScanning] = useState(false);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [tokenEditing, setTokenEditing] = useState<string | null>(null);
  const [savingToken, setSavingToken] = useState<string | null>(null);
  const keyInput = useRef<HTMLInputElement>(null);
  const accountTokenInput = useRef<HTMLInputElement>(null);
  const userIdInput = useRef<HTMLInputElement>(null);
  const threshold = state.preferences.lowQuotaThreshold;

  useEffect(() => {
    if (!desktop) return;
    let cancelled = false;
    setScanning(true);
    discoverRelayAccounts()
      .then((report) => {
        if (cancelled) return;
        setAccounts([...report.accounts].reverse());
        setSkipped(report.skipped);
      })
      .catch((error: unknown) => {
        if (!cancelled) setFormError(errorText(error));
      })
      .finally(() => {
        if (!cancelled) setScanning(false);
      });
    return () => {
      cancelled = true;
    };
  }, [desktop]);

  const rescan = async () => {
    setScanning(true);
    setFormError(null);
    setFormNotice(null);
    try {
      const report = await discoverRelayAccounts();
      setAccounts([...report.accounts].reverse());
      setSkipped(report.skipped);
    } catch (error) {
      setFormError(errorText(error));
    } finally {
      setScanning(false);
    }
  };

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

    setLoading(true);
    setFormError(null);
    setFormNotice(null);
    try {
      const accountToken = accountTokenInput.current?.value.trim() ?? '';
      const userId = userIdInput.current?.value.trim() ?? '';
      const account = await queryRelayAccount(
        baseUrl.trim(),
        apiKey,
        accountToken || undefined,
        userId || undefined,
      );
      setAccounts((current) => [account, ...current.filter((item) => item.id !== account.id)]);
      if (accountToken && account.provider !== 'new-api') {
        setFormNotice('访问令牌只对 New API 生效；当前站点识别为 Sub2API，访问令牌未保存。');
      }
      if (keyInput.current) keyInput.current.value = '';
      if (accountTokenInput.current) accountTokenInput.current.value = '';
      if (userIdInput.current) userIdInput.current.value = '';
    } catch (error) {
      setFormError(errorText(error));
    } finally {
      setLoading(false);
    }
  };

  const saveAccountToken = async (id: string, token: string, userId: string) => {
    setSavingToken(id);
    setFormError(null);
    try {
      const updated = await setRelayAccountToken(id, token, userId || undefined);
      setAccounts((current) =>
        current.map((account) => (account.id === id ? updated : account)),
      );
      setTokenEditing(null);
    } catch (error) {
      setFormError(errorText(error));
    } finally {
      setSavingToken(null);
    }
  };

  const refresh = async (id: string) => {
    setRefreshing(id);
    setFormError(null);
    try {
      const updated = await refreshRelayAccount(id);
      setAccounts((current) =>
        current.map((account) => (account.id === id ? updated : account)),
      );
    } catch (error) {
      setFormError(errorText(error));
    } finally {
      setRefreshing(null);
    }
  };

  const remove = async (id: string) => {
    setFormError(null);
    try {
      await removeRelayAccount(id);
      setAccounts((current) => current.filter((account) => account.id !== id));
      setConfirming(null);
      setTokenEditing(null);
    } catch (error) {
      setFormError(errorText(error));
    }
  };

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
                disabled={!desktop || loading}
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
                disabled={!desktop || loading}
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
                disabled={!desktop || loading}
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
                disabled={!desktop || loading}
                autoComplete="off"
                spellCheck={false}
                aria-describedby="relay-key-note"
              />
            </label>
            <button
              type="submit"
              className={styles.button}
              disabled={!desktop || loading || scanning}
            >
              {loading ? '识别中…' : '识别并查询'}
            </button>
            <button
              type="button"
              className={styles.buttonGhost}
              onClick={() => void rescan()}
              disabled={!desktop || loading || scanning}
            >
              {scanning ? '扫描中…' : '重新扫描本地配置'}
            </button>
          </div>
          <p className={styles.usageNote} id="relay-key-note">
            打开本页时会读取 Claude Code 的 <code className={styles.codeChip}>settings.json</code>
            与 Codex 的 <code className={styles.codeChip}>config.toml</code>
            ，自动识别其中的中转站；识别不出来时也可以手动填写。自动尝试 New API 与 Sub2API
            的额度接口，无需先选择平台。
            <br />
            手动填写的地址、密钥与访问令牌保存在本应用数据目录的{' '}
            <code className={styles.codeChip}>relay-profiles/</code>
            ，不会进入状态文件或界面；自动扫描到的配置只在本次运行使用。账户额度方面：New API
            需要 v1.0.0-rc.45 及以上并在密钥上开启「允许查询用户额度」，或填写访问令牌（v1.0.0-rc.41
            及以上，需 profile:read、订阅还需 wallet:read）；旧版单令牌还需要同时填写面板个人设置里的用户 ID（New-Api-User）。部分站点会直接在密钥响应中返回账户额度
            （quota_scope=account），无需额外配置。Sub2API 在密钥无总额度、无限速且属于普通分组时
            返回账户钱包余额，订阅分组返回账户订阅额度。
          </p>
          {skipped.length > 0 ? (
            <div className={styles.relaySkipped}>
              <p className={styles.usageNote}>以下本地配置未识别为 New API / Sub2API：</p>
              <ul>
                {skipped.map((item, index) => (
                  <li key={`${item.origin}-${item.baseUrl}-${index}`}>
                    <span>
                      {ORIGIN_LABELS[item.origin]} · {item.baseUrl}
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
          {formError ? (
            <p className={styles.noticeDanger} role="alert">
              {formError}
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
            <h2 className={styles.emptyTitle}>还没有查询过中转站额度</h2>
            <p className={styles.usageNote}>
              {scanning
                ? '正在扫描 Claude Code 与 Codex 的本地配置…'
                : '没有从本地配置或手动输入中识别到中转站。填入地址与 API Key 后会显示平台、额度、限速窗口与到期时间。'}
            </p>
          </div>
        ) : (
          <div className={styles.accountStack}>
            {accounts.map((account) => (
              <RelayCard
                key={account.id}
                account={account}
                desktop={desktop}
                refreshing={refreshing === account.id}
                confirming={confirming === account.id}
                tokenEditing={tokenEditing === account.id}
                savingToken={savingToken === account.id}
                threshold={threshold}
                onRefresh={() => void refresh(account.id)}
                onAskRemove={() => setConfirming(account.id)}
                onCancelRemove={() => setConfirming(null)}
                onRemove={() => void remove(account.id)}
                onBeginTokenEdit={() => setTokenEditing(account.id)}
                onCancelTokenEdit={() => setTokenEditing(null)}
                onSubmitToken={(token, userId) =>
                  void saveAccountToken(account.id, token, userId)
                }
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
