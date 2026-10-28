import { useEffect, useId, useRef, useState, type CSSProperties } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
  checkForUpdate,
  downloadedUpdateVersion,
  downloadUpdate,
  emitThemeChanged,
  installUpdate,
  onUpdateDownloadProgress,
  openExternalUrl,
  sendTestNotification,
  type Preferences,
  type ProxyMode,
  type UpdateDownloadProgress,
  type UpdateStatus,
} from '../lib/desktop';
import {
  loadTheme,
  saveTheme,
  REFRESH_OPTIONS,
  THEME_LABELS,
  THEME_OPTIONS,
  type ThemePref,
} from '../lib/prefs';
import { useAppState } from '../lib/store';
import { applyTheme } from '../lib/theme';
import Toast from '../components/Toast';
import styles from '../App.module.css';

function Toggle({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  hint?: string;
  disabled?: boolean;
}) {
  const id = useId();
  const labelId = `${id}-label`;
  const hintId = `${id}-hint`;

  return (
    <label className={styles.settingRow}>
      <span className={styles.settingText}>
        <span id={labelId} className={styles.settingLabel}>{label}</span>
        {hint ? (
          <span id={hintId} className={styles.settingHint}>{hint}</span>
        ) : null}
      </span>
      <span className={styles.switchWrap}>
        <input
          type="checkbox"
          role="switch"
          className={styles.switchInput}
          checked={checked}
          onChange={(event) => onChange(event.target.checked)}
          disabled={disabled}
          aria-labelledby={labelId}
          aria-describedby={hint ? hintId : undefined}
        />
        <span className={styles.switchTrack} aria-hidden="true">
          <span className={styles.switchKnob} />
        </span>
      </span>
    </label>
  );
}

const SAVE_ERROR_MESSAGE = '设置保存失败，请重试。';

function formatMegabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const PROXY_OPTIONS: { value: ProxyMode; label: string }[] = [
  { value: 'system', label: '跟随系统' },
  { value: 'none', label: '不走代理' },
  { value: 'manual', label: '指定代理' },
];

type TestState = 'idle' | 'sending' | 'sent' | 'error';

type UpdateState =
  | 'idle'
  | 'checking'
  | 'done'
  | 'downloading'
  | 'downloaded'
  | 'installing'
  | 'error';

