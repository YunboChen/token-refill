import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { isTauri } from '@tauri-apps/api/core';
import {
  addAntigravityAccount as addAntigravityAccountCommand,
  addApiKeyAccount as addApiKeyAccountCommand,
  beginAccountLogin,
  beginBailianLogin as beginBailianLoginCommand,
  cancelAccountLogin,
  checkForUpdate,
  getAppState,
  onAppStateChanged,
  openExternalUrl,
  refreshUsage,
  removeAccount as removeAccountCommand,
  renameAccount as renameAccountCommand,
  replaceApiKey as replaceApiKeyCommand,
  setAutoLaunch as setAutoLaunchCommand,
  updatePreferences,
  type AppState,
  type LoginStart,
  type Preferences,
  type UpdateStatus,
} from './desktop';
import { clearLegacyPreferences, defaultAppState, readLegacyPreferences } from './prefs';

/** 发起授权的结果：失败原因随结果返回，由发起方在弹窗内展示，不进入页面级提示。 */
export interface LoginAttempt {
  start: LoginStart | null;
  error: string | null;
}

interface AppStateContextValue {
  state: AppState;
  ready: boolean;
  desktop: boolean;
  error: string | null;
  update: UpdateStatus | null;
  refresh: (accountId?: string) => Promise<void>;
  savePreferences: (preferences: Preferences) => Promise<boolean>;
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
  const [ready, setReady] = useState(!desktop);
  const [error, setError] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);

  // 启动时静默检查一次版本；失败（离线、限流）不打扰用户，设置页仍可手动重试。
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    void checkForUpdate().then(
      (status) => {
        if (!disposed) setUpdate(status);
      },
      () => {
        // 静默失败：设置页会显示具体原因。
      },
    );
    return () => {
      disposed = true;
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
      ready,
      desktop,
      error,
      update,
      refresh,
      savePreferences,
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
      ready,
      desktop,
      error,
      update,
      refresh,
      savePreferences,
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
