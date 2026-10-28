import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { isTauri } from '@tauri-apps/api/core';
import {
  importRelayCandidates,
  listRelayAccounts,
  onRelayAccountsChanged,
  queryRelayAccount,
  refreshRelayAccount,
  removeRelayAccount,
  scanRelayCandidates,
  setRelayAccountToken,
  type DiscoveryReport,
  type RelayAccount,
  type RelayAccountsEvent,
  type RelayCandidate,
  type SkippedConfig,
} from './desktop';
import { relayErrorText } from './relayFormat';

/**
 * 中转站账户的共享状态：总览的「中转站账户额度」与「中转站账户管理」使用同一份
 * 数据，避免两个页面各自刷新后互不同步。密钥与访问令牌只在 Rust 侧，这里只保存
 * 脱敏后的账户、候选与额度视图。
 */
interface RelayContextValue {
  accounts: RelayAccount[];
  /** 最近一次扫描出的候选；用户勾选后才导入。 */
  candidates: RelayCandidate[];
  skipped: SkippedConfig[];
  /** 正在扫描本地配置。 */
  scanning: boolean;
  /** 正在导入勾选的候选并查询额度。 */
  importing: boolean;
  /** 已保存账户列表是否已经加载（成功或失败），用于区分“尚未加载”和“确实为空”。 */
  loaded: boolean;
  /** 最近一次扫描/查询的错误；卡片自身的额度错误仍在 account.lastError 上。 */
  error: string | null;
  /** 清除页面级错误；打开新的操作弹窗时避免展示上一次的失败信息。 */
  clearError: () => void;
  /** 正在刷新的账户；批量刷新时可能同时有多个。 */
  refreshingIds: ReadonlySet<string>;
  /** 首次进入中转站相关页面时调用；只读取已保存条目，不扫描本地配置。 */
  ensureLoaded: () => void;
  /** 重新读取条目列表；窗口重新显示时兜底同步，避免漏掉事件。 */
  reload: () => Promise<void>;
  /** 扫描 cc-switch、Claude Code / Codex 的本地配置，只返回候选。 */
  scanCandidates: () => Promise<RelayCandidate[] | null>;
  /** 导入勾选的候选；失败的候选写入 skipped。 */
  importCandidates: (ids: string[]) => Promise<DiscoveryReport | null>;
  /**
   * 进入中转站相关页面时调用：把距上次成功查询已超过 `refreshMinutes`
   * 的账户批量刷新一次。同一账户在间隔内不会重复自动刷新（失败也算一次）。
   */
  refreshDue: (refreshMinutes: number) => Promise<void>;
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
  const [candidates, setCandidates] = useState<RelayCandidate[]>([]);
  const [skipped, setSkipped] = useState<SkippedConfig[]>([]);
  const [scanning, setScanning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshingIds, setRefreshingIds] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  // 与 refreshingIds 同步的 ref：批量刷新需要在同一批事件里判断哪些账户
  // 已在刷新，等 state 提交会漏掉 StrictMode 的重复 effect。
  const refreshingRef = useRef<ReadonlySet<string>>(new Set());
  // 本次运行中每个账户最近一次自动刷新的时间；失败也记录，避免切页面重试。
  const lastAutoRefresh = useRef(new Map<string, number>());
  const started = useRef(false);
  // 最近一次应用的条目修订号，用来丢弃乱序到达的旧快照。
  const revision = useRef(0);

  /** 应用来自命令或事件的完整条目列表；两端都用“最新在前”的顺序。 */
  const applyAccounts = useCallback((payload: RelayAccountsEvent) => {
    if (payload.revision < revision.current) return;
    revision.current = payload.revision;
    setAccounts([...payload.accounts].reverse());
    setLoaded(true);
  }, []);

  const markRefreshing = useCallback((id: string, active: boolean) => {
    const next = new Set(refreshingRef.current);
    if (active) next.add(id);
    else next.delete(id);
    refreshingRef.current = next;
    setRefreshingIds(next);
  }, []);

