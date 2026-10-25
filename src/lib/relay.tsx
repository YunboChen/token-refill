import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { isTauri } from '@tauri-apps/api/core';
import {
  discoverRelayAccounts,
  queryRelayAccount,
  refreshRelayAccount,
  removeRelayAccount,
  setRelayAccountToken,
  type RelayAccount,
  type SkippedConfig,
} from './desktop';
import { relayErrorText } from './relayFormat';

/**
 * 中转站账户的共享状态：总览的「中转站账户额度」与「中转站账户管理」使用同一份
 * 数据，避免两个页面各自扫描、刷新后互不同步。密钥与访问令牌只在 Rust 侧，
 * 这里只保存脱敏后的账户与额度视图。
 */
interface RelayContextValue {
  accounts: RelayAccount[];
  skipped: SkippedConfig[];
  /** 正在扫描本地配置或刷新账号列表。 */
  scanning: boolean;
  /** 首次扫描是否已经结束（成功或失败），用于区分“尚未加载”和“确实为空”。 */
  loaded: boolean;
  /** 最近一次扫描/查询的错误；卡片自身的额度错误仍在 account.lastError 上。 */
  error: string | null;
  refreshingId: string | null;
  savingTokenId: string | null;
  /** 首次进入中转站相关页面时调用；重复调用不会重复扫描。 */
  ensureLoaded: () => void;
  rescan: () => Promise<void>;
  /** 添加失败返回 null，错误写入 error。 */
  addAccount: (
    baseUrl: string,
    apiKey: string,
    accountToken?: string,
    newApiUserId?: string,
  ) => Promise<RelayAccount | null>;
  refresh: (id: string) => Promise<void>;
  remove: (id: string) => Promise<boolean>;
  saveAccountToken: (
    id: string,
    accountToken: string,
    newApiUserId?: string,
  ) => Promise<boolean>;
}

const RelayContext = createContext<RelayContextValue | null>(null);

export function RelayProvider({ children }: { children: ReactNode }) {
  const desktop = isTauri();
  const [accounts, setAccounts] = useState<RelayAccount[]>([]);
  const [skipped, setSkipped] = useState<SkippedConfig[]>([]);
  const [scanning, setScanning] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshingId, setRefreshingId] = useState<string | null>(null);
  const [savingTokenId, setSavingTokenId] = useState<string | null>(null);
  const started = useRef(false);

  const discover = useCallback(async () => {
    if (!desktop) return;
    setScanning(true);
    setError(null);
    try {
      const report = await discoverRelayAccounts();
      // 最近添加的条目排在最前，与管理页的添加顺序一致。
      setAccounts([...report.accounts].reverse());
      setSkipped(report.skipped);
    } catch (err) {
      setError(relayErrorText(err));
    } finally {
      setScanning(false);
      setLoaded(true);
    }
  }, [desktop]);

  const ensureLoaded = useCallback(() => {
    if (!desktop || started.current) return;
    started.current = true;
    void discover();
  }, [desktop, discover]);

  const rescan = useCallback(async () => {
    if (!desktop) return;
    started.current = true;
    await discover();
  }, [desktop, discover]);

  const addAccount = useCallback(
    async (
      baseUrl: string,
      apiKey: string,
      accountToken?: string,
      newApiUserId?: string,
    ) => {
      if (!desktop) {
        setError('浏览器预览无法调用桌面接口。');
        return null;
      }
      setError(null);
      try {
        const account = await queryRelayAccount(
          baseUrl,
          apiKey,
          accountToken,
          newApiUserId,
        );
        setAccounts((current) => [
          account,
          ...current.filter((item) => item.id !== account.id),
        ]);
        return account;
      } catch (err) {
        setError(relayErrorText(err));
        return null;
      }
    },
    [desktop],
  );

  const refresh = useCallback(
    async (id: string) => {
      if (!desktop) return;
      setRefreshingId(id);
      setError(null);
      try {
        const updated = await refreshRelayAccount(id);
        setAccounts((current) =>
          current.map((account) => (account.id === id ? updated : account)),
        );
      } catch (err) {
        setError(relayErrorText(err));
      } finally {
        setRefreshingId((current) => (current === id ? null : current));
      }
    },
    [desktop],
  );

  const remove = useCallback(
    async (id: string) => {
      if (!desktop) return false;
      setError(null);
      try {
        await removeRelayAccount(id);
        setAccounts((current) => current.filter((account) => account.id !== id));
        return true;
      } catch (err) {
        setError(relayErrorText(err));
        return false;
      }
    },
    [desktop],
  );

  const saveAccountToken = useCallback(
    async (id: string, accountToken: string, newApiUserId?: string) => {
      if (!desktop) return false;
      setSavingTokenId(id);
      setError(null);
      try {
        const updated = await setRelayAccountToken(
          id,
          accountToken,
          newApiUserId,
        );
        setAccounts((current) =>
          current.map((account) => (account.id === id ? updated : account)),
        );
        return true;
      } catch (err) {
        setError(relayErrorText(err));
        return false;
      } finally {
        setSavingTokenId((current) => (current === id ? null : current));
      }
    },
    [desktop],
  );

  const value = useMemo<RelayContextValue>(
    () => ({
      accounts,
      skipped,
      scanning,
      loaded,
      error,
      refreshingId,
      savingTokenId,
      ensureLoaded,
      rescan,
      addAccount,
      refresh,
      remove,
      saveAccountToken,
    }),
    [
      accounts,
      skipped,
      scanning,
      loaded,
      error,
      refreshingId,
      savingTokenId,
      ensureLoaded,
      rescan,
      addAccount,
      refresh,
      remove,
      saveAccountToken,
    ],
  );

  return <RelayContext.Provider value={value}>{children}</RelayContext.Provider>;
}

export function useRelay(): RelayContextValue {
  const value = useContext(RelayContext);
  if (!value) {
    throw new Error('useRelay must be used inside RelayProvider');
  }
  return value;
}
