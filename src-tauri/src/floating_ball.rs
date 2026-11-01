//! 桌面悬浮球：置顶小窗口、拖动与屏幕边缘吸附、悬停详情和套餐切换弹层。
//!
//! 悬浮球默认关闭，由设置页的独立命令开启；关闭时两个窗口一起销毁。
//! 球通常为 60×60 点，内部边界收起时缩为 24×60 点；弹层按内容高度自适应。
//!
//! 交互约定：
//! - 鼠标移入球即展开吸附状态（离屏幕边缘 12 点），移出 1 秒后收回，
//!   只保留 24 点可见（呈 D 形）。虚拟桌面最外侧边缘通过窗口移出屏幕实现；
//!   两块屏幕之间的内部边界把窗口缩到 24 点宽并裁剪绘制一侧，同样不会
//!   露出到相邻显示器。
//! - 拖动由前端指针事件触发，Rust 轮询光标位置移动窗口；松开后吸附、
//!   夹取到工作区并保存位置。
//! - 悬停显示详情，点击显示套餐切换菜单；菜单失焦或再次点击球收起。
//!
//! 窗口创建一律投递到主线程，且不在窗口事件回调里就地创建，沿用
//! `tray.rs` 的约束：托盘/窗口回调本身在主线程，就地创建 WebView 会卡住事件循环。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::utils::config::WindowConfig;
use tauri::webview::{PageLoadEvent, WebviewWindowBuilder};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};

use crate::logging::lifecycle;
use crate::relay::RelayStore;
use crate::state::{FloatingBallEdge, FloatingBallPosition, PersistedState, StateStore};

pub const BALL_LABEL: &str = "floating-ball";
pub const POPOVER_LABEL: &str = "float-popover";
/// 弹层模式变化时发给弹层窗口；前端据此切换详情/菜单内容。
pub const POPOVER_MODE_EVENT: &str = "floating-ball-popover-mode";
/// 球窗口布局变化：内部边界收起时窗口缩为 24×60，前端只绘制球的一侧。
pub const BALL_LAYOUT_EVENT: &str = "floating-ball-layout";

/// 球的逻辑尺寸（点），与 `tauri.conf.json` 中的窗口尺寸一致。
const BALL_SIZE: f64 = 60.0;
/// 吸附后保留可见的逻辑宽度（设计稿 24px）。
const SNAP_VISIBLE: f64 = 24.0;
/// 展开后离屏幕边缘的逻辑距离（设计稿 12px）。
const EXPAND_GAP: f64 = 12.0;
/// 拖动结束时，球距离边缘小于该值就吸附。
const SNAP_DISTANCE: f64 = 24.0;
/// 弹层固定逻辑宽度与离球的间距。
const POPOVER_WIDTH: f64 = 340.0;
const POPOVER_GAP: f64 = 10.0;
const POPOVER_MARGIN: f64 = 8.0;
const POPOVER_MIN_HEIGHT: f64 = 96.0;
const POPOVER_MAX_HEIGHT: f64 = 520.0;
const HOVER_HEIGHT: f64 = 244.0;
const MENU_HEIGHT: f64 = 380.0;
/// 鼠标离开球/弹层后，多久收起悬停详情。
const HOVER_HIDE_DELAY: Duration = Duration::from_millis(320);
/// 鼠标离开后多久把吸附的球收回边缘。
const COLLAPSE_DELAY: Duration = Duration::from_secs(1);
/// 悬停看门狗轮询间隔。非 key 的 NSPanel 在菜单取得焦点后可能收不到
/// pointerleave，需要定期用光标位置兜底，否则球会一直停在展开状态。
const HOVER_WATCH_TICK: Duration = Duration::from_millis(150);
/// 拖动轮询间隔。
const DRAG_TICK: Duration = Duration::from_millis(16);
/// 展开：从贴边弹出到间隙位置，末段带一点回弹。
const BALL_EXPAND: Duration = Duration::from_millis(220);
/// 收起分两段：先保持完整外观滑到边缘，贴边后再收成 D 形。
/// 位移与形变同时进行时，球在半路上就已经是 D 形，看起来像“还没贴边就吸附了”。
const BALL_SNAP_SLIDE: Duration = Duration::from_millis(160);
const BALL_SNAP_MORPH: Duration = Duration::from_millis(180);
/// 布局事件发出后多久开始收窄窗口：让前端先落一帧，内容与窗口同步变形。
const BALL_SNAP_LEAD: Duration = Duration::from_millis(16);
/// 自由位置松手后的落点过渡。
const BALL_DROP: Duration = Duration::from_millis(220);
const BALL_ANIMATION_TICK: Duration = Duration::from_millis(16);
/// 拖动开始前等待窗口尺寸/位置生效的时间。
const DRAG_SETTLE: Duration = Duration::from_millis(30);
/// 拖动兜底时长：前端丢失 pointerup 时不会永远拖着窗口。
const DRAG_TIMEOUT: Duration = Duration::from_secs(30);
/// 页面加载事件迟迟不来时直接显示，避免窗口永远隐藏。
const LOAD_FALLBACK: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopoverMode {
    Hover,
    Menu,
}

impl PopoverMode {
    fn as_str(self) -> &'static str {
        match self {
            PopoverMode::Hover => "hover",
            PopoverMode::Menu => "menu",
        }
    }

    fn default_height(self) -> f64 {
        match self {
            PopoverMode::Hover => HOVER_HEIGHT,
            PopoverMode::Menu => MENU_HEIGHT,
        }
    }
}

