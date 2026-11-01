import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import {
  Check,
  PlugsIcon,
  WarningCircle,
} from '@phosphor-icons/react';
import ChevronIcon from '../components/ChevronIcon';
import PlatformLogo from '../components/PlatformLogo';
import {
  floatingBallHidePanel,
  floatingBallPanelHover,
  floatingBallResize,
  floatingBallShowMenu,
  getFloatingBallPopoverMode,
  onFloatingBallPopoverMode,
  selectFloatingBallPlan,
} from '../lib/desktop';
import {
  optionFromSelection,
  planOptions,
  productRows,
  relayRows,
  resolveSelection,
  type FloatPlanOption,
  type FloatWindowRow,
} from '../lib/float';
import { planTierLabel } from '../lib/format';
import { useRelay } from '../lib/relay';
import { useAppState } from '../lib/store';
import styles from './FloatingBall.module.css';

type PopoverMode = 'hover' | 'menu';

function percentText(option: FloatPlanOption): string {
  if (option.percent === null) return option.amount ?? '—';
  return `${Math.round(option.percent)}%`;
}

function OptionMark({ option }: { option: FloatPlanOption }) {
  return (
    <span className={styles.planLogo} aria-hidden="true">
      {option.kind === 'relay' ? (
        <PlugsIcon size={16} weight="fill" />
      ) : (
        <PlatformLogo platform={option.platform} fallback={option.title.slice(0, 1)} />
      )}
    </span>
  );
}

