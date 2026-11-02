import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { PlugsIcon, Plus } from '@phosphor-icons/react';
import {
  floatingBallDragEnd,
  floatingBallDragStart,
  floatingBallHover,
  floatingBallTogglePanel,
  getFloatingBallLayout,
  onFloatingBallLayoutChanged,
  type FloatingBallLayout,
} from '../lib/desktop';
import { optionFromSelection, resolveSelection, severityFor } from '../lib/float';
import { useRelay } from '../lib/relay';
import { useAppState } from '../lib/store';
import PlatformLogo from '../components/PlatformLogo';
import styles from './FloatingBall.module.css';

/** 最外圈进度环的半径；与 CSS 里的 SVG 尺寸保持一致。 */
const RING_RADIUS = 26.5;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;
/** 收起时露出的宽度；与 Rust 的 `SNAP_VISIBLE` 一致。 */
const PEEK_VISIBLE = 24;
/** 环与露出边界相交的位置：球心到贴边侧的距离。 */
const PEEK_CUT_DX = 60 / 2 - PEEK_VISIBLE;
const PEEK_HALF_CHORD = Math.sqrt(RING_RADIUS ** 2 - PEEK_CUT_DX ** 2);
const PEEK_TOP = 30 - PEEK_HALF_CHORD;
const PEEK_BOTTOM = 30 + PEEK_HALF_CHORD;
/**
 * 收起态的归一化弧：从下方切口沿露出侧扫到上方切口，进度按整段弧重新映射，
 * 而不是把整圈环裁掉一半；低额度时从下方切口开始露出一小段。
 * 右侧吸附露出球的左半，弧向左鼓；左侧吸附镜像。
 */
const PEEK_PATH_RIGHT = `M ${PEEK_VISIBLE} ${PEEK_BOTTOM} A ${RING_RADIUS} ${RING_RADIUS} 0 0 1 ${PEEK_VISIBLE} ${PEEK_TOP}`;
const PEEK_PATH_LEFT = `M ${60 - PEEK_VISIBLE} ${PEEK_BOTTOM} A ${RING_RADIUS} ${RING_RADIUS} 0 0 0 ${60 - PEEK_VISIBLE} ${PEEK_TOP}`;
/** 拖动判定阈值（屏幕像素）：小于它视为点击。 */
const DRAG_THRESHOLD = 4;

