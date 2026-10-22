import { useEffect, useId, useState, type CSSProperties } from 'react';
import { Info } from '@phosphor-icons/react';
import {
  loadPrefs,
  savePrefs,
  REFRESH_OPTIONS,
  THEME_LABELS,
  THEME_OPTIONS,
  type Prefs,
} from '../lib/prefs';
import { applyTheme } from '../lib/theme';
import styles from '../App.module.css';

function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  hint?: string;
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

export default function SettingsPanel() {
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);
  const refreshLabelId = useId();
  const themeLabelId = useId();
  const refreshIndex = Math.max(0, REFRESH_OPTIONS.indexOf(prefs.refreshMinutes));
  const themeIndex = Math.max(0, THEME_OPTIONS.indexOf(prefs.theme));
  const refreshFill = ((prefs.lowQuotaThreshold - 5) / 45) * 100;

  useEffect(() => {
    savePrefs(prefs);
    applyTheme(prefs.theme);
  }, [prefs]);

  function set<K extends keyof Prefs>(key: K, value: Prefs[K]) {
    setPrefs((prev) => ({ ...prev, [key]: value }));
  }

  return (
    <section>
      <div className={styles.draftBanner}>
        <Info size={15} className={styles.draftIcon} aria-hidden />
        <span className={styles.draftTag}>草稿</span>
        <p className={styles.draftText}>
          设置暂存于本机浏览器存储；调度、通知与开机自启尚未接入 Rust 后端与系统权限，暂不生效。
        </p>
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
                  onChange={() => set('refreshMinutes', mins)}
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
          onChange={(next) => set('lowQuotaEnabled', next)}
          label="低额度提醒"
          hint="仅在能可靠获得或计算剩余比例时适用，总览中的额度条会同步标记。"
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
            onChange={(event) => set('lowQuotaThreshold', Number(event.target.value))}
          />
          <div className={styles.rangeScale} aria-hidden="true">
            <span>5%</span>
            <span>50%</span>
          </div>
        </div>
        <Toggle
          checked={prefs.authExpiredEnabled}
          onChange={(next) => set('authExpiredEnabled', next)}
          label="授权失效提醒"
          hint="真实授权失效时提示重新授权；普通网络故障不会按失效处理。"
        />
        <Toggle
          checked={prefs.resetNoticeEnabled}
          onChange={(next) => set('resetNoticeEnabled', next)}
          label="重置或恢复提醒"
          hint="默认关闭；仅在查询确认新周期后提示，不根据本地倒计时宣称恢复。"
        />
      </section>
      <p className={`${styles.settingHint} ${styles.groupNote}`}>
        系统尚未接入通知权限，以上提醒暂不会实际发送。
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
                  checked={prefs.theme === value}
                  onChange={() => set('theme', value)}
                  aria-label={THEME_LABELS[value]}
                />
                <span className={styles.pillText}>{THEME_LABELS[value]}</span>
              </label>
            ))}
          </div>
        </div>
        <Toggle
          checked={prefs.autoLaunchEnabled}
          onChange={(next) => set('autoLaunchEnabled', next)}
          label="开机自启"
          hint="默认关闭；接入后以系统实际状态为准，设置失败会如实反馈。"
        />
      </section>
    </section>
  );
}