/// 打开弹层时的高度：切换模式先回到该模式的默认高度，等前端量出新内容；
/// 同一模式重新打开时沿用已测量的高度，避免把内容压回默认值裁掉底部。
fn popover_height_for(mode: PopoverMode, shown_mode: Option<PopoverMode>, measured: f64) -> f64 {
    if shown_mode == Some(mode) {
        measured
    } else {
        mode.default_height()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BallLayout {
    /// 吸附边缘；`None` 表示自由位置。
    pub edge: Option<FloatingBallEdge>,
    /// 收起状态：只露出贴边的 24 点，前端把中心内容换成 logo、额度环收缩到露出的弧。
    pub collapsed: bool,
    /// 内部边界收起：窗口只有 24 点宽，球的一侧被窗口裁掉。
    pub clipped: bool,
}

#[derive(Default)]
struct Runtime {
    /// 吸附边缘；`None` 表示自由位置。
    edge: Option<FloatingBallEdge>,
    /// 吸附状态下球的物理 y 坐标。
    y: Option<f64>,
    /// 最近一次布局目标的物理 x 坐标。窗口过渡期间窗口会逐帧移动，
    /// 弹层按这个目标位置定位，避免跟着动画抖动。
    layout_x: Option<f64>,
    /// 最近一次记录位置所在显示器名称。
    monitor: Option<String>,
    /// 当前吸附边缘是否允许把球藏到屏幕外。只有虚拟桌面外侧边缘才为 true；
    /// 相邻显示器之间必须保持完整可见，否则球会出现在另一块屏幕上。
    hidden_edge: bool,
    expanded: bool,
    dragging: bool,
    hover_ball: bool,
    hover_panel: bool,
    /// 吸附球展开期间挂起的详情卡请求；等窗口过渡结束、指针仍在球上才显示。
    hover_show_pending: bool,
    popover_mode: Option<PopoverMode>,
    /// 最近一次展示给前端的模式。收起时保留（前端组件不会卸载），
    /// 再次打开同一模式时沿用已经测量的内容高度。
    shown_mode: Option<PopoverMode>,
    popover_height: f64,
}

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
static BALL_LOADING: AtomicBool = AtomicBool::new(false);
static BALL_GENERATION: AtomicU64 = AtomicU64::new(0);
static POPOVER_LOADING: AtomicBool = AtomicBool::new(false);
static POPOVER_LOADED: AtomicBool = AtomicBool::new(false);
static POPOVER_GENERATION: AtomicU64 = AtomicU64::new(0);
static HOVER_HIDE_GENERATION: AtomicU64 = AtomicU64::new(0);
static HOVER_SHOW_GENERATION: AtomicU64 = AtomicU64::new(0);
static COLLAPSE_GENERATION: AtomicU64 = AtomicU64::new(0);
/// 球窗口过渡的代号；新动画、拖动开始或窗口销毁都会顶掉旧动画。
static BALL_ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);
/// 菜单因失焦收起的时间。紧接着的球点击通常就是在“关闭菜单”，
/// 不能再把菜单重新打开一次。
static LAST_POPOVER_BLUR: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
const BLUR_CLICK_WINDOW: Duration = Duration::from_millis(250);

fn runtime() -> std::sync::MutexGuard<'static, Runtime> {
    RUNTIME
        .get_or_init(|| {
            Mutex::new(Runtime {
                popover_height: HOVER_HEIGHT,
                ..Runtime::default()
            })
        })
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 当前球窗口的布局。内部边界收起时 `clipped=true`，窗口宽度是 24 点。
fn current_layout() -> BallLayout {
    let rt = runtime();
    let collapsed = rt.edge.is_some() && !rt.expanded;
    BallLayout {
        edge: rt.edge,
        collapsed,
        clipped: collapsed && !rt.hidden_edge,
    }
}

/// 布局变化后通知球窗口；前端错过事件时会在挂载后主动拉取一次。
fn emit_layout(app: &AppHandle) {
    let _ = app.emit_to(BALL_LABEL, BALL_LAYOUT_EVENT, current_layout());
}

fn window_config(app: &AppHandle, label: &str) -> Option<WindowConfig> {
    app.config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .cloned()
}

fn defer_to_main<F>(app: &AppHandle, task: F)
where
    F: FnOnce(AppHandle) + Send + 'static,
{
    let handle = app.clone();
    thread::spawn(move || {
        let target = handle.clone();
        if let Err(err) = handle.run_on_main_thread(move || task(target)) {
            lifecycle!("[float] 投递主线程任务失败：{err}");
        }
    });
}

/// 从 IPC 线程按提交顺序转到主线程执行。
///
/// 悬停与拖动会产生连续命令，先起后台线程再投递会打乱顺序（例如
/// `hover(true)` 后紧跟 `hover(false)` 可能反向执行）；窗口事件回调不走这里。
fn on_main<F>(app: &AppHandle, task: F)
where
    F: FnOnce(AppHandle) + Send + 'static,
{
    let target = app.clone();
    if let Err(err) = app.run_on_main_thread(move || task(target)) {
        lifecycle!("[float] 投递主线程任务失败：{err}");
    }
}

// ---------- Tauri 命令 ----------

/// 设置页开关：偏好落盘与窗口创建分开处理，窗口失败会把开关退回关闭。
#[tauri::command]
pub fn set_floating_ball_enabled(
    app: AppHandle,
    store: State<'_, Arc<StateStore>>,
    enabled: bool,
) -> Result<PersistedState, String> {
    let state = store.set_floating_ball_enabled(enabled)?;
    set_enabled(&app, enabled);
    Ok(state)
}

#[tauri::command]
pub fn floating_ball_drag_start(app: AppHandle) {
    drag_start(&app);
}

#[tauri::command]
pub fn floating_ball_drag_end(app: AppHandle) {
    drag_end(&app);
}

#[tauri::command]
pub fn floating_ball_hover(app: AppHandle, hovered: bool) {
    set_hover(&app, HoverTarget::Ball, hovered);
}

#[tauri::command]
pub fn floating_ball_panel_hover(app: AppHandle, hovered: bool) {
    set_hover(&app, HoverTarget::Popover, hovered);
}

#[tauri::command]
pub fn floating_ball_toggle_panel(app: AppHandle) {
    toggle_menu(&app);
}

#[tauri::command]
pub fn floating_ball_show_menu(app: AppHandle) {
    show_menu(&app);
}

#[tauri::command]
pub fn floating_ball_hide_panel(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
pub fn floating_ball_popover_resize(app: AppHandle, height: f64) {
    resize_popover(&app, height);
}

#[tauri::command]
pub fn floating_ball_popover_mode() -> Option<String> {
    popover_mode()
}

/// 球窗口挂载后主动拉取一次布局，避免错过创建期间的布局事件。
#[tauri::command]
pub fn floating_ball_layout() -> BallLayout {
    current_layout()
}

/// 选择套餐；校验通过后写入状态，事件会把新选择同步到球窗口。
#[tauri::command]
pub fn floating_ball_select(
    app: AppHandle,
    store: State<'_, Arc<StateStore>>,
    key: String,
) -> Result<PersistedState, String> {
    validate_selection(&app, &key)?;
    let state = store.set_floating_ball_selection(Some(key))?;
    on_main(&app, |app| {
        hide_popover(&app);
        collapse_if_idle(&app);
    });
    Ok(state)
}

// ---------- 命令入口 ----------

/// 设置页开关：先落盘偏好，再异步创建或销毁窗口。
pub fn set_enabled(app: &AppHandle, enabled: bool) {
    if enabled {
        on_main(app, |app| {
            ensure_ball(&app);
        });
    } else {
        on_main(app, |app| {
            destroy_float_windows(&app);
        });
    }
}

/// 启动时按偏好恢复悬浮球；关闭状态下什么都不做。
pub fn restore(app: &AppHandle) {
    if app
        .state::<Arc<StateStore>>()
        .snapshot()
        .preferences
        .floating_ball_enabled
    {
        ensure_ball(app);
    }
}

/// 校验套餐标识仍指向一个存在的订阅套餐或中转站账户。
pub fn validate_selection(app: &AppHandle, key: &str) -> Result<(), String> {
    if let Some(rest) = key.strip_prefix("product:") {
        let (account_id, product_id) = rest
            .split_once(':')
            .ok_or_else(|| "套餐标识无效。".to_string())?;
        let state = app.state::<Arc<StateStore>>().snapshot();
        let exists = state.accounts.iter().any(|account| {
            account.account_id == account_id
                && account
                    .products
                    .iter()
                    .any(|product| product.product_id == product_id)
        });
        return if exists {
            Ok(())
        } else {
            Err("所选套餐已不存在，请重新选择。".to_string())
        };
    }
    if let Some(id) = key.strip_prefix("relay:") {
        return if app.state::<Arc<RelayStore>>().has(id) {
            Ok(())
        } else {
            Err("所选中转站账户已不存在，请重新选择。".to_string())
        };
    }
    Err("套餐标识无效。".to_string())
}

/// 球开始被拖动：展开吸附状态并开始跟随光标。
pub fn drag_start(app: &AppHandle) {
    on_main(app, |app| {
        let Some(window) = app.get_webview_window(BALL_LABEL) else {
            return;
        };
        {
            let mut rt = runtime();
            if rt.dragging {
                return;
            }
            rt.dragging = true;
            // 拖动期间前端会忽略指针离开事件；这里主动清掉悬停状态，
            // 否则拖动结束后旧的 hover 标记会阻止收回边缘。
            rt.hover_ball = false;
            rt.hover_panel = false;
        }
        cancel_hover_hide();
        cancel_collapse();
        // 拖动要立刻拿到完整的 60 点窗口：取消进行中的过渡并立即铺开，
        // 否则动画和拖动循环会同时改窗口位置。
        cancel_ball_animation();
        hide_popover(&app);
        expand_ball(&app, &window, false);
        start_drag_loop(app.clone(), window);
    });
}

/// 松开鼠标：吸附到最近的左右边缘并保存位置。
pub fn drag_end(app: &AppHandle) {
    on_main(app, |app| {
        let Some(window) = app.get_webview_window(BALL_LABEL) else {
            return;
        };
        {
            let mut rt = runtime();
            if !rt.dragging {
                return;
            }
            rt.dragging = false;
        }
        finish_drag(&app, &window);
    });
}

/// 球或弹层的指针进出。两者任一保持悬停时不收起。
pub fn set_hover(app: &AppHandle, target: HoverTarget, hovered: bool) {
    on_main(app, move |app| {
        {
            let mut rt = runtime();
            match target {
                HoverTarget::Ball => rt.hover_ball = hovered,
                HoverTarget::Popover => rt.hover_panel = hovered,
            }
        }
        if hovered {
            cancel_hover_hide();
            cancel_collapse();
            if target == HoverTarget::Ball {
                let (edge, expanded, pending) = {
                    let rt = runtime();
                    (rt.edge, rt.expanded, rt.hover_show_pending)
                };
                if let Some(window) = app.get_webview_window(BALL_LABEL) {
                    expand_ball(&app, &window, true);
                }
                // 展开动画期间重复的 pointerenter/watchdog 通知共用同一个
                // 挂起请求，不会把详情卡提前弹出来。
                if runtime_popover_mode() != Some(PopoverMode::Menu) && !pending {
                    if hover_show_waits_for_expand(edge, expanded) {
                        schedule_hover_show(&app);
                    } else {
                        show_popover(&app, PopoverMode::Hover);
                    }
                }
            }
        } else {
            cancel_hover_show();
            schedule_hover_hide(app.clone());
            schedule_collapse(app.clone());
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverTarget {
    Ball,
    Popover,
}

/// 点击球：菜单未开时打开，已开时收起。
pub fn toggle_menu(app: &AppHandle) {
    on_main(app, |app| {
        if runtime_popover_mode() == Some(PopoverMode::Menu)
            && app
                .get_webview_window(POPOVER_LABEL)
                .and_then(|window| window.is_visible().ok())
                .unwrap_or(false)
        {
            hide_popover(&app);
            collapse_if_idle(&app);
            return;
        }
        // 菜单刚因点击外部收起：这次球点击就是那次“外部点击”，
        // 保持收起，不再弹出。
        if popover_blurred_recently() {
            return;
        }
        show_menu(&app);
    });
}

/// 详情卡里的“切换套餐”。
pub fn show_menu(app: &AppHandle) {
    on_main(app, |app| {
        if let Some(window) = app.get_webview_window(BALL_LABEL) {
            expand_ball(&app, &window, true);
        }
        show_popover(&app, PopoverMode::Menu);
    });
}

/// 弹层按内容高度调整尺寸并重新定位。
pub fn resize_popover(app: &AppHandle, height: f64) {
    if !height.is_finite() {
        return;
    }
    let height = height.clamp(POPOVER_MIN_HEIGHT, POPOVER_MAX_HEIGHT);
    let changed = {
        let mut rt = runtime();
        if (rt.popover_height - height).abs() < 0.5 {
            false
        } else {
            rt.popover_height = height;
            true
        }
    };
    if !changed {
        return;
    }
    on_main(app, |app| {
        if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
            if window.is_visible().unwrap_or(false) {
                place_popover(&app, &window);
            }
        }
    });
}

/// 弹层当前模式；页面加载完成后主动拉取一次，避免错过事件。
pub fn popover_mode() -> Option<String> {
    runtime_popover_mode().map(|mode| mode.as_str().to_string())
}

/// 弹层请求收起（Esc / 指针离开详情卡）。
pub fn hide(app: &AppHandle) {
    on_main(app, |app| {
        hide_popover(&app);
        collapse_if_idle(&app);
    });
}

fn runtime_popover_mode() -> Option<PopoverMode> {
    runtime().popover_mode
}

fn last_popover_blur() -> &'static Mutex<Option<Instant>> {
    LAST_POPOVER_BLUR.get_or_init(|| Mutex::new(None))
}

fn note_popover_blur() {
    if let Ok(mut guard) = last_popover_blur().lock() {
        *guard = Some(Instant::now());
    }
}

fn popover_blurred_recently() -> bool {
    last_popover_blur()
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|at| at.elapsed() < BLUR_CLICK_WINDOW)
}

// ---------- 窗口创建 ----------

fn ensure_ball(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = app.get_webview_window(BALL_LABEL) {
        if !BALL_LOADING.load(Ordering::SeqCst) && !window.is_visible().unwrap_or(false) {
            show_ball_when_ready(app, &window);
        }
        return Some(window);
    }
    if BALL_LOADING.load(Ordering::SeqCst) {
        return None;
    }

    let config = window_config(app, BALL_LABEL)?;
    let generation = BALL_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    BALL_LOADING.store(true, Ordering::SeqCst);
    cancel_ball_animation();
    load_runtime_from_state(app);

    let app_for_load = app.clone();
    lifecycle!("[float] 创建 floating-ball");
    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            BALL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[float] 读取 floating-ball 配置失败：{err}");
            return None;
        }
    };
    let window = match builder
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || BALL_GENERATION.load(Ordering::SeqCst) != generation
            {
                return;
            }
            BALL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[float] floating-ball 已加载");
            let app = app_for_load.clone();
            let window = window.clone();
            let _ = app_for_load.run_on_main_thread(move || show_ball_when_ready(&app, &window));
        })
        .build()
    {
        Ok(window) => window,
        Err(err) => {
            BALL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[float] 创建 floating-ball 失败：{err}");
            revert_enabled(app);
            return None;
        }
    };

    prepare_native_ball(&window);
    attach_ball_handlers(&window);
    start_hover_watch(app.clone(), generation);
    // 标题来自合并后的 productName：测试版窗口不应显示正式版名称。
    let _ = window.set_title(&app.package_info().name);
    let fallback = window.clone();
    let app_for_fallback = app.clone();
    thread::spawn(move || {
        thread::sleep(LOAD_FALLBACK);
        if BALL_GENERATION.load(Ordering::SeqCst) != generation
            || !BALL_LOADING.swap(false, Ordering::SeqCst)
        {
            return;
        }
        lifecycle!("[float] floating-ball 加载超时，直接显示");
        let target = app_for_fallback.clone();
        let _ = app_for_fallback.run_on_main_thread(move || {
            show_ball_when_ready(&target, &fallback);
        });
    });
    Some(window)
}

