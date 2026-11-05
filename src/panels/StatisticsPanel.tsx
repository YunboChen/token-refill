import { useEffect, useMemo, useState, type MouseEvent } from 'react';
import { ArrowClockwiseIcon, ChartLineUp } from '@phosphor-icons/react';
import {
  getQuotaHistory,
  type QuotaHistoryProduct,
  type QuotaHistorySample,
  type QuotaHistoryWindow,
} from '../lib/desktop';
import { durationLabel } from '../lib/format';
import { platformMeta } from '../lib/platforms';
import { useAppState } from '../lib/store';
import PlatformLogo from '../components/PlatformLogo';
import PillSwitch from '../components/PillSwitch';
import SelectMenu from '../components/SelectMenu';
import styles from '../App.module.css';

/** 与 Rust 侧保留期一致；首版只提供最近 7 天。 */
const HISTORY_DAYS = 7;
const HISTORY_SECS = HISTORY_DAYS * 24 * 60 * 60;
/** 缺口判定下限：至少 15 分钟没有采样就断开，不插值。 */
const MIN_GAP_SECS = 15 * 60;
/** 点数较少时才画采样点，避免 5 分钟采样把折线画成一串圆点。 */
const MAX_DOTS = 40;
/** 平均每两个点的横向间距低于该值时也不再画点，只保留曲线。 */
const MIN_DOT_SPACING = 16;

interface TrendPoint {
  at: number;
  value: number;
  resetsAt: number | null;
}

interface TrendGap {
  from: TrendPoint;
  to: TrendPoint;
}

interface ResetMark {
  /** 平台给出的重置时刻（上一周期结束时间），用于坐标轴与记录。 */
  at: number;
  /** 新周期第一个采样点的值。 */
  value: number;
}

interface TrendSeries {
  segments: TrendPoint[][];
  gaps: TrendGap[];
  resets: ResetMark[];
  metric: 'percent' | 'amount';
  total: number;
}

function seriesValue(
  sample: QuotaHistorySample,
  metric: 'percent' | 'amount',
): number | null {
  return metric === 'percent' ? sample.remainingPercent : sample.remainingAmount;
}

/** 选择坐标轴口径：优先窗口单位；该口径没有数据时退回另一种。 */
function seriesMetric(window: QuotaHistoryWindow): 'percent' | 'amount' {
  const preferred = window.unit === 'percent' ? 'percent' : 'amount';
  if (window.samples.some((sample) => seriesValue(sample, preferred) !== null)) {
    return preferred;
  }
  return preferred === 'percent' ? 'amount' : 'percent';
}

/**
 * 折线分段规则：相邻样本缺口超过阈值，或平台给出的重置时间发生变化，
 * 都在这里断开。缺口用虚线连接并标注「无采样」；重置位置单独标记，
 * 前后不连线。缺失的采样不插值。
 *
 * 缺口阈值取该窗口采样间隔中位数的 3 倍，并以刷新间隔的 3 倍与 15 分钟
 * 兜底：连续采样时能识别真正的离线缺口，数据本来就稀疏时不会把每两点
 * 之间都标成「无采样」。
 */
