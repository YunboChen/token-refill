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
  getAppState,
  onAppStateChanged,
  refreshUsage,
  updatePreferences,
  type AppState,
  type Preferences,
} from './desktop';
import { clearLegacyPreferences, defaultAppState, readLegacyPreferences } from './prefs';

interface AppStateContextValue {
  state: AppState;
  ready: boolean;
  desktop: boolean;
  error: string | null;
  refresh: (accountId?: string) => Promise<void>;
  savePreferences: (preferences: Preferences) => Promise<boolean>;
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

  const value = useMemo<AppStateContextValue>(
    () => ({
      state,
      ready,
      desktop,
      error,
      refresh,
      savePreferences,
    }),
    [state, ready, desktop, error, refresh, savePreferences],
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