/// 窗口创建失败时把开关退回关闭，避免设置显示开启但界面没有球。
fn revert_enabled(app: &AppHandle) {
    let store = app.state::<Arc<StateStore>>();
    if let Err(err) = store.set_floating_ball_enabled(false) {
        lifecycle!("[float] 回退悬浮球开关失败：{err}");
    }
}

fn attach_ball_handlers(window: &WebviewWindow) {
    let window = window.clone();
    let target = window.clone();
    window.on_window_event(move |event| {
        // 球本身没有标题栏和关闭按钮；系统窗口菜单或快捷键发起的关闭
        // 按“关闭悬浮球”处理，避免留下一个看不到的常驻窗口。
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let app = target.app_handle().clone();
            if let Err(err) = app
                .state::<Arc<StateStore>>()
                .set_floating_ball_enabled(false)
            {
                lifecycle!("[float] 关闭悬浮球时保存开关失败：{err}");
            }
            // 不在当前窗口事件回调里就地销毁自己：转到后台线程再回主线程，
            // 避免 NSPanel 转换与销毁和正在派发的关闭事件重入。
            defer_to_main(&app, |app| destroy_float_windows(&app));
        }
    });
}

/// 按保存的位置放置并显示球；没有保存位置时使用右侧居中的默认位置。
fn show_ball_when_ready(app: &AppHandle, window: &WebviewWindow) {
    place_ball(app, window);
    show_native_ball(window);
    lifecycle!("[float] floating-ball 已显示");
}

fn place_ball(app: &AppHandle, window: &WebviewWindow) {
    let saved = app
        .state::<Arc<StateStore>>()
        .snapshot()
        .floating_ball
        .position;

    let monitor = saved
        .as_ref()
        .and_then(|position| find_monitor(app, window, position))
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    // 位置按目标显示器的缩放计算，避免多屏不同 DPI 时窗口落到半像素上。
    let scale = monitor.scale_factor();
    let size = BALL_SIZE * scale;
    let screen = screen_area(&monitor);
    let work = work_area(&monitor);
    let name = monitor.name().cloned();

    let (x, y, hidden_edge, clipped) = match &saved {
        Some(position) => {
            let y = clamp_y(position.y, work, size);
            match position.edge {
                // 吸附贴的是屏幕边缘而不是工作区：Dock 或任务栏存在时，
                // 工作区边缘会让球停在 Dock 外侧，看起来没有贴边。
                Some(edge) => {
                    let (x, hidden, clipped) =
                        snap_layout(app, &monitor, screen, edge, y, size, SNAP_VISIBLE * scale);
                    (x, y, hidden, clipped)
                }
                None => (clamp_x(position.x, work, size), y, false, false),
            }
        }
        None => {
            let x = clamp_x(
                work.x + work.width - size - SNAP_DISTANCE * scale,
                work,
                size,
            );
            let y = work.y + (work.height - size) / 2.0;
            (x, y, false, false)
        }
    };

    {
        let mut rt = runtime();
        rt.edge = saved.as_ref().and_then(|position| position.edge);
        rt.y = Some(y);
        rt.layout_x = Some(x);
        rt.monitor = name;
        rt.hidden_edge = hidden_edge;
        rt.expanded = false;
    }
    // 启动恢复没有过渡：窗口尚未显示，直接落到目标位置。
    cancel_ball_animation();
    set_ball_layout(window, x, y, clipped);
    emit_layout(app);
}

