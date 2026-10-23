import { useEffect, useId, useRef, useState, type CSSProperties } from 'react';
import { Info } from '@phosphor-icons/react';
import { sendTestNotification, type Preferences } from '../lib/desktop';
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

type SaveState = 'idle' | 'saving' | 'saved' | 'error';

type TestState = 'idle' | 'sending' | 'sent' | 'error';

export default function SettingsPanel() {
  const { state, savePreferences, setAutoLaunch, desktop } = useAppState();
  const [prefs, setPrefs] = useState<Preferences>(state.preferences);
  const [theme, setTheme] = useState<ThemePref>(loadTheme);
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [testState, setTestState] = useState<TestState>('idle');
  const [autoLaunchBusy, setAutoLaunchBusy] = useState(false);
  const [autoLaunchError, setAutoLaunchError] = useState<string | null>(null);
  const dirty = useRef(false);
  const latestPrefs = useRef(prefs);
  latestPrefs.current = prefs;

  const refreshLabelId = useId();
  const themeLabelId = useId();
  const refreshIndex = Math.max(0, REFRESH_OPTIONS.indexOf(prefs.refreshMinutes));
  const themeIndex = Math.max(0, THEME_OPTIONS.indexOf(theme));
  const refreshFill = ((prefs.lowQuotaThreshold - 5) / 45) * 100;

  // 后端状态变化（如刷新事件）时同步；本地有未保存修改时不覆盖。
  useEffect(() => {
    if (!dirty.current) setPrefs(state.preferences);
  }, [state.preferences]);

  // 合并连续改动后写入后端，避免拖动阈值时产生大量并发请求。
  useEffect(() => {
    if (!dirty.current) return;
    setSaveState('saving');
    const timer = window.setTimeout(() => {
      dirty.current = false;
      void savePreferences(latestPrefs.current).then((ok) => {
        setSaveState(ok ? 'saved' : 'error');
        if (!ok) dirty.current = true;
      });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [prefs, savePreferences]);

  // 切换页面前把尚未写入的改动补发一次。
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
    void sendTestNotification().then(
      () => setTestState('sent'),
      () => setTestState('error'),
    );
  }

  return (
    <section>
      <div className={styles.draftBanner}>
        <Info size={15} className={styles.draftIcon} aria-hidden />
        <span className={styles.draftTag}>本机</span>
        <p className={styles.draftText}>
          设置保存到本机 Rust 状态；三类提醒走系统通知，开机自启写入系统登录项。
        </p>
        {saveState === 'saving' ? (
          <span className={styles.saveState}>保存中…</span>
        ) : saveState === 'saved' ? (
          <span className={styles.saveState}>已保存</span>
        ) : saveState === 'error' ? (
          <span className={styles.saveState} data-state="error">保存失败</span>
        ) : null}
      </div>

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
              提醒通过 macOS 系统通知发送，桌面端没有应用内授权弹窗。若未显示，请在“系统设置 →
              通知”中允许 Token Refill（开发模式下来源可能显示为 Terminal）。
            </span>
            {testState === 'sent' ? (
              <span className={styles.settingHint} role="status">
                已请求发送；未出现请检查系统通知设置。
              </span>
            ) : null}
            {testState === 'error' ? (
              <span className={styles.settingHint} role="status" data-danger="true">
                发送失败，请稍后重试。
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

      <h2 id="settings-system" className={styles.groupTitle}>系统</h2>
      <section className={`${styles.card} ${styles.listCard}`} aria-labelledby="settings-system">
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
    </section>
  );
}
