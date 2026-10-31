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
import { version as packageVersion } from '../../package.json';
import {
  addAntigravityAccount as addAntigravityAccountCommand,
  addApiKeyAccount as addApiKeyAccountCommand,
  beginAccountLogin,
  beginBailianLogin as beginBailianLoginCommand,
  cancelAccountLogin,
  checkForUpdate,
  getAppInfo,
  getAppState,
  getStateFileNotice,
  onAppStateChanged,
  openExternalUrl,
  refreshUsage,
  removeAccount as removeAccountCommand,
  renameAccount as renameAccountCommand,
  replaceApiKey as replaceApiKeyCommand,
  setAutoLaunch as setAutoLaunchCommand,
  setOverviewOrder as setOverviewOrderCommand,
  setRelayOrder as setRelayOrderCommand,
  updatePreferences,
  type AppInfo,
  type AppState,
  type LoginStart,
  type OverviewOrder,
  type Preferences,
  type UpdateStatus,
} from './desktop';
import { clearLegacyPreferences, defaultAppState, readLegacyPreferences } from './prefs';
import { IS_TRAY_VIEW } from './view';

/** 自动复查间隔：新版本发布后最迟约 6 小时被发现。 */
const UPDATE_RECHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
/** 静默检查失败（离线、限流）后的重试间隔。 */
const UPDATE_RETRY_INTERVAL_MS = 30 * 60 * 1000;
/** 窗口重新获得焦点时的最小复查间隔，避免频繁请求接口。 */
const UPDATE_FOCUS_MIN_INTERVAL_MS = 60 * 60 * 1000;

/** 浏览器预览拿不到 Rust 的应用信息，回退到 package.json 的版本与正式渠道。 */
const fallbackAppInfo: AppInfo = {
  name: 'Token Refill',
  version: packageVersion,
  platform: 'web',
  arch: 'unknown',
  channel: 'pro',
};

/** 发起授权的结果：失败原因随结果返回，由发起方在弹窗内展示，不进入页面级提示。 */
export interface LoginAttempt {
  start: LoginStart | null;
  error: string | null;
}

interface AppStateContextValue {
  state: AppState;
  /** 应用名、版本与渠道；Rust 侧加载完成前为 null。 */
  appInfo: AppInfo | null;
  ready: boolean;
  desktop: boolean;
  error: string | null;
  /** 状态文件来自更高版本时的只读提示；正常为 null。 */
  stateFileNotice: string | null;
  update: UpdateStatus | null;
  refresh: (accountId?: string) => Promise<void>;
  /** 用完整快照对齐一次；窗口重新显示时兜底隐藏期间漏掉的事件。 */
  resync: () => Promise<void>;
  savePreferences: (preferences: Preferences) => Promise<boolean>;
  /** 保存总览的平台与账户排序；浏览器预览只在当前界面生效。 */
  saveOverviewOrder: (order: OverviewOrder) => Promise<boolean>;
  /** 保存总览中转站账户的排序；浏览器预览只在当前界面生效。 */
  saveRelayOrder: (ids: string[]) => Promise<boolean>;
  beginLogin: (accountId?: string, displayName?: string) => Promise<LoginAttempt>;
  beginBailianLogin: (
    accountId?: string,
    displayName?: string,
  ) => Promise<LoginAttempt>;
  addApiKeyAccount: (
    platform: string,
    displayName: string | undefined,
    apiKey: string,
  ) => Promise<string | null>;
  replaceApiKey: (accountId: string, apiKey: string) => Promise<string | null>;
  addAntigravityAccount: (displayName?: string) => Promise<string | null>;
  cancelLogin: (loginId: string) => Promise<void>;
  renameAccount: (accountId: string, displayName: string) => Promise<boolean>;
  removeAccount: (accountId: string) => Promise<boolean>;
  setAutoLaunch: (enabled: boolean) => Promise<boolean>;
  openExternal: (url: string) => Promise<void>;
}

const AppStateContext = createContext<AppStateContextValue | null>(null);