function buildSeries(window: QuotaHistoryWindow, refreshSecs: number): TrendSeries {
  const metric = seriesMetric(window);
  const points: TrendPoint[] = [];
  for (const sample of window.samples) {
    const value = seriesValue(sample, metric);
    if (value === null) continue;
    points.push({ at: sample.at, value, resetsAt: sample.resetsAtEpoch });
  }
  points.sort((left, right) => left.at - right.at);

  const intervals = points
    .slice(1)
    .map((point, index) => point.at - points[index].at)
    .filter((interval) => interval > 0)
    .sort((left, right) => left - right);
  const median = intervals.length > 0 ? intervals[Math.floor(intervals.length / 2)] : 0;
  const gapSecs = Math.max(median * 3, refreshSecs * 3, MIN_GAP_SECS);

  const segments: TrendPoint[][] = [];
  const gaps: TrendGap[] = [];
  const resets: ResetMark[] = [];
  let current: TrendPoint[] = [];
  for (const point of points) {
    const previous = current[current.length - 1];
    if (previous) {
      const resetChanged =
        previous.resetsAt !== null &&
        point.resetsAt !== null &&
        previous.resetsAt !== point.resetsAt;
      if (resetChanged) {
        segments.push(current);
        // 重置点取平台给出的上一周期结束时间；新周期首个样本给出重置后的值。
        resets.push({ at: previous.resetsAt ?? point.at, value: point.value });
        current = [];
      } else if (point.at - previous.at > gapSecs) {
        segments.push(current);
        gaps.push({ from: previous, to: point });
        current = [];
      }
    }
    current.push(point);
  }
  if (current.length > 0) segments.push(current);

  return { segments, gaps, resets, metric, total: points.length };
}

