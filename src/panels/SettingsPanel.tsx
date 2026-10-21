import { useEffect, useState } from 'react';
import { loadPrefs, savePrefs, REFRESH_OPTIONS, type Prefs } from '../lib/prefs';
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
  return (
    <div className={styles.settingRow}>
      <div className={styles.settingText}>
        <span className={styles.settingLabel}>{label}</span>
        {hint ? <p className={styles.settingHint}>{hint}</p> : null}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        className={styles.switch}
        data-on={checked || undefined}
        onClick={() => onChange(!checked)}
      >
        <span className={styles.switchKnob} />
      </button>
    </div>
  );
}

export default function SettingsPanel() {
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);

  useEffect(() => {
    savePrefs(prefs);
  }, [prefs]);

  function set<K extends keyof Prefs>(key: K, value: Prefs[K]) {
    setPrefs((prev) => ({ ...prev, [key]: value }));
  }

  return (
    <section>
      <p className={styles.notice}>
        当前为界面草稿：设置仅保存在本机浏览器存储，尚未接入 Rust 后端与系统权限，不会实际执行调度、通知或自启。
      </p>

      <h2 className={styles.groupTitle}>刷新</h2>
      <article className={`${styles.card} ${styles.listCard}`}>
        <div className={styles.stackRow}>
          <div className={styles.settingText}>
            <span className={styles.settingLabel}>自动刷新间隔</span>
            <p className={styles.settingHint}>后台按该间隔（分钟）查询全部账户，并服从平台限流。</p>
          </div>
          <div className={`${styles.pills} ${styles.pillsFull}`}>
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
      </article>

      <h2 className={styles.groupTitle}>提醒</h2>
      <article className={`${styles.card} ${styles.listCard}`}>
        <Toggle
          checked={prefs.lowQuotaEnabled}
          onChange={(next) => set('lowQuotaEnabled', next)}
          label="低额度提醒"
          hint="仅在能可靠获得或计算剩余比例时适用，总览中的额度条会同步标记。"
        />
        <div className={styles.stackRow}>
          <div className={styles.rangeHead}>
            <span className={styles.settingLabel}>提醒阈值</span>
            <span className={styles.rangeValue}>剩余低于 {prefs.lowQuotaThreshold}% 时提醒</span>
          </div>
          <input
            type="range"
            className={styles.range}
            min={5}
            max={50}
            step={5}
            value={prefs.lowQuotaThreshold}
            disabled={!prefs.lowQuotaEnabled}
            aria-label="低额度提醒阈值（剩余百分比）"
            onChange={(event) => set('lowQuotaThreshold', Number(event.target.value))}
          />
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
      </article>
      <p className={`${styles.settingHint} ${styles.groupNote}`}>
        系统尚未接入通知权限，以上提醒暂不会实际发送。
      </p>

      <h2 className={styles.groupTitle}>系统</h2>
      <article className={`${styles.card} ${styles.listCard}`}>
        <Toggle
          checked={prefs.autoLaunchEnabled}
          onChange={(next) => set('autoLaunchEnabled', next)}
          label="开机自启"
          hint="默认关闭；接入后以系统实际状态为准，设置失败会如实反馈。"
        />
      </article>
    </section>
  );
}