  const reload = useCallback(async () => {
    if (!desktop) return;
    setError(null);
    // 只读取已保存条目与上次成功快照；扫描本地配置由用户手动触发。
    try {
      applyAccounts(await listRelayAccounts());
    } catch (err) {
      setError(relayErrorText(err));
    } finally {
      setLoaded(true);
    }
  }, [desktop, applyAccounts]);

  const load = useCallback(async () => {
    if (!desktop) return;
    started.current = true;
    await reload();
  }, [desktop, reload]);

  const ensureLoaded = useCallback(() => {
    if (!desktop || started.current) return;
    void load();
  }, [desktop, load]);

  // 主窗口与菜单栏面板各自持有前端副本，真正共享的是 Rust 注册表：
  // 任一侧添加、刷新、补充令牌或移除后都会广播完整列表，这里同步另一侧。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onRelayAccountsChanged((event) => {
      if (!disposed) applyAccounts(event);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop, applyAccounts]);

  const scanCandidates = useCallback(async () => {
    if (!desktop) return null;
    setScanning(true);
    setError(null);
    try {
      const found = await scanRelayCandidates();
      setCandidates(found);
      return found;
    } catch (err) {
      setError(relayErrorText(err));
      return null;
    } finally {
      setScanning(false);
    }
  }, [desktop]);

  const importCandidates = useCallback(
    async (ids: string[]) => {
      if (!desktop || ids.length === 0) return null;
      setImporting(true);
      setError(null);
      try {
        const report = await importRelayCandidates(ids);
        // 导入结果已包含完整列表；沿用统一的反向排序。
        setAccounts([...report.accounts].reverse());
        setSkipped(report.skipped);
        return report;
      } catch (err) {
        setError(relayErrorText(err));
        return null;
      } finally {
        setImporting(false);
      }
    },
    [desktop],
  );

  const clearError = useCallback(() => setError(null), []);

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
      markRefreshing(id, true);
      setError(null);
      try {
        const updated = await refreshRelayAccount(id);
        setAccounts((current) =>
          current.map((account) => (account.id === id ? updated : account)),
        );
      } catch (err) {
        setError(relayErrorText(err));
      } finally {
        markRefreshing(id, false);
      }
    },
    [desktop, markRefreshing],
  );

  /**
   * 页面打开时自动刷新过期账户。只看已成功查询的时间：失败的账户由
   * `lastAutoRefresh` 按同一间隔退避，避免每次切换页面都重新请求。
   */
  const refreshDue = useCallback(
    async (refreshMinutes: number) => {
      if (!desktop) return;
      const now = Math.floor(Date.now() / 1000);
      // 合法选项最小为 1 分钟；兜底成 0 会让每次状态更新都重新刷新。
      const interval = Math.max(60, refreshMinutes * 60);
      const due = accounts.filter((account) => {
        if (refreshingRef.current.has(account.id)) return false;
        const attemptedAt = lastAutoRefresh.current.get(account.id);
        if (attemptedAt !== undefined && now - attemptedAt < interval) {
          return false;
        }
        return (
          account.checkedAtEpoch === null
          || now - account.checkedAtEpoch >= interval
        );
      });
      if (due.length === 0) return;
      for (const account of due) {
        lastAutoRefresh.current.set(account.id, now);
      }
      // 并发查询；每个账户的失败都落在自己的 lastError 上，不互相中断。
      await Promise.all(due.map((account) => refresh(account.id)));
    },
    [accounts, desktop, refresh],
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
      }
    },
    [desktop],
  );

  const value = useMemo<RelayContextValue>(
    () => ({
      accounts,
      candidates,
      skipped,
      scanning,
      importing,
      loaded,
      error,
      clearError,
      refreshingIds,
      ensureLoaded,
      reload,
      scanCandidates,
      importCandidates,
      refreshDue,
      addAccount,
      refresh,
      remove,
      saveAccountToken,
    }),
    [
      accounts,
      candidates,
      skipped,
      scanning,
      importing,
      loaded,
      error,
      clearError,
      refreshingIds,
      ensureLoaded,
      reload,
      scanCandidates,
      importCandidates,
      refreshDue,
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