/// 悬停/打开菜单时展开吸附的球。
///
/// `animate=false` 只用于拖动开始：拖动循环需要立刻拿到完整窗口；
/// 其余情况用窗口过渡，内容和窗口同步变形。
fn expand_ball(app: &AppHandle, window: &WebviewWindow, animate: bool) {
    let (edge, y, expanded) = {
        let rt = runtime();
        (rt.edge, rt.y, rt.expanded)
    };
    let (Some(edge), Some(y)) = (edge, y) else {
        return;
    };
    if expanded {
        return;
    }
    let Some(monitor) = ball_monitor(app, window) else {
        return;
    };
    let scale = monitor.scale_factor();
    let size = BALL_SIZE * scale;
    let screen = screen_area(&monitor);
    let work = work_area(&monitor);
    let neighbor = monitor_has_neighbor(app, &monitor, edge, y, size);
    let x = expanded_x_at_cursor(app, edge, screen, size, EXPAND_GAP * scale);
    let target_y = clamp_y(y, work, size);
    {
        let mut rt = runtime();
        rt.hidden_edge = !neighbor;
        rt.expanded = true;
        rt.y = Some(target_y);
        rt.layout_x = Some(x);
    }
    emit_layout(app);
    if animate {
        animate_ball(window, x, target_y, BALL_SIZE, BALL_EXPAND, ease_out_back);
    } else {
        set_ball_layout(window, x, target_y, false);
    }
}

fn start_drag_loop(app: AppHandle, window: WebviewWindow) {
    thread::spawn(move || {
        // 收起状态的窗口只有 24 点宽，拖动前刚扩成 60 点；等一帧让
        // 尺寸与位置落到窗口系统，再记录拖动起点，避免首帧跳动。
        thread::sleep(DRAG_SETTLE);
        let (Ok(start), Ok(origin)) = (app.cursor_position(), window.outer_position()) else {
            return;
        };
        let started = Instant::now();
        loop {
            thread::sleep(DRAG_TICK);
            if !runtime().dragging || started.elapsed() > DRAG_TIMEOUT {
                break;
            }
            let Ok(cursor) = app.cursor_position() else {
                break;
            };
            let x = origin.x as f64 + (cursor.x - start.x);
            let y = origin.y as f64 + (cursor.y - start.y);
            let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
    });
}

/// 拖动结束：吸附到最近的边缘，或夹取到工作区内；位置写入状态文件。
fn finish_drag(app: &AppHandle, window: &WebviewWindow) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let Some(monitor) = window.current_monitor().ok().flatten() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let screen = screen_area(&monitor);
    let work = work_area(&monitor);
    let name = monitor.name().cloned();
    let x = position.x as f64;
    let y = position.y as f64;

    // 边缘判定与吸附都按完整屏幕计算；工作区只用于上下夹取与自由位置，
    // 避免球停在 Dock / 任务栏外侧。
    let edge = if x <= screen.x + SNAP_DISTANCE * scale {
        Some(FloatingBallEdge::Left)
    } else if x + size.width as f64 >= screen.x + screen.width - SNAP_DISTANCE * scale {
        Some(FloatingBallEdge::Right)
    } else {
        None
    };

    let (final_x, final_y, hidden_edge, clipped) = match edge {
        Some(edge) => {
            let (x, hidden, clipped) = snap_layout(
                app,
                &monitor,
                screen,
                edge,
                y,
                size.height as f64,
                SNAP_VISIBLE * scale,
            );
            (x, clamp_y(y, work, size.height as f64), hidden, clipped)
        }
        None => (
            clamp_x(x, work, size.width as f64),
            clamp_y(y, work, size.height as f64),
            false,
            false,
        ),
    };

    {
        let mut rt = runtime();
        rt.edge = edge;
        rt.y = Some(final_y);
        rt.layout_x = Some(final_x);
        rt.monitor = name.clone();
        rt.hidden_edge = hidden_edge;
        rt.expanded = false;
    }
    // 松手后从落点平滑滑到吸附位置；贴边后再收成 D 形。
    // 布局事件等到贴边后才发，球不会在半路上就显示成吸附状态。
    if let Some(edge) = edge {
        // 内部边界第一段停在贴边位置（完整 60 点仍在本屏内），第二段再收窄。
        let slid_x = if clipped {
            flush_x(edge, screen, size.width as f64)
        } else {
            final_x
        };
        animate_ball_collapse(app, window, slid_x, final_x, final_y, clipped);
    } else {
        emit_layout(app);
        animate_ball(
            window,
            final_x,
            final_y,
            BALL_SIZE,
            BALL_DROP,
            ease_out_cubic,
        );
    }

    let saved = FloatingBallPosition {
        x: final_x,
        y: final_y,
        edge,
        monitor: name,
    };
    if let Err(err) = app
        .state::<Arc<StateStore>>()
        .set_floating_ball_position(Some(saved))
    {
        lifecycle!("[float] 保存悬浮球位置失败：{err}");
    }
    lifecycle!("[float] 悬浮球位置已保存 edge={edge:?}");
}

fn load_runtime_from_state(app: &AppHandle) {
    let position = app
        .state::<Arc<StateStore>>()
        .snapshot()
        .floating_ball
        .position;
    let mut rt = runtime();
    rt.edge = position.as_ref().and_then(|value| value.edge);
    rt.y = position.as_ref().map(|value| value.y);
    rt.layout_x = position.as_ref().map(|value| value.x);
    rt.monitor = position.and_then(|value| value.monitor);
    rt.hidden_edge = false;
    rt.expanded = false;
    rt.dragging = false;
    rt.hover_ball = false;
    rt.hover_panel = false;
    rt.hover_show_pending = false;
    rt.popover_mode = None;
    rt.shown_mode = None;
    rt.popover_height = HOVER_HEIGHT;
}

fn monitor_named(app: &AppHandle, name: Option<&str>) -> Option<tauri::Monitor> {
    let name = name?;
    app.available_monitors()
        .ok()?
        .into_iter()
        .find(|monitor| monitor.name().is_some_and(|value| value == name))
}

/// 球所在显示器：优先按保存的名称，其次按窗口当前所在显示器。
/// 吸附后窗口有一部分在屏幕外，`current_monitor` 可能拿不到，名称更可靠。
fn ball_monitor(app: &AppHandle, window: &WebviewWindow) -> Option<tauri::Monitor> {
    let name = runtime().monitor.clone();
    monitor_named(app, name.as_deref())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

fn find_monitor(
    app: &AppHandle,
    window: &WebviewWindow,
    saved: &FloatingBallPosition,
) -> Option<tauri::Monitor> {
    if let Some(found) = monitor_named(app, saved.monitor.as_deref()) {
        return Some(found);
    }
    // 显示器名称不可用时，用球的可见部分探测目标显示器：隐藏式吸附的
    // 窗口有一部分在屏幕外，直接取左上角可能落在相邻屏幕或空区域。
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = BALL_SIZE * scale;
    let visible = SNAP_VISIBLE * scale;
    let probe_x = match saved.edge {
        Some(FloatingBallEdge::Right) => saved.x + visible / 2.0,
        Some(FloatingBallEdge::Left) => saved.x + size - visible / 2.0,
        None => saved.x + size / 2.0,
    };
    app.monitor_from_point(probe_x, saved.y + size / 2.0)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
}

// ---------- 弹层 ----------

fn show_popover(app: &AppHandle, mode: PopoverMode) {
    // 悬停通知可能同时来自前端 pointerenter 与光标看门狗；同一模式已经
    // 显示时直接复用，避免重复派发模式事件、重新测量并重新摆位。
    if runtime_popover_mode() == Some(mode)
        && app
            .get_webview_window(POPOVER_LABEL)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false)
    {
        return;
    }
    {
        let mut rt = runtime();
        rt.popover_height = popover_height_for(mode, rt.shown_mode, rt.popover_height);
        rt.shown_mode = Some(mode);
        rt.popover_mode = Some(mode);
    }

    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        if POPOVER_LOADING.load(Ordering::SeqCst) || !POPOVER_LOADED.load(Ordering::SeqCst) {
            return;
        }
        present_popover(app, &window, mode);
        return;
    }
    create_popover(app);
}