function stampLabel(epoch: number): string {
  const date = new Date(epoch * 1000);
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function timeLabel(epoch: number): string {
  return new Date(epoch * 1000).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
}

function valueLabel(
  value: number,
  metric: 'percent' | 'amount',
  unit: string,
): string {
  if (metric === 'percent') return `${Math.round(value)}%`;
  if (unit.toUpperCase() === 'USD') return `$${value.toFixed(2)}`;
  return `${value.toFixed(value < 10 ? 2 : 0)} ${unit}`;
}

/** 卡片标题：「每周剩余额度」「5 小时剩余额度」。 */
function windowTitle(window: QuotaHistoryWindow | null): string {
  if (!window) return '剩余额度';
  if (window.durationMins === 10080) return '每周剩余额度';
  if (window.durationMins === 1440) return '每日剩余额度';
  const duration = durationLabel(window.durationMins);
  return duration ? `${duration}剩余额度` : '剩余额度';
}

/** 窗口切换与重置记录里的短名称：「5 小时额度」「每周额度」。 */
function windowShortLabel(window: QuotaHistoryWindow): string {
  if (window.durationMins === 10080) return '每周额度';
  if (window.durationMins === 43200) return '每月额度';
  if (window.durationMins === 1440) return '每日额度';
  const duration = durationLabel(window.durationMins);
  return duration ? `${duration}额度` : '额度';
}

/**
 * Catmull–Rom 样条转三次贝塞尔：曲线经过每个采样点、相邻段切线连续，
 * 比单调插值更圆滑。控制点纵向夹在绘图区内，避免曲线越过 0–100% 的
 * 可视范围。
 */
function smoothPath(
  points: Array<{ x: number; y: number }>,
  minY: number,
  maxY: number,
): string {
  if (points.length === 0) return '';
  if (points.length === 1) {
    return `M${points[0].x.toFixed(1)},${points[0].y.toFixed(1)}`;
  }
  if (points.length === 2) {
    return `M${points[0].x.toFixed(1)},${points[0].y.toFixed(1)} L${points[1].x.toFixed(1)},${points[1].y.toFixed(1)}`;
  }

  const clamp = (value: number) => Math.min(Math.max(value, minY), maxY);
  let path = `M${points[0].x.toFixed(1)},${points[0].y.toFixed(1)}`;
  for (let index = 0; index < points.length - 1; index += 1) {
    const previous = points[index - 1] ?? points[index];
    const current = points[index];
    const next = points[index + 1];
    const after = points[index + 2] ?? next;
    const control1X = current.x + (next.x - previous.x) / 6;
    const control1Y = clamp(current.y + (next.y - previous.y) / 6);
    const control2X = next.x - (after.x - current.x) / 6;
    const control2Y = clamp(next.y - (after.y - current.y) / 6);
    path +=
      ` C${control1X.toFixed(1)},${control1Y.toFixed(1)}` +
      ` ${control2X.toFixed(1)},${control2Y.toFixed(1)}` +
      ` ${next.x.toFixed(1)},${next.y.toFixed(1)}`;
  }
  return path;
}

function TrendChart({
  series,
  window,
}: {
  series: TrendSeries;
  window: QuotaHistoryWindow;
}) {
  const [hover, setHover] = useState<TrendPoint | null>(null);

  if (series.total === 0) {
    return (
      <p className={styles.statisticsEmpty}>
        这个窗口还没有可画的数值；只有成功查询并返回剩余比例或余额时才会记录。
      </p>
    );
  }

  const width = 720;
  const height = 300;
  const padding = { left: 56, right: 20, top: 28, bottom: 34 };
  const innerWidth = width - padding.left - padding.right;
  const innerHeight = height - padding.top - padding.bottom;
  const now = Math.floor(Date.now() / 1000);
  const start = now - HISTORY_SECS;
  const points = series.segments.flat();
  const maxAmount = Math.max(1, ...points.map((point) => point.value));
  const yMax = series.metric === 'percent' ? 100 : maxAmount * 1.1;

  const x = (at: number) =>
    padding.left +
    ((Math.min(Math.max(at, start), now) - start) / HISTORY_SECS) * innerWidth;
  const y = (value: number) =>
    padding.top + (1 - Math.min(value, yMax) / yMax) * innerHeight;

  const yTicks = [0, 0.25, 0.5, 0.75, 1].map((ratio) => {
    const value = yMax * (1 - ratio);
    return {
      y: padding.top + ratio * innerHeight,
      label:
        series.metric === 'percent'
          ? `${Math.round(value)}%`
          : valueLabel(value, 'amount', window.unit),
    };
  });

  // 横轴按自然日标注，最多 7 个刻度。
  const dayTicks = useMemo(() => {
    const base = new Date();
    base.setHours(0, 0, 0, 0);
    return Array.from({ length: HISTORY_DAYS }, (_, index) => {
      const date = new Date(base);
      date.setDate(base.getDate() - (HISTORY_DAYS - 1 - index));
      return {
        at: date.getTime() / 1000,
        label: `${date.getMonth() + 1}/${date.getDate()}`,
      };
    });
  }, []);

  const pathFor = (segment: TrendPoint[]) =>
    smoothPath(
      segment.map((point) => ({ x: x(point.at), y: y(point.value) })),
      padding.top,
      padding.top + innerHeight,
    );

  const spacing = innerWidth / Math.max(1, series.total);
  const showDots = series.total <= MAX_DOTS && spacing >= MIN_DOT_SPACING;

  const handleMove = (event: MouseEvent<SVGRectElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    if (bounds.width === 0) return;
    const svgX = ((event.clientX - bounds.left) / bounds.width) * innerWidth + padding.left;
    let nearest: TrendPoint | null = null;
    let distance = 26;
    for (const point of points) {
      const candidate = Math.abs(x(point.at) - svgX);
      if (candidate < distance) {
        nearest = point;
        distance = candidate;
      }
    }
    setHover(nearest);
  };

  const tooltipWidth = 122;
  const tooltipHeight = 48;
  const tooltipX = hover
    ? Math.min(
        Math.max(x(hover.at) - tooltipWidth / 2, padding.left),
        width - padding.right - tooltipWidth,
      )
    : 0;
  const tooltipY = hover
    ? Math.min(
        Math.max(y(hover.value) - tooltipHeight - 14, padding.top),
        height - padding.bottom - tooltipHeight - 4,
      )
    : 0;

  return (
    <svg
      className={styles.statisticsChart}
      viewBox={`0 0 ${width} ${height}`}
      role="img"
      aria-label={`${windowTitle(window)}，最近 ${HISTORY_DAYS} 天共 ${series.total} 个采样点`}
      onMouseLeave={() => setHover(null)}
    >
      <text
        className={styles.statisticsAxisLabel}
        transform={`rotate(-90 ${padding.left - 34} ${padding.top + innerHeight / 2})`}
        x={padding.left - 34}
        y={padding.top + innerHeight / 2}
        textAnchor="middle"
      >
        {series.metric === 'percent' ? '剩余比例' : `剩余额度（${window.unit}）`}
      </text>

      {yTicks.map((tick) => (
        <g key={tick.y}>
          <line
            className={styles.statisticsGrid}
            x1={padding.left}
            x2={width - padding.right}
            y1={tick.y}
            y2={tick.y}
          />
          <text
            className={styles.statisticsAxis}
            x={padding.left - 10}
            y={tick.y + 3.5}
            textAnchor="end"
          >
            {tick.label}
          </text>
        </g>
      ))}

      {dayTicks.map((tick) => (
        <text
          key={tick.at}
          className={styles.statisticsAxis}
          x={x(tick.at)}
          y={height - 12}
          textAnchor="middle"
        >
          {tick.label}
        </text>
      ))}

      {series.gaps.map((gap) => (
        <g key={`gap-${gap.from.at}-${gap.to.at}`}>
          <line
            className={styles.statisticsGapLine}
            x1={x(gap.from.at)}
            y1={y(gap.from.value)}
            x2={x(gap.to.at)}
            y2={y(gap.to.value)}
          />
          {/* 缺口太窄时只画虚线，避免标签彼此重叠。 */}
          {x(gap.to.at) - x(gap.from.at) >= 56 ? (
            <g
              transform={`translate(${(x(gap.from.at) + x(gap.to.at)) / 2 - 24}, ${
                (y(gap.from.value) + y(gap.to.value)) / 2 - 20
              })`}
            >
              <rect
                className={styles.statisticsBadge}
                width={48}
                height={17}
                rx={5}
              />
              <text
                className={styles.statisticsBadgeText}
                x={24}
                y={12}
                textAnchor="middle"
              >
                无采样
              </text>
            </g>
          ) : null}
        </g>
      ))}

      {series.segments.map((segment, index) => (
        <g key={`segment-${index}`}>
          {segment.length > 1 ? (
            <path className={styles.statisticsLine} d={pathFor(segment)} />
          ) : null}
          {segment.length === 1 || showDots
            ? segment.map((point) => (
                <circle
                  key={point.at}
                  className={styles.statisticsDot}
                  cx={x(point.at)}
                  cy={y(point.value)}
                  r={3}
                />
              ))
            : null}
        </g>
      ))}

      {series.resets.map((mark) => (
        <g key={`reset-${mark.at}`}>
          <line
            className={styles.statisticsReset}
            x1={x(mark.at)}
            x2={x(mark.at)}
            y1={padding.top}
            y2={padding.top + innerHeight}
          />
          <g transform={`translate(${x(mark.at) - 31}, 2)`}>
            <rect
              className={styles.statisticsBadge}
              width={62}
              height={19}
              rx={5}
            />
            <text
              className={styles.statisticsBadgeText}
              x={31}
              y={13}
              textAnchor="middle"
            >
              周期重置
            </text>
          </g>
          <circle
            className={styles.statisticsResetDot}
            cx={x(mark.at)}
            cy={y(mark.value)}
            r={4}
          />
        </g>
      ))}

      {hover ? (
        <g pointerEvents="none">
          <line
            className={styles.statisticsGuide}
            x1={x(hover.at)}
            x2={x(hover.at)}
            y1={padding.top}
            y2={padding.top + innerHeight}
          />
          <circle
            className={styles.statisticsHoverDot}
            cx={x(hover.at)}
            cy={y(hover.value)}
            r={4.5}
          />
          <g transform={`translate(${tooltipX}, ${tooltipY})`}>
            <rect
              className={styles.statisticsTooltip}
              width={tooltipWidth}
              height={tooltipHeight}
              rx={8}
            />
            <text className={styles.statisticsTooltipTime} x={10} y={18}>
              {stampLabel(hover.at)}
            </text>
            <text className={styles.statisticsTooltipValue} x={10} y={36}>
              剩余 {valueLabel(hover.value, series.metric, window.unit)}
            </text>
          </g>
        </g>
      ) : null}

      <rect
        x={padding.left}
        y={padding.top}
        width={innerWidth}
        height={innerHeight}
        fill="transparent"
        onMouseMove={handleMove}
      />
    </svg>
  );
}

function RefreshButton({
  refreshing,
  onRefresh,
  accountName,
}: {
  refreshing: boolean;
  onRefresh: () => void;
  accountName: string;
}) {
  return (
    <button
      type="button"
      className={styles.statisticsRefresh}
      onClick={onRefresh}
      disabled={refreshing}
      aria-label={`刷新 ${accountName} 的额度`}
      title="重新查询额度"
    >
      <ArrowClockwiseIcon
        size={14}
        className={refreshing ? styles.spin : undefined}
        aria-hidden
      />
    </button>
  );
}

export default function StatisticsPanel({ desktop }: { desktop: boolean }) {
  const { state, refresh } = useAppState();
  const [accountId, setAccountId] = useState<string | null>(null);
  const [productId, setProductId] = useState<string | null>(null);
  const [windowKey, setWindowKey] = useState<string | null>(null);
  const [history, setHistory] = useState<QuotaHistoryProduct[]>([]);
  const [loading, setLoading] = useState(false);

  const accounts = state.accounts;
  const selected =
    accounts.find((account) => account.accountId === accountId) ?? accounts[0] ?? null;

  // 账户被移除或首次进入时选中第一个可用账户。
  useEffect(() => {
    if (accounts.length === 0) {
      setAccountId(null);
      return;
    }
    if (!accounts.some((account) => account.accountId === accountId)) {
      setAccountId(accounts[0].accountId);
    }
  }, [accounts, accountId]);

  useEffect(() => {
    if (!desktop || !selected) {
      setHistory([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    void getQuotaHistory(selected.accountId)
      .then((data) => {
        if (!cancelled) setHistory(data);
      })
      .catch(() => {
        if (!cancelled) setHistory([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // revision 变化意味着有新的查询结果，重新读取一次。
  }, [desktop, selected, state.revision]);

  const products = useMemo(
    () => history.filter((entry) => entry.windows.length > 0),
    [history],
  );
  const product =
    products.find((entry) => entry.productId === productId) ?? products[0] ?? null;
  const window =
    product?.windows.find((entry) => entry.key === windowKey) ??
    product?.windows[0] ??
    null;

  const refreshSecs = state.preferences.refreshMinutes * 60;
  const series = useMemo(
    () => (window ? buildSeries(window, refreshSecs) : null),
    [window, refreshSecs],
  );
  const lastPoint = series?.segments.flat().at(-1) ?? null;
  const refreshing = selected?.products.some((item) => item.query.refreshing) ?? false;

  if (!desktop) {
    return (
      <p className={styles.accountNote}>
        浏览器预览无法调用 Rust，请在桌面应用内查看（
        <code className={styles.codeChip}>pnpm tauri dev</code>）。
      </p>
    );
  }

  if (!selected) {
    return (
      <p className={styles.statisticsEmpty}>
        还没有可统计的账户。先在「账户」或「中转站」接入账户。
      </p>
    );
  }

  return (
    <div>
      <div className={styles.statisticsToolbar}>
        <SelectMenu
          value={selected.accountId}
          onChange={(accountKey) => {
            setAccountId(accountKey);
            setProductId(null);
            setWindowKey(null);
          }}
          label="选择账户"
          options={accounts.map((account) => ({
            value: account.accountId,
            label: `${platformMeta(account.platform).name} · ${account.displayName}`,
            icon: (
              <PlatformLogo
                platform={account.platform}
                fallback={platformMeta(account.platform).mark}
              />
            ),
          }))}
        />

        {products.length > 1 ? (
          <PillSwitch
            value={product?.productId ?? ''}
            label="套餐"
            equal
            options={products.map((entry) => ({
              value: entry.productId,
              label:
                selected.products.find(
                  (item) => item.productId === entry.productId,
                )?.displayName ?? entry.productId,
            }))}
            onChange={(nextProductId) => {
              setProductId(nextProductId);
              setWindowKey(null);
            }}
          />
        ) : null}

        {product && product.windows.length > 1 ? (
          <PillSwitch
            value={window?.key ?? ''}
            label="额度窗口"
            equal
            options={product.windows.map((entry) => ({
              value: entry.key,
              label: windowShortLabel(entry),
            }))}
            onChange={setWindowKey}
          />
        ) : null}
      </div>

      <section className={styles.statisticsCard}>
        <header className={styles.statisticsCardHead}>
          <div>
            <h2 className={styles.statisticsCardTitle}>{windowTitle(window)}</h2>
            {series && series.total > 0 ? (
              <p className={styles.statisticsLegend}>
                <span className={styles.statisticsLegendDot} aria-hidden />
                {series.metric === 'amount' ? '剩余额度' : '剩余比例'}
              </p>
            ) : null}
          </div>
          <div className={styles.statisticsCurrent}>
            {lastPoint && series ? (
              <>
                <span className={styles.statisticsCurrentValue}>
                  {valueLabel(lastPoint.value, series.metric, window?.unit ?? '')}
                </span>
                <span className={styles.statisticsUpdated}>
                  最近更新于 {timeLabel(lastPoint.at)}
                  <RefreshButton
                    refreshing={refreshing}
                    onRefresh={() => void refresh(selected.accountId)}
                    accountName={selected.displayName}
                  />
                </span>
              </>
            ) : (
              <span className={styles.statisticsUpdated}>
                暂无数据
                <RefreshButton
                  refreshing={refreshing}
                  onRefresh={() => void refresh(selected.accountId)}
                  accountName={selected.displayName}
                />
              </span>
            )}
          </div>
        </header>

        {loading ? (
          <div className={styles.statisticsEmptyState} role="status">
            <p className={styles.statisticsEmptyText}>正在读取走势…</p>
          </div>
        ) : !window || !series || series.total === 0 ? (
          <div className={styles.statisticsEmptyState} role="status">
            <span className={styles.statisticsEmptyIcon} aria-hidden>
              <ChartLineUp size={18} />
            </span>
            <p className={styles.statisticsEmptyTitle}>
              {history.length === 0 ? '还没有额度走势' : '这个窗口还没有数值'}
            </p>
            <p className={styles.statisticsEmptyText}>
              {history.length === 0
                ? '完成一次成功查询后开始积累，只保留最近 7 天。'
                : '只有成功查询并返回剩余比例或余额时才会记录。'}
            </p>
            {history.length === 0 ? (
              <button
                type="button"
                className={styles.statisticsEmptyAction}
                onClick={() => void refresh(selected.accountId)}
                disabled={refreshing}
              >
                {refreshing ? '刷新中…' : '立即刷新'}
              </button>
            ) : null}
          </div>
        ) : (
          <TrendChart series={series} window={window} />
        )}

        <footer className={styles.statisticsCardFoot}>
          <span>本机记录 · 最近 {HISTORY_DAYS} 天</span>
          <span>缺失时段留空，周期重置后分段展示</span>
        </footer>
      </section>

      {series && series.resets.length > 0 ? (
        <section className={styles.statisticsCard}>
          <h2 className={styles.statisticsSubheading}>重置记录</h2>
          <ul className={styles.statisticsResetList}>
            {series.resets.map((mark) => (
              <li key={mark.at}>
                <time className={styles.statisticsResetTime} dateTime={new Date(mark.at * 1000).toISOString()}>
                  {stampLabel(mark.at)}
                </time>
                <span className={styles.statisticsResetLabel}>
                  {window ? windowShortLabel(window) : '额度'}重置
                </span>
                <span className={styles.statisticsResetValue}>
                  {valueLabel(
                    mark.value,
                    series.metric,
                    window?.unit ?? '',
                  )}
                </span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
}