function QuotaRows({ rows }: { rows: FloatWindowRow[] }) {
  if (rows.length === 0) return null;
  return (
    <div className={styles.rows}>
      {rows.map((row) => (
        <div className={styles.row} key={row.key}>
          <div className={styles.rowHead}>
            <span className={styles.rowLabel} title={row.group ?? undefined}>
              {row.group ? `${row.group} · ` : ''}
              {row.label}
            </span>
            <span className={styles.rowValue} data-level={row.level}>
              {row.percent !== null ? `${row.percent}%` : '—'}
            </span>
          </div>
          {row.percent !== null ? (
            <span className={styles.bar} aria-hidden="true">
              <span
                className={styles.barFill}
                data-level={row.level}
                style={{ width: `${Math.max(0, Math.min(100, row.percent))}%` }}
              />
            </span>
          ) : null}
          {row.meta.length > 0 ? (
            <span className={styles.rowMeta}>{row.meta.join(' · ')}</span>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function HoverCard({ onSwitch }: { onSwitch: () => void }) {
  const { state } = useAppState();
  const { accounts: relays } = useRelay();
  const selection = resolveSelection(state, relays);
  const option = optionFromSelection(selection);
  const threshold = state.preferences.lowQuotaThreshold;
  const lowEnabled = state.preferences.lowQuotaEnabled;
  const rows = selection?.kind === 'product'
    ? productRows(selection.product, threshold, lowEnabled)
    : selection?.kind === 'relay'
      ? relayRows(selection.account, threshold, lowEnabled)
      : [];

  if (!option || !selection) {
    return (
      <div className={styles.hover}>
        <span className={styles.eyebrow}>当前套餐</span>
        <p className={styles.emptyText}>尚未选择要显示的套餐。</p>
        <button type="button" className={styles.footerButton} onClick={onSwitch}>
          选择套餐
          <ChevronIcon direction="right" />
        </button>
      </div>
    );
  }

  const identity = selection.kind === 'product'
    ? selection.account.identityHint
    : null;
  const tier = selection.kind === 'product' && option.tier
    ? planTierLabel(option.tier)
    : null;

  return (
    <div className={styles.hover}>
      <div className={styles.hoverHead}>
        <span className={styles.eyebrow}>正在使用</span>
        <div className={styles.hoverTitleRow}>
          <OptionMark option={option} />
          <span className={styles.hoverTitle} title={option.title}>
            {option.title}
          </span>
          {tier ? <span className={styles.tierBadge}>{tier}</span> : null}
        </div>
        <span className={styles.hoverAccount} title={identity ?? undefined}>
          {option.subtitle}
          {identity ? ` · ${identity}` : ''}
        </span>
      </div>

      <QuotaRows rows={rows} />

      {option.error ? (
        <p className={styles.hoverError} role="status">
          <WarningCircle size={12} aria-hidden />
          <span>{option.error}</span>
        </p>
      ) : null}

      <button type="button" className={styles.footerButton} onClick={onSwitch}>
        切换套餐
        <ChevronIcon direction="right" />
      </button>
    </div>
  );
}

function MenuCard() {
  const { state } = useAppState();
  const { accounts: relays } = useRelay();
  const options = planOptions(state, relays);
  const selected = state.floatingBall.selected;
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const choose = useCallback(
    async (key: string) => {
      if (busyKey) return;
      setBusyKey(key);
      setError(null);
      try {
        await selectFloatingBallPlan(key);
      } catch (err) {
        setError(typeof err === 'string' ? err : '切换套餐失败，请重试。');
      } finally {
        setBusyKey(null);
      }
    },
    [busyKey],
  );

  return (
    <div className={styles.menu}>
      <div className={styles.menuHead}>
        <span className={styles.eyebrow}>当前套餐</span>
        <span className={styles.menuHint}>点击切换悬浮球显示</span>
      </div>
      {options.length === 0 ? (
        <p className={styles.emptyText}>还没有可显示的套餐或中转站账户。</p>
      ) : (
        <div className={styles.menuList} role="listbox" aria-label="选择当前套餐">
          {options.map((option) => {
            const isSelected = option.key === selected;
            return (
              <button
                type="button"
                role="option"
                aria-selected={isSelected}
                key={option.key}
                className={styles.item}
                data-selected={isSelected || undefined}
                disabled={busyKey !== null}
                onClick={() => void choose(option.key)}
                title={option.error ?? option.title}
              >
                <span className={styles.itemCheck} aria-hidden="true">
                  {isSelected ? <Check size={14} weight="bold" /> : null}
                </span>
                <OptionMark option={option} />
                <span className={styles.itemText}>
                  <span className={styles.itemTitle}>{option.title}</span>
                  <span className={styles.itemSub}>
                    {option.subtitle}
                    {option.kind === 'product' && option.tier
                      ? ` · ${planTierLabel(option.tier)}`
                      : ''}
                  </span>
                </span>
                <span className={styles.itemValue}>
                  <span className={styles.itemPercent}>
                    {busyKey === option.key ? '…' : percentText(option)}
                  </span>
                  {option.period ? (
                    <span className={styles.itemPeriod}>{option.period}</span>
                  ) : null}
                </span>
              </button>
            );
          })}
        </div>
      )}
      {error ? (
        <p className={styles.menuError} role="status">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export default function FloatingBallPopover() {
  const [mode, setMode] = useState<PopoverMode | null>(null);
  // 每次收到展示事件都 +1：窗口可能是按默认高度重新打开的，需要重新测量。
  const [showRevision, setShowRevision] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);

  const applyMode = useCallback((value: PopoverMode) => {
    setMode(value);
    setShowRevision((revision) => revision + 1);
  }, []);

  // 页面可能先于事件就绪：挂载后主动拉取一次，再订阅后续切换。
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getFloatingBallPopoverMode().then((value) => {
      if (!disposed && value) applyMode(value);
    });
    void onFloatingBallPopoverMode((value) => {
      if (!disposed) applyMode(value);
    }).then((stop) => {
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
      // 订阅完成后再拉取一次，覆盖“先取模式、后订阅”之间的切换。
      void getFloatingBallPopoverMode().then((value) => {
        if (!disposed && value) applyMode(value);
      });
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyMode]);

  // 内容高度变化时同步窗口尺寸；Rust 会重新定位弹层。
  useEffect(() => {
    const element = rootRef.current;
    if (!element || !mode) return;
    let frame = 0;
    const report = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        const height = Math.ceil(element.getBoundingClientRect().height);
        if (height > 0) void floatingBallResize(height);
      });
    };
    const observer = new ResizeObserver(report);
    observer.observe(element);
    report();
    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(frame);
    };
  }, [mode, showRevision]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void floatingBallHidePanel();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  if (!mode) return null;

  return (
    <div
      ref={rootRef}
      className={styles.popover}
      data-mode={mode}
      onPointerEnter={() => {
        if (mode === 'hover') void floatingBallPanelHover(true);
      }}
      onPointerLeave={() => {
        if (mode === 'hover') void floatingBallPanelHover(false);
      }}
    >
      {mode === 'hover' ? (
        <HoverCard onSwitch={() => void floatingBallShowMenu()} />
      ) : (
        <MenuCard />
      )}
    </div>
  );
}