fn create_popover(app: &AppHandle) {
    let Some(config) = window_config(app, POPOVER_LABEL) else {
        return;
    };
    let generation = POPOVER_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    POPOVER_LOADING.store(true, Ordering::SeqCst);
    POPOVER_LOADED.store(false, Ordering::SeqCst);
    let app_for_load = app.clone();
    lifecycle!("[float] 创建 float-popover");
    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            POPOVER_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[float] 读取 float-popover 配置失败：{err}");
            return;
        }
    };
    let window = match builder
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || POPOVER_GENERATION.load(Ordering::SeqCst) != generation
            {
                return;
            }
            POPOVER_LOADING.store(false, Ordering::SeqCst);
            POPOVER_LOADED.store(true, Ordering::SeqCst);
            lifecycle!("[float] float-popover 已加载");
            let app = app_for_load.clone();
            let window = window.clone();
            let _ = app_for_load.run_on_main_thread(move || {
                let Some(mode) = runtime_popover_mode() else {
                    return;
                };
                present_popover(&app, &window, mode);
            });
        })
        .build()
    {
        Ok(window) => window,
        Err(err) => {
            POPOVER_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[float] 创建 float-popover 失败：{err}");
            return;
        }
    };

    prepare_native_popover(&window);
    attach_popover_handlers(&window);
    let _ = window.set_title(&app.package_info().name);
    // 首帧后前端会按内容高度调用 resize；这里先按模式默认高度显示。
    let fallback = window.clone();
    let app_for_fallback = app.clone();
    thread::spawn(move || {
        thread::sleep(LOAD_FALLBACK);
        if POPOVER_GENERATION.load(Ordering::SeqCst) != generation
            || !POPOVER_LOADING.swap(false, Ordering::SeqCst)
        {
            return;
        }
        POPOVER_LOADED.store(true, Ordering::SeqCst);
        lifecycle!("[float] float-popover 加载超时，直接显示");
        let target = app_for_fallback.clone();
        let _ = app_for_fallback.run_on_main_thread(move || {
            let Some(mode) = runtime_popover_mode() else {
                return;
            };
            present_popover(&target, &fallback, mode);
        });
    });
}

fn present_popover(app: &AppHandle, window: &WebviewWindow, mode: PopoverMode) {
    let _ = app.emit_to(POPOVER_LABEL, POPOVER_MODE_EVENT, mode.as_str());
    place_popover(app, window);
    show_native_popover(window, mode == PopoverMode::Menu);
    lifecycle!("[float] float-popover 已显示 mode={}", mode.as_str());
}

fn attach_popover_handlers(window: &WebviewWindow) {
    let window = window.clone();
    let target = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            if runtime_popover_mode() == Some(PopoverMode::Menu) {
                note_popover_blur();
                let app = target.app_handle().clone();
                hide_popover(&app);
                collapse_if_idle(&app);
            }
        }
    });
}

fn place_popover(app: &AppHandle, window: &WebviewWindow) {
    let Some(ball) = app.get_webview_window(BALL_LABEL) else {
        return;
    };
    let Some(monitor) = ball_monitor(app, &ball) else {
        return;
    };
    let scale = ball.scale_factor().unwrap_or(1.0);
    // 球正在做吸附/展开过渡时窗口还在逐帧移动，弹层按最近一次布局的
    // 目标位置定位，避免卡片跟着动画抖一下再回位。
    let (ball_x, ball_y, ball_width) = {
        let rt = runtime();
        match (rt.layout_x, rt.y) {
            (Some(x), Some(y)) => (x, y, BALL_SIZE * scale),
            _ => {
                let (Ok(position), Ok(size)) = (ball.outer_position(), ball.outer_size()) else {
                    return;
                };
                (position.x as f64, position.y as f64, size.width as f64)
            }
        }
    };
    let work = work_area(&monitor);
    let height = runtime().popover_height;
    let panel_width = POPOVER_WIDTH * scale;
    let panel_height = height * scale;
    let gap = POPOVER_GAP * scale;
    let margin = POPOVER_MARGIN * scale;

    let ball_center = ball_x + ball_width / 2.0;
    let open_left = ball_center > work.x + work.width / 2.0;
    let raw_x = if open_left {
        ball_x - panel_width - gap
    } else {
        ball_x + ball_width + gap
    };
    let x = raw_x.clamp(
        work.x + margin,
        (work.x + work.width - panel_width - margin).max(work.x + margin),
    );
    let raw_y = ball_y + BALL_SIZE * scale / 2.0 - 20.0 * scale;
    let y = raw_y.clamp(
        work.y + margin,
        (work.y + work.height - panel_height - margin).max(work.y + margin),
    );

    let _ = window.set_size(LogicalSize::new(POPOVER_WIDTH, height));
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn hide_popover(app: &AppHandle) {
    {
        let mut rt = runtime();
        rt.popover_mode = None;
        // 弹层被隐藏后不会再收到 pointerleave；清掉悬停标记，
        // 避免下一次停留被旧的 hover_panel=true 阻塞。
        rt.hover_panel = false;
        rt.hover_show_pending = false;
    }
    HOVER_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        }
    }
}

fn destroy_float_windows(app: &AppHandle) {
    hide_popover(app);
    cancel_hover_hide();
    cancel_hover_show();
    cancel_collapse();
    cancel_ball_animation();
    POPOVER_LOADING.store(false, Ordering::SeqCst);
    POPOVER_LOADED.store(false, Ordering::SeqCst);
    BALL_LOADING.store(false, Ordering::SeqCst);
    BALL_GENERATION.fetch_add(1, Ordering::SeqCst);
    POPOVER_GENERATION.fetch_add(1, Ordering::SeqCst);

    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        restore_native(&window);
        let _ = window.destroy();
    }
    if let Some(window) = app.get_webview_window(BALL_LABEL) {
        restore_native(&window);
        let _ = window.destroy();
    }
    *runtime() = Runtime {
        popover_height: HOVER_HEIGHT,
        ..Runtime::default()
    };
    lifecycle!("[float] 悬浮球已销毁");
}

// ---------- 悬停与吸附计时 ----------

fn schedule_hover_hide(app: AppHandle) {
    let generation = HOVER_HIDE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        thread::sleep(HOVER_HIDE_DELAY);
        if HOVER_HIDE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let (hover_ball, hover_panel) = {
            let rt = runtime();
            (rt.hover_ball, rt.hover_panel)
        };
        if hover_ball || hover_panel {
            return;
        }
        let target = app.clone();
        let _ = app.run_on_main_thread(move || {
            if runtime_popover_mode() == Some(PopoverMode::Hover) {
                hide_popover(&target);
            }
        });
    });
}

fn schedule_collapse(app: AppHandle) {
    let generation = COLLAPSE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        thread::sleep(COLLAPSE_DELAY);
        if COLLAPSE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let target = app.clone();
        let _ = app.run_on_main_thread(move || collapse_if_idle(&target));
    });
}

fn collapse_if_idle(app: &AppHandle) {
    let should_collapse = {
        let rt = runtime();
        rt.edge.is_some()
            && rt.expanded
            && !rt.hover_ball
            && !rt.hover_panel
            && rt.popover_mode.is_none()
    };
    if !should_collapse {
        return;
    }
    let Some(window) = app.get_webview_window(BALL_LABEL) else {
        return;
    };
    let (edge, y) = {
        let rt = runtime();
        (rt.edge, rt.y)
    };
    let (Some(edge), Some(y)) = (edge, y) else {
        return;
    };
    let Some(monitor) = ball_monitor(app, &window) else {
        return;
    };
    let scale = monitor.scale_factor();
    let size = BALL_SIZE * scale;
    let screen = screen_area(&monitor);
    let work = work_area(&monitor);
    let (x, hidden, clipped) =
        snap_layout(app, &monitor, screen, edge, y, size, SNAP_VISIBLE * scale);
    let final_y = clamp_y(y, work, size);
    {
        let mut rt = runtime();
        rt.hidden_edge = hidden;
        rt.expanded = false;
        rt.y = Some(final_y);
        rt.layout_x = Some(x);
    }
    // 先滑到边缘、贴边后再切布局收成 D 形，避免在离边缘还有一段距离时
    // 就变成吸附的样子。布局事件由收起动画在贴边后发出。
    let slid_x = if clipped {
        flush_x(edge, screen, size)
    } else {
        x
    };
    animate_ball_collapse(app, &window, slid_x, x, final_y, clipped);
    lifecycle!("[float] 悬浮球收回边缘");
}