export default function FloatingBall() {
  const { state } = useAppState();
  const { accounts: relays, ensureLoaded } = useRelay();
  const [dragging, setDragging] = useState(false);
  const [layout, setLayout] = useState<FloatingBallLayout>({
    edge: null,
    collapsed: false,
    clipped: false,
  });
  // 首帧拿到真实布局后才启用内容过渡，避免启动时 logo 从居中滑到贴边。
  const [transitions, setTransitions] = useState(false);
  const drag = useRef<{
    x: number;
    y: number;
    pointerId: number;
    started: boolean;
  } | null>(null);

  useEffect(() => {
    ensureLoaded();
  }, [ensureLoaded]);

  // 内部边界收起时窗口缩为 24 点宽；布局由 Rust 通知，挂载后再拉取一次兜底。
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let frame = 0;
    void getFloatingBallLayout().then((value) => {
      if (disposed) return;
      setLayout(value);
      // 等初始布局落画后再开启过渡：启动时球已经是收起状态，
      // 不应该播放一次“展开 → 收起”的内容位移动画。
      frame = window.requestAnimationFrame(() => {
        frame = window.requestAnimationFrame(() => {
          if (!disposed) setTransitions(true);
        });
      });
    });
    void onFloatingBallLayoutChanged((value) => {
      if (!disposed) setLayout(value);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      window.cancelAnimationFrame(frame);
      unlisten?.();
    };
  }, []);

  // 拖动途中窗口被销毁时收尾，避免 Rust 侧一直轮询光标。
  useEffect(
    () => () => {
      if (drag.current?.started) void floatingBallDragEnd();
    },
    [],
  );

  const selection = resolveSelection(state, relays);
  const option = optionFromSelection(selection);
  const severity = severityFor(
    option,
    state.preferences.lowQuotaThreshold,
    state.preferences.lowQuotaEnabled,
  );
  const percent =
    option?.percent !== null && option?.percent !== undefined
      ? Math.max(0, Math.min(100, option.percent))
      : null;
  const hasError = Boolean(option?.error) && !option?.available;
  const refreshing = option?.refreshing ?? false;
  const dashOffset =
    percent === null ? RING_CIRCUMFERENCE : RING_CIRCUMFERENCE * (1 - percent / 100);

  const mainText = percent !== null
    ? `${Math.round(percent)}%`
    : option
      ? (option.amount ?? '—')
      : null;
  const detailText = !option
    ? '选择'
    : hasError
      ? '异常'
      : (option.period ?? (refreshing ? '刷新中' : '额度'));
  const compactMain = percent === null && option !== null && option.amount !== null;

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      x: event.screenX,
      y: event.screenY,
      pointerId: event.pointerId,
      started: false,
    };
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    if (current.started) return;
    const distance = Math.hypot(
      event.screenX - current.x,
      event.screenY - current.y,
    );
    if (distance < DRAG_THRESHOLD) return;
    current.started = true;
    setDragging(true);
    void floatingBallDragStart();
  }

  function finishPointer(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (current.started) {
      setDragging(false);
      void floatingBallDragEnd();
    } else {
      void floatingBallTogglePanel();
    }
  }

  function cancelPointer(event: PointerEvent<HTMLDivElement>) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    drag.current = null;
    if (current.started) {
      setDragging(false);
      void floatingBallDragEnd();
    }
  }

  return (
    <div
      className={styles.stage}
      data-dragging={dragging || undefined}
      data-severity={severity}
      data-edge={layout.edge ?? undefined}
      data-collapsed={layout.collapsed || undefined}
      data-clipped={layout.clipped || undefined}
      data-transitions={transitions || undefined}
    >
      <div
        className={styles.ball}
        role="button"
        tabIndex={-1}
        aria-label={
          option
            ? `${option.title}：${percent !== null ? `剩余 ${Math.round(percent)}%` : mainText ?? '暂无数据'}，点击切换套餐`
            : '悬浮球尚未选择套餐，点击选择'
        }
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={finishPointer}
        onPointerCancel={cancelPointer}
        onLostPointerCapture={cancelPointer}
        onPointerEnter={() => {
          if (!drag.current) void floatingBallHover(true);
        }}
        onPointerLeave={() => {
          if (!drag.current) void floatingBallHover(false);
        }}
        onContextMenu={(event) => event.preventDefault()}
      >
        <svg className={styles.ring} viewBox="0 0 60 60" aria-hidden="true">
          <circle
            className={styles.track}
            cx="30"
            cy="30"
            r={RING_RADIUS}
          />
          {percent !== null ? (
            <circle
              className={styles.progress}
              cx="30"
              cy="30"
              r={RING_RADIUS}
              strokeDasharray={RING_CIRCUMFERENCE}
              strokeDashoffset={dashOffset}
            />
          ) : null}
          {refreshing ? (
            <circle
              className={styles.spinner}
              cx="30"
              cy="30"
              r={24}
            />
          ) : null}
        </svg>

        {/* 收起态的环：只画露出的那段弧，并把进度归一化到这段弧上。 */}
        <svg className={styles.peekRing} viewBox="0 0 60 60" aria-hidden="true">
          <path
            className={styles.track}
            d={layout.edge === 'left' ? PEEK_PATH_LEFT : PEEK_PATH_RIGHT}
          />
          {percent !== null ? (
            <path
              className={styles.progress}
              d={layout.edge === 'left' ? PEEK_PATH_LEFT : PEEK_PATH_RIGHT}
              pathLength={100}
              strokeDasharray={100}
              strokeDashoffset={100 - percent}
            />
          ) : null}
          {refreshing ? (
            <circle
              className={styles.spinner}
              cx="30"
              cy="30"
              r={24}
            />
          ) : null}
        </svg>

        <div className={styles.center}>
          <span className={styles.logo}>
            {option?.kind === 'relay' ? (
              <PlugsIcon size={12} weight="fill" aria-hidden />
            ) : option ? (
              <PlatformLogo
                platform={option.platform}
                fallback={option.title.slice(0, 1)}
              />
            ) : (
              <Plus size={13} weight="bold" aria-hidden />
            )}
          </span>
          <span
            className={styles.value}
            data-compact={compactMain || undefined}
          >
            {mainText ?? ''}
          </span>
          <span className={styles.detail}>{detailText}</span>
        </div>
      </div>
    </div>
  );
}