function errorMessage(err: unknown): string {
  if (typeof err === 'object' && err !== null && 'message' in err) {
    const message = (err as { message?: unknown }).message;
    if (typeof message === 'string' && message) return message;
  }
  return err instanceof Error ? err.message : String(err);
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  const desktop = isTauri();
  const [state, setState] = useState<AppState>(defaultAppState);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(
    desktop ? null : fallbackAppInfo,
  );
  const [ready, setReady] = useState(!desktop);
  const [error, setError] = useState<string | null>(null);
  const [stateFileNotice, setStateFileNotice] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);

  // 状态文件由更高版本写入时，Rust 侧只读运行；提示一次即可，不进入 error，
  // 避免覆盖掉真正来自操作的失败原因。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    void getStateFileNotice().then(
      (notice) => {
        if (!disposed) setStateFileNotice(notice);
      },
      () => {
        // 读取失败按正常读写处理，不阻塞其余状态加载。
      },
    );
    return () => {
      disposed = true;
    };
  }, [desktop]);

  // 应用名、版本与渠道以 Rust 合并后的配置为准；浏览器预览沿用 package.json 回退值。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    void getAppInfo().then(
      (info) => {
        if (!disposed) setAppInfo(info);
      },
      () => {
        // 读取失败时保持回退值，不阻塞其余状态加载。
      },
    );
    return () => {
      disposed = true;
    };
  }, [desktop]);

  // 启动时静默检查一次版本；失败（离线、限流）不打扰用户，设置页仍可手动重试。
  // 应用会长时间驻留（支持开机自启），只查一次会错过启动之后发布的新版本，
  // 因此定期复查，并在窗口重新获得焦点时按节流复查；失败后较短间隔重试。
  // 菜单栏面板与主窗口同时存在，只由主窗口发起检查，避免重复请求 GitHub。
  // 测试渠道由 Rust 侧直接返回“无更新”，这里不做渠道分支，保持原有节流语义。
  const updateCheckedAt = useRef(0);
  useEffect(() => {
    if (!desktop || IS_TRAY_VIEW) return;
    let disposed = false;
    let timer: number | undefined;

    function schedule(delay: number) {
      if (disposed) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = undefined;
        runCheck(true);
      }, delay);
    }

    function runCheck(force: boolean) {
      if (disposed) return;
      updateCheckedAt.current = Date.now();
      void checkForUpdate(force).then(
        (status) => {
          if (disposed) return;
          setUpdate(status);
          schedule(UPDATE_RECHECK_INTERVAL_MS);
        },
        () => {
          // 静默失败：设置页会显示具体原因。
          if (!disposed) schedule(UPDATE_RETRY_INTERVAL_MS);
        },
      );
    }

    function onFocus() {
      if (Date.now() - updateCheckedAt.current < UPDATE_FOCUS_MIN_INTERVAL_MS) return;
      // 强制刷新，避免复用长时间驻留期间的旧结果。
      runCheck(true);
    }

    runCheck(false);
    window.addEventListener('focus', onFocus);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      window.removeEventListener('focus', onFocus);
    };
  }, [desktop]);

  // 乱序响应或事件不应该把状态回退到旧修订号。
  const applyState = useCallback((next: AppState) => {
    setState((prev) => (next.revision >= prev.revision ? next : prev));
  }, []);

  useEffect(() => {
    if (!desktop) {
      setReady(true);
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      // 先订阅再取快照：两者之间发生的刷新事件不会丢，快照较旧时由修订号丢弃。
      try {
        const stop = await onAppStateChanged((next) => {
          if (!disposed) applyState(next);
        });
        // StrictMode 的首次清理可能早于 listen 解析完成，此时立即退订。
        if (disposed) stop();
        else unlisten = stop;
      } catch (err) {
        if (!disposed) setError(errorMessage(err));
      }

      try {
        const initial = await getAppState();
        if (disposed) return;
        applyState(initial);
        setReady(true);

        const legacy = readLegacyPreferences();
        if (legacy) {
          try {
            const migrated = await updatePreferences(legacy);
            clearLegacyPreferences();
            if (!disposed) applyState(migrated);
          } catch (err) {
            if (!disposed) setError(errorMessage(err));
          }
        }
      } catch (err) {
        if (!disposed) {
          setError(errorMessage(err));
          setReady(true);
        }
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop, applyState]);

  // 事件是主要同步通道；窗口重新显示时再读一次完整状态，
  // 避免隐藏期间（WebView 被节流或事件丢失）停留在旧额度或旧错误上。
  const resync = useCallback(async () => {
    if (!desktop) return;
    try {
      applyState(await getAppState());
    } catch (err) {
      setError(errorMessage(err));
    }
  }, [desktop, applyState]);

  const refresh = useCallback(
    async (accountId?: string) => {
      if (!desktop) return;
      try {
        await refreshUsage(accountId);
        setError(null);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [desktop],
  );

  const savePreferences = useCallback(
    async (preferences: Preferences) => {
      if (!desktop) {
        // 浏览器预览只在内存中生效，方便调整界面。
        setState((prev) => ({
          ...prev,
          revision: prev.revision + 1,
          preferences,
        }));
        return true;
      }
      try {
        const next = await updatePreferences(preferences);
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  // 排序是界面偏好：桌面端写入 Rust 状态并随事件同步到快速面板；
  // 浏览器预览没有后端，界面内自行保留即可。
  const saveOverviewOrder = useCallback(
    async (order: OverviewOrder) => {
      if (!desktop) return true;
      try {
        const next = await setOverviewOrderCommand(
          order.platforms,
          order.accounts,
        );
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  // 中转站排序同样只是界面偏好，写入 Rust 状态后随事件同步到快速面板。
  const saveRelayOrder = useCallback(
    async (ids: string[]) => {
      if (!desktop) return true;
      try {
        const next = await setRelayOrderCommand(ids);
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  const beginLogin = useCallback(
    async (accountId?: string, displayName?: string): Promise<LoginAttempt> => {
      if (!desktop) return { start: null, error: '浏览器预览无法发起桌面授权。' };
      try {
        const start = await beginAccountLogin(accountId, displayName);
        return { start, error: null };
      } catch (err) {
        return { start: null, error: errorMessage(err) };
      }
    },
    [desktop],
  );

  const beginBailianLogin = useCallback(
    async (accountId?: string, displayName?: string): Promise<LoginAttempt> => {
      if (!desktop) return { start: null, error: '浏览器预览无法发起桌面授权。' };
      try {
        const start = await beginBailianLoginCommand(accountId, displayName);
        return { start, error: null };
      } catch (err) {
        return { start: null, error: errorMessage(err) };
      }
    },
    [desktop],
  );

  const addApiKeyAccount = useCallback(
    async (platform: string, displayName: string | undefined, apiKey: string) => {
      if (!desktop) return '浏览器预览无法调用桌面接口。';
      try {
        const next = await addApiKeyAccountCommand(platform, displayName, apiKey);
        applyState(next);
        setError(null);
        return null;
      } catch (err) {
        const message = errorMessage(err);
        setError(message);
        return message;
      }
    },
    [desktop, applyState],
  );

  const replaceApiKey = useCallback(
    async (accountId: string, apiKey: string) => {
      if (!desktop) return '浏览器预览无法调用桌面接口。';
      try {
        const next = await replaceApiKeyCommand(accountId, apiKey);
        applyState(next);
        setError(null);
        return null;
      } catch (err) {
        const message = errorMessage(err);
        setError(message);
        return message;
      }
    },
    [desktop, applyState],
  );

  const addAntigravityAccount = useCallback(
    async (displayName?: string) => {
      if (!desktop) return '浏览器预览无法调用桌面接口。';
      try {
        const next = await addAntigravityAccountCommand(displayName);
        applyState(next);
        setError(null);
        return null;
      } catch (err) {
        const message = errorMessage(err);
        setError(message);
        return message;
      }
    },
    [desktop, applyState],
  );

  const cancelLogin = useCallback(
    async (loginId: string) => {
      if (!desktop) return;
      try {
        await cancelAccountLogin(loginId);
      } catch {
        // 取消是收尾动作：授权刚好结束时 Rust 会返回“未找到等待中的流程”，
        // 这不算用户可见的失败，也不再占用授权入口。
      }
    },
    [desktop],
  );

  const renameAccount = useCallback(
    async (accountId: string, displayName: string) => {
      if (!desktop) return false;
      try {
        const next = await renameAccountCommand(accountId, displayName);
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  const removeAccount = useCallback(
    async (accountId: string) => {
      if (!desktop) return false;
      try {
        const next = await removeAccountCommand(accountId);
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  const setAutoLaunch = useCallback(
    async (enabled: boolean) => {
      if (!desktop) return false;
      try {
        const next = await setAutoLaunchCommand(enabled);
        applyState(next);
        setError(null);
        return true;
      } catch (err) {
        setError(errorMessage(err));
        return false;
      }
    },
    [desktop, applyState],
  );

  const openExternal = useCallback(
    async (url: string) => {
      if (!desktop) return;
      try {
        await openExternalUrl(url);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [desktop],
  );

  const value = useMemo<AppStateContextValue>(
    () => ({
      state,
      appInfo,
      ready,
      desktop,
      error,
      stateFileNotice,
      update,
      refresh,
      resync,
      savePreferences,
      saveOverviewOrder,
      saveRelayOrder,
      beginLogin,
      beginBailianLogin,
      addApiKeyAccount,
      replaceApiKey,
      addAntigravityAccount,
      cancelLogin,
      renameAccount,
      removeAccount,
      setAutoLaunch,
      openExternal,
    }),
    [
      state,
      appInfo,
      ready,
      desktop,
      error,
      stateFileNotice,
      update,
      refresh,
      resync,
      savePreferences,
      saveOverviewOrder,
      saveRelayOrder,
      beginLogin,
      beginBailianLogin,
      addApiKeyAccount,
      replaceApiKey,
      addAntigravityAccount,
      cancelLogin,
      renameAccount,
      removeAccount,
      setAutoLaunch,
      openExternal,
    ],
  );

  return (
    <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>
  );
}

export function useAppState(): AppStateContextValue {
  const value = useContext(AppStateContext);
  if (!value) {
    throw new Error('useAppState must be used inside AppStateProvider');
  }
  return value;
}