fn cancel_hover_hide() {
    HOVER_HIDE_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn cancel_collapse() {
    COLLAPSE_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// 悬停详情卡是否要等球的展开过渡结束。只有吸附收起态的球会先展开；
/// 自由位置或已经展开的球没有过渡，可以立即显示。
fn hover_show_waits_for_expand(edge: Option<FloatingBallEdge>, expanded: bool) -> bool {
    edge.is_some() && !expanded
}

/// 挂起一次详情卡显示：等吸附球完全展开、指针仍停在完整球体上才显示。
///
/// 展开期间重复的悬停通知共享这一次请求，不会让详情卡在过渡中途弹出。
fn schedule_hover_show(app: &AppHandle) {
    let generation = {
        let mut rt = runtime();
        if rt.hover_show_pending {
            return;
        }
        rt.hover_show_pending = true;
        HOVER_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
    };
    let handle = app.clone();
    thread::spawn(move || {
        // 比窗口过渡多留一帧，确保详情卡出现时球已经到位。
        thread::sleep(BALL_EXPAND + BALL_ANIMATION_TICK);
        let target = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let show = {
                let mut rt = runtime();
                // 期间指针离开、菜单接管或窗口销毁都会取消这次请求；
                // 代号不匹配说明已经有更新的请求，不能清掉它的挂起标记。
                if !rt.hover_show_pending
                    || HOVER_SHOW_GENERATION.load(Ordering::SeqCst) != generation
                {
                    false
                } else {
                    rt.hover_show_pending = false;
                    rt.hover_ball
                }
            };
            if !show || runtime_popover_mode() == Some(PopoverMode::Menu) {
                return;
            }
            // 离开球后的宽限期内 hover 标记可能还没清掉，以光标位置为准。
            let Some(ball) = target.get_webview_window(BALL_LABEL) else {
                return;
            };
            let Ok(cursor) = target.cursor_position() else {
                return;
            };
            if !window_contains(&ball, cursor.x, cursor.y) {
                return;
            }
            show_popover(&target, PopoverMode::Hover);
        });
    });
}

fn cancel_hover_show() {
    let mut rt = runtime();
    rt.hover_show_pending = false;
    HOVER_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// 看门狗需要补发的“进入”通知：上一轮已经停在球/卡片上时不重复通知，
/// 否则每一轮轮询都会重新展开窗口、重开弹层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HoverEnters {
    ball: bool,
    panel: bool,
}

fn hover_enters(
    seen_ball: bool,
    seen_panel: bool,
    over_ball: bool,
    over_popover: bool,
) -> HoverEnters {
    HoverEnters {
        ball: over_ball && !seen_ball,
        panel: over_popover && !seen_panel,
    }
}

/// 光标是否落在窗口的矩形范围内。
fn window_contains(window: &WebviewWindow, x: f64, y: f64) -> bool {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return false;
    };
    x >= position.x as f64
        && x <= position.x as f64 + size.width as f64
        && y >= position.y as f64
        && y <= position.y as f64 + size.height as f64
}

/// 悬浮球的悬停看门狗。
///
/// macOS 上球和弹层都是非激活 NSPanel，悬停事件受两个限制：
/// - 鼠标移动/悬停事件只派发给 key 窗口，球在点击取得 key 之前收不到
///   `pointerenter`，表现为“悬停没反应，点一下才开始响应”；
/// - 菜单取得 key 焦点后，球窗口可能不再收到 `pointerleave`，前端的
///   `hover=false` 永远不来，球会停在展开状态、不再收回边缘。
///
/// 因此这里同时兜底进入与离开：光标移到球或详情卡上时按位置补发悬停，
/// 离开超过宽限期后清掉过期的悬停标记，并按同样的 1 秒宽限期收回。
fn start_hover_watch(app: AppHandle, generation: u64) {
    thread::spawn(move || {
        let mut outside_since: Option<Instant> = None;
        // 光标上一轮是否已经停在球/卡片上；只在“从外到内”的跨越时补发悬停。
        let mut over_ball_seen = false;
        let mut over_popover_seen = false;
        loop {
            thread::sleep(HOVER_WATCH_TICK);
            if BALL_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(ball) = app.get_webview_window(BALL_LABEL) else {
                return;
            };
            let (dragging, expanded, mode) = {
                let rt = runtime();
                (rt.dragging, rt.expanded, rt.popover_mode)
            };
            // 拖动中光标跟着窗口走；菜单打开期间以失焦/Esc/再次点击为准。
            // 两者结束时指针通常仍停在球上：要求先移出再移入才算新的悬停，
            // 否则松手或关掉菜单会立刻把球重新展开并弹出详情卡。
            if dragging || mode == Some(PopoverMode::Menu) {
                outside_since = None;
                over_ball_seen = true;
                over_popover_seen = true;
                continue;
            }
            // 球创建后、显示前不要按默认位置触发悬停。
            if !ball.is_visible().unwrap_or(false) {
                outside_since = None;
                continue;
            }
            let Ok(cursor) = app.cursor_position() else {
                continue;
            };
            let over_ball = window_contains(&ball, cursor.x, cursor.y);
            let over_popover = mode == Some(PopoverMode::Hover)
                && app
                    .get_webview_window(POPOVER_LABEL)
                    .filter(|window| window.is_visible().unwrap_or(false))
                    .is_some_and(|window| window_contains(&window, cursor.x, cursor.y));
            let enters = hover_enters(over_ball_seen, over_popover_seen, over_ball, over_popover);
            over_ball_seen = over_ball;
            over_popover_seen = over_popover;
            if enters.ball {
                set_hover(&app, HoverTarget::Ball, true);
            }
            if enters.panel {
                set_hover(&app, HoverTarget::Popover, true);
            }
            if over_ball || over_popover {
                outside_since = None;
                continue;
            }

            let outside = *outside_since.get_or_insert(Instant::now());
            if outside.elapsed() >= HOVER_HIDE_DELAY {
                let had_hover = {
                    let rt = runtime();
                    rt.hover_ball || rt.hover_panel
                };
                // 详情卡可能还显示着（前端的 pointerleave 同样可能丢失），
                // 光标已经不在球和卡片上就一并收起。
                if had_hover || mode == Some(PopoverMode::Hover) {
                    let target = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        {
                            let mut rt = runtime();
                            rt.hover_ball = false;
                            rt.hover_panel = false;
                            rt.hover_show_pending = false;
                        }
                        if runtime_popover_mode() == Some(PopoverMode::Hover) {
                            hide_popover(&target);
                        }
                    });
                }
            }
            if expanded && outside.elapsed() >= COLLAPSE_DELAY {
                let target = app.clone();
                let _ = app.run_on_main_thread(move || collapse_if_idle(&target));
            }
        }
    });
}

// ---------- 纯几何计算 ----------

#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkArea {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn work_area(monitor: &tauri::Monitor) -> WorkArea {
    let area = monitor.work_area();
    WorkArea {
        x: area.position.x as f64,
        y: area.position.y as f64,
        width: area.size.width as f64,
        height: area.size.height as f64,
    }
}

/// 完整屏幕（包含菜单栏、Dock / 任务栏占用的区域）；吸附按它计算。
fn screen_area(monitor: &tauri::Monitor) -> WorkArea {
    let position = monitor.position();
    let size = monitor.size();
    WorkArea {
        x: position.x as f64,
        y: position.y as f64,
        width: size.width as f64,
        height: size.height as f64,
    }
}

/// 吸附后只保留 `visible` 宽度：左侧贴边时窗口向左移动大部分到屏幕外。
fn snapped_x(edge: FloatingBallEdge, work: WorkArea, size: f64, visible: f64) -> f64 {
    match edge {
        FloatingBallEdge::Left => work.x - (size - visible),
        FloatingBallEdge::Right => work.x + work.width - visible,
    }
}