export default function SettingsPanel() {
  const { state, appInfo, savePreferences, setAutoLaunch, desktop } = useAppState();
  const isTestBuild = appInfo?.channel === 'test';
  const appName = appInfo?.name ?? 'Token Refill';
  const [prefs, setPrefs] = useState<Preferences>(state.preferences);
  const [theme, setTheme] = useState<ThemePref>(loadTheme);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testState, setTestState] = useState<TestState>('idle');
  const [testError, setTestError] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateStatus | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>('idle');
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] =
    useState<UpdateDownloadProgress | null>(null);
  const [openingRelease, setOpeningRelease] = useState(false);
  const [autoLaunchBusy, setAutoLaunchBusy] = useState(false);
  const [autoLaunchError, setAutoLaunchError] = useState<string | null>(null);
  const dirty = useRef(false);
  const latestPrefs = useRef(prefs);
  latestPrefs.current = prefs;

  const refreshLabelId = useId();
  const themeLabelId = useId();
  const proxyLabelId = useId();
  const refreshIndex = Math.max(0, REFRESH_OPTIONS.indexOf(prefs.refreshMinutes));
  const themeIndex = Math.max(0, THEME_OPTIONS.indexOf(theme));
  const proxyIndex = Math.max(
    0,
    PROXY_OPTIONS.findIndex((option) => option.value === prefs.proxyMode),
  );
  const refreshFill = ((prefs.lowQuotaThreshold - 5) / 45) * 100;

  // 后端状态变化（如刷新事件）时同步；本地有未保存修改时不覆盖。
  useEffect(() => {
    if (!dirty.current) setPrefs(state.preferences);
  }, [state.preferences]);

  // 合并连续改动后写入后端，避免拖动阈值时产生大量并发请求。
  // 保存成功不打扰用户；失败时才弹一次 toast。
  useEffect(() => {
    if (!dirty.current) return;
    const timer = window.setTimeout(() => {
      dirty.current = false;
      void savePreferences(latestPrefs.current).then((ok) => {
        if (ok) return;
        dirty.current = true;
        setSaveError(SAVE_ERROR_MESSAGE);
      });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [prefs, savePreferences]);

  // 切换页面前把尚未写入的改动补发一次；toast 的计时由组件自己清理。
  useEffect(
    () => () => {
      if (dirty.current) void savePreferences(latestPrefs.current);
    },
    [savePreferences],
  );

  function update(patch: Partial<Preferences>) {
    dirty.current = true;
    setPrefs((prev) => ({ ...prev, ...patch }));
  }

  function changeTheme(next: ThemePref) {
    setTheme(next);
    saveTheme(next);
    applyTheme(next);
    if (desktop) void emitThemeChanged(next);
  }

  async function changeAutoLaunch(next: boolean) {
    if (!desktop || autoLaunchBusy) return;
    setAutoLaunchBusy(true);
    setAutoLaunchError(null);
    const ok = await setAutoLaunch(next);
    setAutoLaunchBusy(false);
    if (!ok) {
      setAutoLaunchError('系统未接受开机自启设置，请检查登录项权限。');
      return;
    }
    // 立即反映系统实际状态；这不是普通偏好改动，不标记 dirty。
    setPrefs((prev) => ({ ...prev, autoLaunchEnabled: next }));
  }

  function testNotification() {
    if (!desktop || testState === 'sending') return;
    setTestState('sending');
    setTestError(null);
    void sendTestNotification().then(
      () => setTestState('sent'),
      (error) => {
        setTestState('error');
        // Rust 侧会返回可直接展示的原因（例如系统通知权限被拒绝）。
        setTestError(typeof error === 'string' ? error : null);
      },
    );
  }

  // 打开设置页时自动查一次；desktop.ts 有 module 级缓存，
  // 重复进入设置页不会反复请求 GitHub，失败后下次仍可重试。
  // 测试渠道不参与更新链路，等 appInfo 到达后直接跳过。
  useEffect(() => {
    if (!desktop || !appInfo || isTestBuild) return;
    void runUpdateCheck(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [desktop, appInfo, isTestBuild]);

  // 下载进度由 Rust 推送；组件卸载时取消订阅。
  useEffect(() => {
    if (!desktop) return;
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void onUpdateDownloadProgress((progress) =>
      setDownloadProgress(progress),
    ).then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [desktop]);

  async function runUpdateCheck(force: boolean) {
    if (!desktop || isTestBuild || updateState === 'checking') return;
    setUpdateState('checking');
    setUpdateError(null);
    try {
      const info = await checkForUpdate(force);
      setUpdateInfo(info);
      // 之前下载过的更新在重新打开设置页后仍可安装；版本不一致时不再展示。
      const pending = await downloadedUpdateVersion().catch(() => null);
      if (pending && info.updateAvailable && info.latestVersion === pending) {
        setUpdateState('downloaded');
      } else {
        setDownloadProgress(null);
        setUpdateState('done');
      }
    } catch (err) {
      setUpdateError(
        typeof err === 'string' ? err : '检查更新失败，请稍后重试。',
      );
      setUpdateState('error');
    }
  }

  async function startDownload() {
    if (!desktop || updateState === 'downloading') return;
    setUpdateError(null);
    setDownloadProgress(null);
    setUpdateState('downloading');
    try {
      await downloadUpdate();
      setUpdateState('downloaded');
    } catch (err) {
      setUpdateError(typeof err === 'string' ? err : '下载更新失败，请重试。');
      setUpdateState('error');
    }
  }

  async function installDownloaded() {
    if (!desktop || updateState === 'installing') return;
    setUpdateError(null);
    setUpdateState('installing');
    try {
      await installUpdate();
      // 正常情况下应用会退出并重启，不会走到这里。
    } catch (err) {
      setUpdateError(
        typeof err === 'string' ? err : '安装更新失败，请前往发布页手动下载。',
      );
      setUpdateState('error');
    }
  }

  async function openReleasePage() {
    const url = updateInfo?.releaseUrl;
    if (!url || openingRelease) return;
    setOpeningRelease(true);
    try {
      await openExternalUrl(url);
    } catch (err) {
      setUpdateError(typeof err === 'string' ? err : '无法打开下载页。');
    } finally {
      setOpeningRelease(false);
    }
  }

  const progressPercent =
    downloadProgress?.total && downloadProgress.total > 0
      ? Math.min(
          100,
          Math.round(
            (downloadProgress.downloaded / downloadProgress.total) * 100,
          ),
        )
      : null;
  const progressDetail = downloadProgress
    ? progressPercent !== null
      ? `${progressPercent}%`
      : formatMegabytes(downloadProgress.downloaded)
    : null;
  const busyWithUpdate =
    updateState === 'downloading' ||
    updateState === 'downloaded' ||
    updateState === 'installing';
  const barPercent =
    updateState === 'downloaded' || updateState === 'installing'
      ? 100
      : (progressPercent ?? 0);

  const updateStatusText = isTestBuild
    ? '测试版不检查更新；新测试包请从 GitHub Actions 的 build-test 产物下载。'
    : updateState === 'checking'
      ? '正在检查 GitHub Releases…'
      : updateState === 'downloading'
        ? `正在下载新版本${progressDetail ? `（${progressDetail}）` : '…'}`
        : updateState === 'downloaded'
          ? `新版本 v${updateInfo?.latestVersion ?? ''} 已下载，重启应用后完成安装。`
          : updateState === 'installing'
            ? '正在安装更新，应用即将重启…'
            : updateState === 'error'
              ? (updateError ?? '检查更新失败，请稍后重试。')
              : updateInfo?.updateAvailable
                ? updateInfo.selfUpdateSupported
                  ? `发现新版本 v${updateInfo.latestVersion}${
                      updateInfo.publishedAt
                        ? `（${updateInfo.publishedAt.slice(0, 10)} 发布）`
                        : ''
                    }，可下载后重启完成替换。`
                  : `发现新版本 v${updateInfo.latestVersion}。当前是免安装版本，请前往发布页下载后手动替换。`
                : updateInfo
                  ? `已是最新版本（v${updateInfo.currentVersion}）。`
                  : '检查最新正式版本；下载与安装都需要你确认后才会开始。';

  return (
    <section>
      <h2 id="settings-general" className={styles.groupTitle}>通用</h2>
      <section
        className={`${styles.card} ${styles.listCard}`}
        aria-labelledby="settings-general"
      >
        <div className={styles.settingRowStatic}>
          <div className={styles.settingText}>
            <span id={themeLabelId} className={styles.settingLabel}>外观</span>
            <span className={styles.settingHint}>
              选择后立即应用到整个窗口；跟随系统时随系统外观自动切换。
            </span>
          </div>
          <div
            className={styles.pills}
            role="radiogroup"
            aria-labelledby={themeLabelId}
            style={{
              '--pill-index': themeIndex,
              '--pill-count': THEME_OPTIONS.length,
            } as CSSProperties}
          >
            <span className={styles.pillThumb} aria-hidden="true" />
            {THEME_OPTIONS.map((value) => (
              <label className={styles.pill} key={value}>
                <input
                  type="radio"
                  name="theme"
                  value={value}
                  checked={theme === value}
                  onChange={() => changeTheme(value)}
                  aria-label={THEME_LABELS[value]}
                />
                <span className={styles.pillText}>{THEME_LABELS[value]}</span>
              </label>
            ))}
          </div>
        </div>
        <Toggle
          checked={prefs.autoLaunchEnabled}
          onChange={(next) => void changeAutoLaunch(next)}
          label="开机自启"
          hint={
            !desktop
              ? '浏览器预览无法修改系统登录项。'
              : autoLaunchBusy
                ? '正在写入系统登录项…'
                : '默认关闭；写入系统登录项，设置失败会如实反馈。'
          }
          disabled={!desktop || autoLaunchBusy}
        />
        {autoLaunchError ? (
          <p
            className={`${styles.settingHint} ${styles.groupNote}`}
            data-danger="true"
            role="status"
          >
            {autoLaunchError}
          </p>
        ) : null}
      </section>

      <h2 id="settings-refresh" className={styles.groupTitle}>刷新</h2>
      <section className={`${styles.card} ${styles.listCard}`} aria-labelledby="settings-refresh">
        <div className={styles.settingRowStatic}>
          <div className={styles.settingText}>
            <span id={refreshLabelId} className={styles.settingLabel}>
              自动刷新间隔（分钟）
            </span>
            <span className={styles.settingHint}>
              后台按所选间隔查询全部账户，并服从平台限流。
            </span>
          </div>
          <div
            className={styles.pills}
            role="radiogroup"
            aria-labelledby={refreshLabelId}
            style={{
              '--pill-index': refreshIndex,
              '--pill-count': REFRESH_OPTIONS.length,
            } as CSSProperties}
          >
            <span className={styles.pillThumb} aria-hidden="true" />
            {REFRESH_OPTIONS.map((mins) => (
              <label className={styles.pill} key={mins}>
                <input
                  type="radio"
                  name="refreshMinutes"
                  value={mins}
                  checked={prefs.refreshMinutes === mins}
                  onChange={() => update({ refreshMinutes: mins })}
                  aria-label={`${mins} 分钟`}
                />
                <span className={styles.pillText}>{mins}</span>
              </label>
            ))}
          </div>
        </div>
      </section>

      <h2 id="settings-reminders" className={styles.groupTitle}>提醒</h2>
      <section
        className={`${styles.card} ${styles.listCard}`}
        aria-labelledby="settings-reminders"
      >
        <Toggle
          checked={prefs.lowQuotaEnabled}
          onChange={(next) => update({ lowQuotaEnabled: next })}
          label="低额度提醒"
          hint="剩余比例低于阈值时通过系统通知提醒，同一额度周期只提醒一次；总览中的剩余额度环会同步标记。"
        />
        <div
          className={styles.stackRow}
          data-disabled={prefs.lowQuotaEnabled ? undefined : true}
        >
          <div className={styles.rangeHead}>
            <span className={styles.settingLabel}>提醒阈值</span>
            <span className={styles.rangeValue}>
              低于 {prefs.lowQuotaThreshold}% 时提醒
            </span>
          </div>
          <input
            type="range"
            className={styles.range}
            style={{ '--range-fill': `${refreshFill}%` } as CSSProperties}
            min={5}
            max={50}
            step={5}
            value={prefs.lowQuotaThreshold}
            disabled={!prefs.lowQuotaEnabled}
            aria-label="低额度提醒阈值（剩余百分比）"
            onChange={(event) => update({ lowQuotaThreshold: Number(event.target.value) })}
          />
          <div className={styles.rangeScale} aria-hidden="true">
            <span>5%</span>
            <span>50%</span>
          </div>
        </div>
        <Toggle
          checked={prefs.authExpiredEnabled}
          onChange={(next) => update({ authExpiredEnabled: next })}
          label="授权失效提醒"
          hint="真实授权失效时提示重新授权；普通网络故障不会按失效处理。"
        />
        <Toggle
          checked={prefs.resetNoticeEnabled}
          onChange={(next) => update({ resetNoticeEnabled: next })}
          label="重置或恢复提醒"
          hint="默认关闭；仅在查询确认新周期后提示，不根据本地倒计时宣称恢复。"
        />
        <div className={styles.settingRowStatic}>
          <div className={styles.settingText}>
            <span className={styles.settingLabel}>系统通知</span>
            <span className={styles.settingHint}>
              提醒通过系统通知发送。首次启动时 macOS 会请求通知权限；若未显示，请在“系统设置 →
              通知”中允许 {appName}。
            </span>
            {testState === 'sent' ? (
              <span className={styles.settingHint} role="status">
                已发送测试通知。
              </span>
            ) : null}
            {testState === 'error' ? (
              <span className={styles.settingHint} role="status" data-danger="true">
                {testError ?? '发送失败，请稍后重试。'}
              </span>
            ) : null}
          </div>
          <button
            type="button"
            className={styles.testButton}
            onClick={testNotification}
            disabled={!desktop || testState === 'sending'}
            title={desktop ? '发送一条测试通知' : '浏览器预览无法调用 Rust'}
          >
            {testState === 'sending' ? '发送中…' : '发送测试通知'}
          </button>
        </div>
      </section>
      <p className={`${styles.settingHint} ${styles.groupNote}`}>
        三类提醒均已接入系统通知。
      </p>

      <h2 id="settings-proxy" className={styles.groupTitle}>网络代理</h2>
      <section className={`${styles.card} ${styles.listCard}`} aria-labelledby="settings-proxy">
        <div className={styles.settingRowStatic}>
          <div className={styles.settingText}>
            <span id={proxyLabelId} className={styles.settingLabel}>代理方式</span>
            <span className={styles.settingHint}>
              统一作用于额度查询、版本检查与外置 CLI。跟随系统会读取系统代理设置；
              指定代理仅支持 HTTP/HTTPS，不支持 PAC。
            </span>
          </div>
          <div
            className={styles.pills}
            role="radiogroup"
            aria-labelledby={proxyLabelId}
            style={{
              '--pill-index': proxyIndex,
              '--pill-count': PROXY_OPTIONS.length,
            } as CSSProperties}
          >
            <span className={styles.pillThumb} aria-hidden="true" />
            {PROXY_OPTIONS.map((option) => (
              <label className={styles.pill} key={option.value}>
                <input
                  type="radio"
                  name="proxyMode"
                  value={option.value}
                  checked={prefs.proxyMode === option.value}
                  onChange={() => update({ proxyMode: option.value })}
                  aria-label={option.label}
                />
                <span className={styles.pillText}>{option.label}</span>
              </label>
            ))}
          </div>
        </div>
        {prefs.proxyMode === 'manual' ? (
          <div className={styles.stackRow}>
            <span className={styles.settingLabel}>代理地址</span>
            <input
              className={styles.textInput}
              value={prefs.proxyUrl ?? ''}
              onChange={(event) => update({ proxyUrl: event.target.value })}
              placeholder="http://127.0.0.1:7890"
              spellCheck={false}
              autoComplete="off"
              aria-label="代理地址"
            />
            <span className={styles.settingHint}>
              只支持 http:// 或 https://，不能包含用户名、密码或路径。
            </span>
          </div>
        ) : null}
      </section>

      <h2 id="settings-version" className={styles.groupTitle}>版本</h2>
      <section className={`${styles.card} ${styles.listCard}`} aria-labelledby="settings-version">
        <div className={styles.settingRowStatic}>
          <div className={styles.settingText}>
            <span className={styles.settingLabel}>
              当前版本 v{appInfo?.version ?? '—'}
              {isTestBuild ? '（测试版）' : ''}
            </span>
            <span
              className={styles.settingHint}
              role="status"
              data-danger={updateState === 'error' ? 'true' : undefined}
            >
              {updateStatusText}
            </span>
          </div>
          {isTestBuild ? null : (
          <div className={styles.buttonRow}>
            {updateInfo?.updateAvailable && updateInfo.selfUpdateSupported ? (
              updateState === 'downloaded' || updateState === 'installing' ? (
                <button
                  type="button"
                  className={styles.testButton}
                  data-primary
                  onClick={() => void installDownloaded()}
                  disabled={updateState === 'installing'}
                >
                  {updateState === 'installing' ? '安装中…' : '重启并安装'}
                </button>
              ) : updateState === 'downloading' ? (
                <button
                  type="button"
                  className={styles.testButton}
                  data-primary
                  disabled
                >
                  {progressDetail ? `下载中 ${progressDetail}` : '下载中…'}
                </button>
              ) : (
                <button
                  type="button"
                  className={styles.testButton}
                  data-primary
                  onClick={() => void startDownload()}
                  disabled={!desktop || updateState === 'checking'}
                >
                  下载更新
                </button>
              )
            ) : null}
            {updateInfo?.updateAvailable && updateInfo.releaseUrl ? (
              <button
                type="button"
                className={styles.testButton}
                onClick={() => void openReleasePage()}
                disabled={openingRelease}
              >
                {openingRelease ? '打开中…' : '前往发布页'}
              </button>
            ) : null}
            <button
              type="button"
              className={styles.testButton}
              onClick={() => void runUpdateCheck(true)}
              disabled={
                !desktop || updateState === 'checking' || busyWithUpdate
              }
              title={desktop ? '向 GitHub Releases 查询最新正式版本' : '浏览器预览无法调用 Rust'}
            >
              {updateState === 'checking' ? '检查中…' : '检查更新'}
            </button>
          </div>
          )}
        </div>
        {updateState === 'downloading' ||
        updateState === 'downloaded' ||
        updateState === 'installing' ? (
          <div
            className={styles.updateProgress}
            role="progressbar"
            aria-label="更新下载进度"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={progressPercent ?? undefined}
          >
            <span style={{ width: `${barPercent}%` }} />
          </div>
        ) : null}
      </section>
      {isTestBuild ? null : (
        <p className={`${styles.settingHint} ${styles.groupNote}`}>
          下载与安装都需要手动确认；更新包通过签名校验后才会安装，安装期间应用会退出并重启。
          免安装版 Windows exe 无法原地替换，请从发布页手动下载。
        </p>
      )}

      {saveError ? (
        <Toast message={saveError} onDismiss={() => setSaveError(null)} />
      ) : null}
    </section>
  );
}