/// 悬停展开后离边缘留出 `gap`。
fn expanded_x(edge: FloatingBallEdge, work: WorkArea, size: f64, gap: f64) -> f64 {
    match edge {
        FloatingBallEdge::Left => work.x + gap,
        FloatingBallEdge::Right => work.x + work.width - gap - size,
    }
}

/// 相邻显示器存在时只贴边，球完整留在当前屏幕内。
fn flush_x(edge: FloatingBallEdge, screen: WorkArea, size: f64) -> f64 {
    match edge {
        FloatingBallEdge::Left => screen.x,
        FloatingBallEdge::Right => screen.x + screen.width - size,
    }
}

/// 当前显示器在该方向是否还有另一块屏幕。
///
/// 只有虚拟桌面最外侧的边缘才允许把球藏到屏幕外；两块屏幕之间的内部边界
/// 如果把球推出当前屏幕，露出的部分会直接显示在相邻屏幕上。这里检查的是
/// “球被藏起来的那一段”与所有其他显示器屏幕区域是否相交，因此也能覆盖
/// 显示器之间留有间隙或只在一部分高度上相邻的排列。
fn monitor_has_neighbor(
    app: &AppHandle,
    monitor: &tauri::Monitor,
    edge: FloatingBallEdge,
    ball_top: f64,
    ball_size: f64,
) -> bool {
    let screen = screen_area(monitor);
    let scale = monitor.scale_factor();
    let hidden = ball_size - SNAP_VISIBLE * scale;
    if hidden <= 0.0 {
        return false;
    }
    let band_x = match edge {
        FloatingBallEdge::Left => screen.x - hidden,
        FloatingBallEdge::Right => screen.x + screen.width,
    };
    let Ok(monitors) = app.available_monitors() else {
        return false;
    };
    monitors.into_iter().any(|other| {
        // 当前显示器自身的边界与隐藏带只接触、不相交，跳过。
        if other.position() == monitor.position() && other.size() == monitor.size() {
            return false;
        }
        let area = screen_area(&other);
        overlap_len(band_x, hidden, area.x, area.width) > 0.0
            && overlap_len(ball_top, ball_size, area.y, area.height) > 0.0
    })
}

fn overlap_len(a: f64, a_len: f64, b: f64, b_len: f64) -> f64 {
    (a + a_len).min(b + b_len) - a.max(b)
}

/// 吸附布局：返回窗口 x、是否把球藏到屏幕外、是否使用 24 点内部收起窗口。
/// `visible` 是收起后希望保留的物理宽度。
fn snap_layout(
    app: &AppHandle,
    monitor: &tauri::Monitor,
    screen: WorkArea,
    edge: FloatingBallEdge,
    y: f64,
    size: f64,
    visible: f64,
) -> (f64, bool, bool) {
    if monitor_has_neighbor(app, monitor, edge, y, size) {
        // 内部边界：窗口缩到 visible 宽，完整留在当前屏幕内，靠前端把球裁成 D 形。
        (flush_x(edge, screen, visible), false, true)
    } else {
        (snapped_x(edge, screen, size, visible), true, false)
    }
}

/// 悬停展开位置。鼠标已经落在贴边的一小段时不要留 12 点间隙，
/// 否则展开会把球从鼠标下面移走；判断用包含边界，避免鼠标刚好停在
/// 间隙边缘时反复“展开 → 移出 → 收起”。
fn expanded_x_at_cursor(
    app: &AppHandle,
    edge: FloatingBallEdge,
    screen: WorkArea,
    size: f64,
    gap: f64,
) -> f64 {
    let default = expanded_x(edge, screen, size, gap);
    let Ok(cursor) = app.cursor_position() else {
        return default;
    };
    match edge {
        FloatingBallEdge::Left if cursor.x <= screen.x + gap => screen.x,
        FloatingBallEdge::Right if cursor.x >= screen.x + screen.width - gap => {
            screen.x + screen.width - size
        }
        _ => default,
    }
}

fn clamp_x(x: f64, work: WorkArea, size: f64) -> f64 {
    x.clamp(work.x, (work.x + work.width - size).max(work.x))
}

fn clamp_y(y: f64, work: WorkArea, size: f64) -> f64 {
    y.clamp(work.y, (work.y + work.height - size).max(work.y))
}

/// 设置窗口宽度并按逻辑尺寸调整；内部收起时宽度为 24 点。
fn set_ball_layout(window: &WebviewWindow, x: f64, y: f64, clipped: bool) {
    let _ = window.set_size(LogicalSize::new(collapsed_width(clipped), BALL_SIZE));
    set_ball_position(window, x, y);
}

fn set_ball_position(window: &WebviewWindow, x: f64, y: f64) {
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// 收起后的窗口逻辑宽度：内部边界缩到 24 点；最外侧边缘保留 60 点，
/// 靠把窗口移出屏幕只露出 24 点。
fn collapsed_width(clipped: bool) -> f64 {
    if clipped {
        SNAP_VISIBLE
    } else {
        BALL_SIZE
    }
}

/// 取消进行中的球窗口过渡。
fn cancel_ball_animation() {
    BALL_ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// ease-out cubic：起步快、收尾稳，贴边时不会显得拖沓。
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// 带轻微回弹的 ease-out：末段越过目标约 7% 再落回，弹出手感更“活”。
///
/// 只用于朝屏幕内侧或已确认没有相邻显示器的位移：越过贴边位置会把球
/// 露到相邻屏幕上。
fn ease_out_back(t: f64) -> f64 {
    const C1: f64 = 1.4;
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + (C1 + 1.0) * t * t * t + C1 * t * t
}

/// 执行一段球窗口补间；返回 `false` 表示被新动画、拖动或窗口销毁打断。
fn tween_ball(
    window: &WebviewWindow,
    generation: u64,
    x: f64,
    y: f64,
    width: f64,
    duration: Duration,
    easing: fn(f64) -> f64,
) -> bool {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return false;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return false;
    }
    let start_x = position.x as f64;
    let start_y = position.y as f64;
    let start_width = size.width as f64 / scale;
    let started = Instant::now();
    loop {
        if BALL_ANIMATION_GENERATION.load(Ordering::SeqCst) != generation {
            return false;
        }
        let progress = started.elapsed().as_secs_f64() / duration.as_secs_f64();
        let eased = easing(progress);
        let frame_x = start_x + (x - start_x) * eased;
        let frame_y = start_y + (y - start_y) * eased;
        let frame_width = start_width + (width - start_width) * eased;
        let _ = window.set_size(LogicalSize::new(frame_width, BALL_SIZE));
        let _ = window.set_position(PhysicalPosition::new(
            frame_x.round() as i32,
            frame_y.round() as i32,
        ));
        if progress >= 1.0 {
            return true;
        }
        thread::sleep(BALL_ANIMATION_TICK);
    }
}

/// 把球窗口平滑移动到目标位置，并把宽度过渡到 `width`（逻辑点）。
///
/// 新动画、拖动开始与窗口销毁都会让旧动画在下一次循环时退出。
fn animate_ball(
    window: &WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    duration: Duration,
    easing: fn(f64) -> f64,
) {
    let generation = BALL_ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let window = window.clone();
    thread::spawn(move || {
        tween_ball(&window, generation, x, y, width, duration, easing);
    });
}

/// 吸附收起：先保持完整外观滑到边缘，贴边后再切换布局并收窄窗口。
///
/// 位移与形变放在同一段里时，球在离边缘还有一段距离就已经是 D 形，
/// 看起来像“还没贴边就吸附了”。拆成两段后形变只发生在贴边之后。
fn animate_ball_collapse(
    app: &AppHandle,
    window: &WebviewWindow,
    slid_x: f64,
    final_x: f64,
    y: f64,
    clipped: bool,
) {
    let generation = BALL_ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let window = window.clone();
    let app = app.clone();
    thread::spawn(move || {
        // 外侧边缘可以稍微越过落点再弹回来；内部边界越过就会露到相邻
        // 显示器上，只用普通的缓出。
        let slide = if clipped {
            ease_out_cubic
        } else {
            ease_out_back
        };
        if !tween_ball(
            &window,
            generation,
            slid_x,
            y,
            BALL_SIZE,
            BALL_SNAP_SLIDE,
            slide,
        ) {
            return;
        }
        let target = app.clone();
        let win = window.clone();
        let _ = app.run_on_main_thread(move || {
            // 滑向边缘期间可能已经开始新的展开或拖动，过期的收起不能再切布局。
            if BALL_ANIMATION_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            emit_layout(&target);
            // 内侧边界的窗口收窄与前端变形同时进行；外侧边缘只靠移出屏幕，
            // 窗口保持 60 点。
            thread::spawn(move || {
                thread::sleep(BALL_SNAP_LEAD);
                tween_ball(
                    &win,
                    generation,
                    final_x,
                    y,
                    collapsed_width(clipped),
                    BALL_SNAP_MORPH,
                    ease_out_cubic,
                );
            });
        });
    });
}

// ---------- 平台适配 ----------

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
use macos::{
    prepare_native_ball, prepare_native_popover, restore_native, show_native_ball,
    show_native_popover,
};

#[cfg(windows)]
use windows::{
    prepare_native_ball, prepare_native_popover, restore_native, show_native_ball,
    show_native_popover,
};

#[cfg(not(any(target_os = "macos", windows)))]
fn prepare_native_ball(_window: &WebviewWindow) {}
#[cfg(not(any(target_os = "macos", windows)))]
fn prepare_native_popover(_window: &WebviewWindow) {}
#[cfg(not(any(target_os = "macos", windows)))]
fn show_native_ball(window: &WebviewWindow) {
    let _ = window.show();
}
#[cfg(not(any(target_os = "macos", windows)))]
fn show_native_popover(window: &WebviewWindow, focus: bool) {
    let _ = window.show();
    if focus {
        let _ = window.set_focus();
    }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn restore_native(_window: &WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> WorkArea {
        WorkArea {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        }
    }

    #[test]
    fn snapped_ball_leaves_exactly_the_visible_strip() {
        let left = snapped_x(FloatingBallEdge::Left, work(), 60.0, 24.0);
        let right = snapped_x(FloatingBallEdge::Right, work(), 60.0, 24.0);
        assert_eq!(left, -36.0);
        assert_eq!(right, 976.0);
    }

    #[test]
    fn expanded_ball_keeps_the_design_gap() {
        assert_eq!(expanded_x(FloatingBallEdge::Left, work(), 60.0, 12.0), 12.0);
        assert_eq!(
            expanded_x(FloatingBallEdge::Right, work(), 60.0, 12.0),
            928.0
        );
    }

    #[test]
    fn internal_edges_stay_fully_inside_the_screen() {
        // 相邻显示器之间的边界不把球推出当前屏幕，避免出现在另一块屏幕上。
        assert_eq!(flush_x(FloatingBallEdge::Left, work(), 60.0), 0.0);
        assert_eq!(flush_x(FloatingBallEdge::Right, work(), 60.0), 940.0);
        // 内部边界收起时窗口只有 24 点宽，贴边位置按 24 点计算。
        assert_eq!(flush_x(FloatingBallEdge::Left, work(), 24.0), 0.0);
        assert_eq!(flush_x(FloatingBallEdge::Right, work(), 24.0), 976.0);
    }

    #[test]
    fn free_position_is_clamped_into_the_work_area() {
        assert_eq!(clamp_x(-20.0, work(), 60.0), 0.0);
        assert_eq!(clamp_x(980.0, work(), 60.0), 940.0);
        assert_eq!(clamp_y(-5.0, work(), 60.0), 0.0);
        assert_eq!(clamp_y(900.0, work(), 60.0), 740.0);
    }

    #[test]
    fn collapsed_width_keeps_the_peek_strip_only_on_internal_edges() {
        // 内部边界缩窗口，前端在同一窗口里只画露出的 24 点。
        assert_eq!(collapsed_width(true), 24.0);
        // 最外侧边缘窗口保持 60 点，靠移出屏幕只露出 24 点。
        assert_eq!(collapsed_width(false), 60.0);
    }

    #[test]
    fn animation_easing_starts_and_ends_exactly() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        // 越界进度被夹取，动画循环不会给出超过目标的位置。
        assert_eq!(ease_out_cubic(-0.5), 0.0);
        assert_eq!(ease_out_cubic(1.5), 1.0);
        // ease-out：前半段已经走过一半以上。
        assert!(ease_out_cubic(0.5) > 0.5);
    }

    #[test]
    fn back_easing_settles_on_target_with_a_small_overshoot() {
        assert_eq!(ease_out_back(0.0), 0.0);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-9);
        assert_eq!(ease_out_back(-0.5), 0.0);
        assert!((ease_out_back(1.5) - 1.0).abs() < 1e-9);
        // 回弹控制在 10% 以内：展开只往屏幕内侧越界，不会明显偏离落点。
        let peak = (0..=100)
            .map(|step| ease_out_back(step as f64 / 100.0))
            .fold(0.0_f64, f64::max);
        assert!(peak > 1.02 && peak < 1.10, "peak={peak}");
    }

    #[test]
    fn hover_enter_is_inferred_from_the_cursor_only_once() {
        // 非 key 的 NSPanel 收不到 pointerenter：光标进入球时补发一次。
        assert_eq!(
            hover_enters(false, false, true, false),
            HoverEnters {
                ball: true,
                panel: false
            }
        );
        // 已经处于悬停状态时，轮询不能反复展开/重开弹层。
        assert_eq!(
            hover_enters(true, false, true, false),
            HoverEnters {
                ball: false,
                panel: false
            }
        );
        // 光标从球移到详情卡：补发卡片的进入，避免它按“离开球”收起。
        assert_eq!(
            hover_enters(true, false, false, true),
            HoverEnters {
                ball: false,
                panel: true
            }
        );
        assert_eq!(
            hover_enters(false, true, false, true),
            HoverEnters {
                ball: false,
                panel: false
            }
        );
        // 光标不在两个窗口上时不产生进入通知。
        assert_eq!(
            hover_enters(false, false, false, false),
            HoverEnters {
                ball: false,
                panel: false
            }
        );
    }

    #[test]
    fn hover_card_waits_only_while_a_snapped_ball_expands() {
        // 吸附收起态会先播放展开过渡，详情卡必须等球完整显示。
        assert!(hover_show_waits_for_expand(
            Some(FloatingBallEdge::Right),
            false
        ));
        assert!(hover_show_waits_for_expand(
            Some(FloatingBallEdge::Left),
            false
        ));
        // 已经展开或自由位置没有窗口过渡，可以立即显示。
        assert!(!hover_show_waits_for_expand(
            Some(FloatingBallEdge::Right),
            true
        ));
        assert!(!hover_show_waits_for_expand(None, false));
    }

    #[test]
    fn reopening_the_same_popover_mode_keeps_measured_height() {
        // 首次打开或切换模式时先用默认高度，等前端报告内容高度。
        assert_eq!(
            popover_height_for(PopoverMode::Hover, None, HOVER_HEIGHT),
            HOVER_HEIGHT
        );
        assert_eq!(
            popover_height_for(PopoverMode::Menu, Some(PopoverMode::Hover), 312.0),
            MENU_HEIGHT
        );
        // 同一模式重新打开时沿用前端已测的高度，不能压回默认值。
        assert_eq!(
            popover_height_for(PopoverMode::Hover, Some(PopoverMode::Hover), 312.0),
            312.0
        );
    }

    #[test]
    fn selection_keys_are_unambiguous() {
        // 校验只依赖前缀与分隔符；这里固定格式，避免前端/Rust 两侧漂移。
        assert!(validate_key_shape("product:codex-local:codex-subscription"));
        assert!(validate_key_shape("relay:relay-1720000000000-1"));
        assert!(!validate_key_shape("product:missing-separator"));
        assert!(!validate_key_shape("unknown:x"));
    }

    fn validate_key_shape(key: &str) -> bool {
        if let Some(rest) = key.strip_prefix("product:") {
            let mut parts = rest.split(':');
            return parts.next().is_some_and(|value| !value.is_empty())
                && parts.next().is_some_and(|value| !value.is_empty())
                && parts.next().is_none();
        }
        key.strip_prefix("relay:")
            .is_some_and(|id| !id.is_empty() && !id.contains(':'))
    }
}
