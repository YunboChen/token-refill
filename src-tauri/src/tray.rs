//! 菜单栏常驻与快速查看面板。
//!
//! 菜单栏图标左键切换一个无边框面板窗口（label 为 `tray-panel`），
//! 面板定位在图标下方（Windows 在任务栏上方），失焦后自动隐藏。
//! 右键菜单提供打开主窗口、查看额度与退出；关闭主窗口只隐藏，
//! 应用继续在菜单栏驻留并刷新。
//!
//! 窗口里的 WebView 是常驻内存的大头，因此两条窗口都按需加载、按空闲销毁：
//! `tray-panel` 在 `tauri.conf.json` 中是 `create: false`，第一次点开时才创建；
//! 面板与主窗口隐藏 60 秒后销毁 WebView（保留进程），再次打开时重建。
//! 重建期间窗口保持隐藏，等页面加载完成（或 1.5 秒兜底）后再显示，避免白屏。
//!
//! Windows 上重建窗口是在"零 WebView"状态下冷启动 WebView2，耗时明显长于首次
//! 启动，因此这里的每条路径都必须保证“点了就一定会出现窗口”：创建请求一律
//! 投递到后台线程再回主线程（不在托盘回调里就地创建），加载标志在创建入口
//! 处判断（不重复创建），并给显示补一次延迟重试。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::WindowConfig,
    webview::{PageLoadEvent, WebviewWindowBuilder},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, Size, WebviewWindow,
    WindowEvent,
};

use crate::logging::lifecycle;
use crate::state::StateStore;
use crate::update::UpdateStore;

#[cfg(target_os = "macos")]
mod macos;

pub const MAIN_LABEL: &str = "main";
pub const PANEL_LABEL: &str = "tray-panel";
/// 面板打开后广播给前端，用于按刷新间隔补查中转站额度。
pub const PANEL_SHOWN_EVENT: &str = "tray-panel-shown";

/// 面板与菜单栏 / 任务栏之间留出的距离，避免贴边。
const PANEL_GAP: f64 = 6.0;
/// 面板与屏幕左右边缘的最小距离。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const EDGE_MARGIN: f64 = 8.0;
/// 面板的逻辑尺寸（点）；macOS 直接使用，其他平台按目标显示器缩放。
const PANEL_WIDTH: f64 = 380.0;
const PANEL_HEIGHT: f64 = 560.0;
/// 失焦后紧接着的托盘点击通常就是“关闭面板”，不再立即重新打开。
const BLUR_CLICK_WINDOW: Duration = Duration::from_millis(250);
/// 面板显示后的失焦宽限期。Windows 冷启动显示面板时，前台激活可能晚于首帧，
/// 期间收到的失焦事件不代表用户点了别处，立刻隐藏会让面板“一闪而过”。
const PANEL_BLUR_GRACE: Duration = Duration::from_millis(400);
/// 面板显示后没拿到激活态时的补焦间隔（macOS 面板不补焦）。
#[cfg_attr(target_os = "macos", allow(dead_code))]
const FOCUS_RETRY: Duration = Duration::from_millis(120);

/// 窗口隐藏后保留多久再销毁 WebView。只销毁窗口，不结束进程。
const WINDOW_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 空闲销毁的检查间隔。
const IDLE_TICK: Duration = Duration::from_secs(2);
/// 加载事件迟迟不来时直接显示窗口的兜底时间，避免窗口永远隐藏。
const LOAD_FALLBACK: Duration = Duration::from_millis(1500);
/// 兜底显示后窗口仍不可见时的重试间隔（只补一次）。
const SHOW_RETRY: Duration = Duration::from_secs(2);

static LAST_PANEL_BLUR: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
/// 面板最近一次显示的时间；用于忽略“刚显示就失焦”的杂散事件。
static PANEL_SHOWN_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
/// 面板自上次显示以来是否真的拿到过激活态。没有拿到过就不按失焦自动收起：
/// Windows 的前台锁会让托盘点击唤出的窗口停在未激活状态，此时收起等于点了没反应。
static PANEL_HAD_FOCUS: AtomicBool = AtomicBool::new(false);
/// 当前是否期望面板可见。延迟重试只在这个意图仍然成立时补显示，
/// 否则用户刚手动收起的面板会被“重试”重新弹出来。
static PANEL_WANT_VISIBLE: AtomicBool = AtomicBool::new(false);
/// 主窗口同理：关闭（隐藏）后就不能再被兜底重试显示。
static MAIN_WANT_VISIBLE: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct IdleWindows {
    main_deadline: Option<Instant>,
    panel_deadline: Option<Instant>,
}

static IDLE_WINDOWS: OnceLock<Mutex<IdleWindows>> = OnceLock::new();
/// 面板本次创建后是否还在等首帧；加载期间的点击只更新待显示位置。
static PANEL_LOADING: AtomicBool = AtomicBool::new(false);
/// 面板创建代次：旧的加载回调或兜底线程不能操作后来重建的窗口。
static PANEL_GENERATION: AtomicU64 = AtomicU64::new(0);
static PENDING_PANEL_RECT: OnceLock<Mutex<Option<Rect>>> = OnceLock::new();
/// 主窗口是否正在按配置重建；加载期间的重复“打开”请求不提前显示窗口。
static MAIN_LOADING: AtomicBool = AtomicBool::new(false);
static MAIN_GENERATION: AtomicU64 = AtomicU64::new(0);

fn idle_windows() -> &'static Mutex<IdleWindows> {
    IDLE_WINDOWS.get_or_init(Default::default)
}

fn pending_panel_rect() -> &'static Mutex<Option<Rect>> {
    PENDING_PANEL_RECT.get_or_init(Default::default)
}

fn lock_idle() -> std::sync::MutexGuard<'static, IdleWindows> {
    idle_windows()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_pending_rect() -> std::sync::MutexGuard<'static, Option<Rect>> {
    pending_panel_rect()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn last_blur() -> &'static Mutex<Option<Instant>> {
    LAST_PANEL_BLUR.get_or_init(|| Mutex::new(None))
}

/// 面板因失焦隐藏时记录时间，供托盘点击判断“这次点击是在关闭面板”。
pub fn note_panel_blur() {
    if let Ok(mut guard) = last_blur().lock() {
        *guard = Some(Instant::now());
    }
}

fn blurred_recently() -> bool {
    last_blur()
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|at| at.elapsed() < BLUR_CLICK_WINDOW)
}

fn panel_shown_at() -> &'static Mutex<Option<Instant>> {
    PANEL_SHOWN_AT.get_or_init(|| Mutex::new(None))
}

/// 记录面板刚刚显示；显示后的短窗口内忽略失焦。
fn note_panel_shown() {
    if let Ok(mut guard) = panel_shown_at().lock() {
        *guard = Some(Instant::now());
    }
}

fn since_panel_shown() -> Option<Duration> {
    panel_shown_at()
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .map(|at| at.elapsed())
}

/// 失焦时是否应当自动收起面板。
///
/// `had_focus` 为 false 表示窗口自显示后从未真正获得过激活态（Windows 前台锁
/// 下托盘点击唤出的窗口可能停在未激活状态），此时的失焦既可能是杂散消息，
/// 也不代表用户点了别处；刚显示不久的失焦同理。两种情况都不收起窗口，
/// 让用户至少能再点一次托盘图标关闭它，而不是看到面板闪一下就消失。
fn should_hide_on_blur(visible: bool, had_focus: bool, since_shown: Option<Duration>) -> bool {
    if !visible || !had_focus {
        return false;
    }
    !since_shown.is_some_and(|elapsed| elapsed < PANEL_BLUR_GRACE)
}

/// 面板相对菜单栏 / 任务栏的方向：macOS 向下展开，Windows 向上展开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelPlacement {
    BelowIcon,
    AboveIcon,
}

fn panel_placement() -> PanelPlacement {
    if cfg!(target_os = "macos") {
        PanelPlacement::BelowIcon
    } else {
        PanelPlacement::AboveIcon
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn from_rect(rect: &Rect, scale: f64) -> Self {
        let position = rect.position.to_physical::<f64>(scale);
        let size = rect.size.to_physical::<f64>(scale);
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }

    fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// 计算面板左上角的物理坐标。`work_area` 是图标所在显示器去掉菜单栏 /
/// 任务栏后的可用区域；取不到显示器时不做边界收敛。
fn panel_origin(
    icon: Bounds,
    panel: (f64, f64),
    work_area: Option<Bounds>,
    placement: PanelPlacement,
) -> (i32, i32) {
    let (panel_width, panel_height) = panel;

    let (mut x, mut y) = if icon.is_empty() {
        // 个别平台首次点击不提供图标矩形：贴工作区右上 / 右下角。
        match (work_area, placement) {
            (Some(work), PanelPlacement::BelowIcon) => (
                work.x + work.width - panel_width - PANEL_GAP,
                work.y + PANEL_GAP,
            ),
            (Some(work), PanelPlacement::AboveIcon) => (
                work.x + work.width - panel_width - PANEL_GAP,
                work.y + work.height - panel_height - PANEL_GAP,
            ),
            (None, PanelPlacement::BelowIcon) => (PANEL_GAP, PANEL_GAP),
            (None, PanelPlacement::AboveIcon) => (PANEL_GAP, PANEL_GAP),
        }
    } else {
        let center_x = icon.x + icon.width / 2.0;
        let y = match placement {
            PanelPlacement::BelowIcon => icon.y + icon.height + PANEL_GAP,
            PanelPlacement::AboveIcon => icon.y - panel_height - PANEL_GAP,
        };
        (center_x - panel_width / 2.0, y)
    };

    if let Some(work) = work_area {
        // 面板比可用区域还大时，至少保证左上角可见。
        let max_x = (work.x + work.width - panel_width).max(work.x);
        let max_y = (work.y + work.height - panel_height).max(work.y);
        x = x.clamp(work.x, max_x);
        y = y.clamp(work.y, max_y);
    }

    (x.round() as i32, y.round() as i32)
}

/// 把任务排到主线程执行，但不在当前回调里就地执行。
///
/// 托盘与窗口事件回调本身就在主线程上，`run_on_main_thread` 会直接内联执行；
/// 而创建窗口要等 WebView2 初始化（零 WebView 状态下冷启动可达数百毫秒，
/// 内部还会自行泵消息）。就地执行会卡住托盘菜单，并让重入的点击走进创建分支。
/// 先转到一个后台线程再回主线程，事件回调就能立刻返回。
fn defer_to_main<F>(app: &AppHandle, task: F)
where
    F: FnOnce(AppHandle) + Send + 'static,
{
    let handle = app.clone();
    thread::spawn(move || {
        let target = handle.clone();
        if let Err(err) = handle.run_on_main_thread(move || task(target)) {
            lifecycle!("[window] 投递主线程任务失败：{err}");
        }
    });
}

/// 打开快速查看面板。macOS 用 Cocoa 原生坐标系定位（多显示器与
/// Retina/非 Retina 混排时 Tauri 的物理坐标不可靠），其他平台按
/// 托盘图标矩形与显示器可用区域计算。
pub fn show_panel(app: &AppHandle, rect: Rect) {
    if PANEL_LOADING.load(Ordering::SeqCst) {
        // 正在创建：只更新位置，由加载回调或兜底负责显示，不重复创建。
        *lock_pending_rect() = Some(rect);
        return;
    }
    defer_to_main(app, move |app| show_panel_request(&app, rect));
}

/// 在主线程处理一次“打开面板”请求；窗口不存在时按配置创建。
fn show_panel_request(app: &AppHandle, rect: Rect) {
    // 先判断加载标志：创建期间窗口还没登记进管理器，若先查窗口就会重复创建，
    // 重复创建会自增代次，让真正在加载的那个窗口永远等不到显示回调。
    if PANEL_LOADING.load(Ordering::SeqCst) {
        *lock_pending_rect() = Some(rect);
        return;
    }

    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        show_panel_now(app, &panel, rect);
        return;
    }

    *lock_pending_rect() = Some(rect);
    if create_panel(app).is_none() {
        *lock_pending_rect() = None;
        lifecycle!("[window] 创建 tray-panel 失败，本次点击未生效");
    }
}

/// 按 `tauri.conf.json` 里的配置创建面板（该窗口是 `create: false`）。
/// 先以隐藏状态创建，等页面加载完成后再显示，避免透明窗口出现空白。
fn create_panel(app: &AppHandle) -> Option<WebviewWindow> {
    let config = window_config(app, PANEL_LABEL)?;
    let started = Instant::now();
    let generation = PANEL_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    PANEL_LOADING.store(true, Ordering::SeqCst);

    let app_for_load = app.clone();
    lifecycle!("[window] 创建 tray-panel（无 WebView 窗口可用）");
    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            PANEL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[window] 读取 tray-panel 配置失败：{err}");
            return None;
        }
    };
    let panel = match builder
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || PANEL_GENERATION.load(Ordering::SeqCst) != generation
            {
                return;
            }
            PANEL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!(
                "[window] tray-panel 加载耗时 {} ms",
                started.elapsed().as_millis()
            );
            let pending = lock_pending_rect().take();
            if let Some(rect) = pending {
                let app = app_for_load.clone();
                let _ =
                    app_for_load.run_on_main_thread(move || show_panel_now(&app, &window, rect));
            } else {
                // 加载期间用户取消了这次打开：窗口保持隐藏，交给空闲销毁回收。
                schedule_idle_destroy(PANEL_LABEL);
            }
        })
        .build()
    {
        Ok(panel) => panel,
        Err(err) => {
            PANEL_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[window] 创建 tray-panel 失败：{err}");
            return None;
        }
    };

    #[cfg(target_os = "macos")]
    if let Err(err) = macos::prepare(&panel) {
        PANEL_LOADING.store(false, Ordering::SeqCst);
        let _ = panel.destroy();
        lifecycle!("[window] 创建原生快速面板失败：{err}");
        return None;
    }

    attach_panel_handlers(&panel);
    start_load_fallback(app, &panel, generation);
    Some(panel)
}

/// 页面加载事件没来（资源损坏、渲染进程异常）时也要让面板出现，
/// 否则窗口会一直隐藏，用户会以为托盘图标失效。
fn start_load_fallback(app: &AppHandle, panel: &WebviewWindow, generation: u64) {
    let fallback = panel.clone();
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(LOAD_FALLBACK);
        if PANEL_GENERATION.load(Ordering::SeqCst) != generation
            || !PANEL_LOADING.swap(false, Ordering::SeqCst)
        {
            return;
        }
        lifecycle!("[window] tray-panel 加载超时，直接显示");
        if let Some(rect) = lock_pending_rect().take() {
            let handle = app.clone();
            let window = fallback.clone();
            let _ = app.run_on_main_thread(move || show_panel_now(&handle, &window, rect));
            // 显示请求投递后仍不可见（窗口刚建好、消息还没轮到）时补一次，
            // 避免用户点了托盘却看不到任何反馈。用户已取消（没有待显示位置）时不补。
            retry_show_panel(fallback);
        } else {
            schedule_idle_destroy(PANEL_LABEL);
        }
    });
}

fn attach_panel_handlers(panel: &WebviewWindow) {
    let window = panel.clone();
    panel.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            PANEL_HAD_FOCUS.store(true, Ordering::SeqCst);
        }
        WindowEvent::Focused(false) => {
            let had_focus = PANEL_HAD_FOCUS.load(Ordering::SeqCst);
            if !should_hide_on_blur(
                window.is_visible().unwrap_or(false),
                had_focus,
                since_panel_shown(),
            ) {
                // 没真正拿到过焦点：这多半是 Windows 前台锁下的杂散失焦，
                // 收起会让左键点击看起来完全没反应，保持显示。
                return;
            }
            PANEL_HAD_FOCUS.store(false, Ordering::SeqCst);
            PANEL_WANT_VISIBLE.store(false, Ordering::SeqCst);
            let _ = window.hide();
            note_panel_blur();
            schedule_idle_destroy(PANEL_LABEL);
            lifecycle!("[window] tray-panel 失焦收起");
        }
        _ => {}
    });
}

fn show_panel_now(app: &AppHandle, panel: &WebviewWindow, rect: Rect) {
    cancel_idle_destroy(PANEL_LABEL);
    PANEL_WANT_VISIBLE.store(true, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    let placed = place_panel_on_macos(panel).unwrap_or(false);
    #[cfg(not(target_os = "macos"))]
    let placed = false;

    if !placed {
        place_panel_generic(app, panel, &rect);
    }

    #[cfg(target_os = "macos")]
    if macos::show(panel).is_err() {
        // 插件里还没有这个窗口：这次显示无效，撤销可见意图，避免兜底重试反复弹空窗口。
        PANEL_WANT_VISIBLE.store(false, Ordering::SeqCst);
        lifecycle!("[window] 显示原生快速面板失败：窗口尚未注册");
        return;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = panel.show();
        let _ = panel.set_focus();
    }

    // 记录显示时间与激活状态：随后的杂散失焦不应立刻收起面板。
    note_panel_shown();
    #[cfg(target_os = "macos")]
    {
        // 非激活 NSPanel 由 `macos::show` 里的 makeKeyWindow 拿到 key 焦点；
        // 这里不能再用 Tauri 的 set_focus，那会激活整个应用并切回主桌面空间。
        PANEL_HAD_FOCUS.store(true, Ordering::SeqCst);
    }
    #[cfg(not(target_os = "macos"))]
    {
        PANEL_HAD_FOCUS.store(panel.is_focused().unwrap_or(false), Ordering::SeqCst);
        retry_panel_focus(panel);
    }
    lifecycle!("[window] tray-panel 已显示");
    let _ = app.emit(PANEL_SHOWN_EVENT, ());
}

/// 面板跨桌面空间显示，并允许覆盖其他应用的全屏空间；保留无关窗口行为。
#[cfg(target_os = "macos")]
fn panel_collection_behavior(
    mut behavior: objc2_app_kit::NSWindowCollectionBehavior,
) -> objc2_app_kit::NSWindowCollectionBehavior {
    use objc2_app_kit::NSWindowCollectionBehavior as Behavior;

    behavior.remove(
        Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary | Behavior::FullScreenNone,
    );
    behavior.insert(Behavior::CanJoinAllSpaces | Behavior::FullScreenAuxiliary);
    behavior
}

/// 显示后没拿到激活态时补一次聚焦。仍失败也不隐藏窗口：
/// 窗口是置顶的，用户能看到内容，再点一次托盘图标即可收起。
/// 仅用于非 macOS：macOS 的面板是非激活 NSPanel，补焦会激活应用并切换桌面空间。
#[cfg(not(target_os = "macos"))]
fn retry_panel_focus(panel: &WebviewWindow) {
    if panel.is_focused().unwrap_or(false) {
        return;
    }
    let window = panel.clone();
    thread::spawn(move || {
        thread::sleep(FOCUS_RETRY);
        if !window.is_visible().unwrap_or(false) || window.is_focused().unwrap_or(false) {
            return;
        }
        lifecycle!("[window] tray-panel 未能获得焦点，补一次");
        let app = window.app_handle().clone();
        let _ = app.run_on_main_thread(move || {
            let _ = window.set_focus();
        });
    });
}

/// 按平台显示面板：macOS 走非激活 NSPanel，其他平台 show + 聚焦。
fn reveal_panel(window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    let _ = macos::show(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 兜底显示后窗口仍不可见时补一次显示，只补一次，避免与正在进行的重建互相覆盖。
fn retry_show_panel(window: WebviewWindow) {
    thread::spawn(move || {
        thread::sleep(SHOW_RETRY);
        if !PANEL_WANT_VISIBLE.load(Ordering::SeqCst) || window.is_visible().unwrap_or(true) {
            return;
        }
        lifecycle!("[window] tray-panel 仍不可见，重试显示");
        let app = window.app_handle().clone();
        let _ = app.run_on_main_thread(move || reveal_panel(&window));
    });
}

/// macOS：以鼠标位置所在屏幕为基准，面板贴在菜单栏下方、水平对齐菜单栏图标。
/// 鼠标此刻就在被点击的托盘图标上，比解析托盘事件里的混合坐标更可靠。
#[cfg(target_os = "macos")]
fn place_panel_on_macos(panel: &tauri::WebviewWindow) -> tauri::Result<bool> {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
    use objc2_foundation::NSPoint;

    let Some(mtm) = MainThreadMarker::new() else {
        return Ok(false);
    };

    let pointer = panel.ns_window()? as *mut NSWindow;
    // SAFETY: 已确认当前在主线程，`ns_window` 返回由 panel 持有的有效窗口指针。
    let window = unsafe { &*pointer };
    // 仅置顶不能进入其他应用的全屏空间；必须在显示和聚焦前配置空间行为。
    window.setCollectionBehavior(panel_collection_behavior(window.collectionBehavior()));

    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let matched: Option<Retained<NSScreen>> = screens.iter().find(|screen| {
        let frame = screen.frame();
        mouse.x >= frame.origin.x
            && mouse.x <= frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y <= frame.origin.y + frame.size.height
    });
    let Some(screen) = matched.or_else(|| NSScreen::mainScreen(mtm)) else {
        return Ok(false);
    };

    // visibleFrame 已排除菜单栏与 Dock；面板顶边贴在菜单栏下沿。
    let visible = screen.visibleFrame();
    let min_x = visible.origin.x + EDGE_MARGIN;
    let max_x = visible.origin.x + visible.size.width - PANEL_WIDTH - EDGE_MARGIN;
    let x = (mouse.x - PANEL_WIDTH / 2.0).clamp(min_x, max_x.max(min_x));

    // `setFrameTopLeftPoint` 的 y 是顶边；visible 的顶边已经不含菜单栏。
    // 屏幕特别矮时面板会向下溢出，但顶边仍需贴着菜单栏。
    let top = visible.origin.y + visible.size.height - PANEL_GAP;

    window.setFrameTopLeftPoint(NSPoint::new(x, top));
    Ok(true)
}

/// 其他平台：按托盘图标矩形与目标显示器可用区域计算物理像素位置。
fn place_panel_generic(app: &AppHandle, panel: &tauri::WebviewWindow, rect: &Rect) {
    let scale = panel.scale_factor().unwrap_or(1.0);
    let icon = Bounds::from_rect(rect, scale);
    let anchor = if icon.is_empty() {
        None
    } else {
        Some((icon.x + icon.width / 2.0, icon.y + icon.height / 2.0))
    };
    let monitor = anchor
        .and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let monitor_scale = monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(scale);
    let work_area = monitor.as_ref().map(|monitor| {
        let area = monitor.work_area();
        Bounds {
            x: area.position.x as f64,
            y: area.position.y as f64,
            width: area.size.width as f64,
            height: area.size.height as f64,
        }
    });

    let (x, y) = panel_origin(
        icon,
        (PANEL_WIDTH * monitor_scale, PANEL_HEIGHT * monitor_scale),
        work_area,
        panel_placement(),
    );
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

fn toggle_panel(app: &AppHandle, rect: Rect) {
    if PANEL_LOADING.load(Ordering::SeqCst) {
        // 正在创建时又点了一次：视为取消，清掉待显示位置，
        // 加载完成后保持隐藏（窗口本身仍会在空闲后销毁）。
        *lock_pending_rect() = None;
        lifecycle!("[window] tray-panel 加载中再次点击，取消本次显示");
        return;
    }
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        if panel.is_visible().unwrap_or(false) {
            PANEL_WANT_VISIBLE.store(false, Ordering::SeqCst);
            let _ = panel.hide();
            note_panel_blur();
            schedule_idle_destroy(PANEL_LABEL);
            lifecycle!("[window] tray-panel 再次点击收起");
            return;
        }
    }
    if blurred_recently() {
        return;
    }
    show_panel(app, rect);
}

/// 面板的显式收起入口（Esc / 命令）。隐藏后进入空闲销毁倒计时。
pub fn hide_panel(app: &AppHandle) {
    *lock_pending_rect() = None;
    PANEL_WANT_VISIBLE.store(false, Ordering::SeqCst);
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let _ = panel.hide();
        schedule_idle_destroy(PANEL_LABEL);
    }
}

/// 把主窗口带到前台；面板若打开则先收起，避免两个窗口同时盖在界面上。
/// 主窗口已按空闲策略销毁时在这里重建。
pub fn show_main_window(app: &AppHandle) {
    defer_to_main(app, |app| show_main_window_now(&app));
}

fn show_main_window_now(app: &AppHandle) {
    hide_panel(app);
    cancel_idle_destroy(MAIN_LABEL);
    MAIN_WANT_VISIBLE.store(true, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        // 关闭主窗口时应用已退成附件形态；必须切回常规形态再显示窗口，否则
        // 窗口拿不到 key 焦点，也没有应用菜单栏。应用被 Cmd+H 隐藏时一并取消。
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }
    // 正在重建：加载完成后会自动显示；这里提前 show 会出现白屏。
    // 标志必须在查窗口之前判断，创建期间窗口还没登记进管理器。
    if MAIN_LOADING.load(Ordering::SeqCst) {
        lifecycle!("[window] 主窗口正在重建，忽略重复的打开请求");
        return;
    }
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        show_main_now(app, &main);
        return;
    }
    if create_main_window(app).is_none() {
        lifecycle!("[window] 重建主窗口失败，本次打开未生效");
    }
}

fn show_main_now(_app: &AppHandle, window: &WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let _ = _app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = _app.show();
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    lifecycle!(
        "[window] 显示主窗口 visible={} focused={}",
        window.is_visible().unwrap_or(false),
        window.is_focused().unwrap_or(false)
    );
}

/// 空闲销毁后重建主窗口：先隐藏创建，加载完成后由 `on_page_load` 显示。
fn create_main_window(app: &AppHandle) -> Option<WebviewWindow> {
    let config = window_config(app, MAIN_LABEL)?;
    let started = Instant::now();
    let generation = MAIN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    MAIN_LOADING.store(true, Ordering::SeqCst);
    let app_for_load = app.clone();
    let shown = Arc::new(AtomicBool::new(false));
    let shown_for_load = Arc::clone(&shown);

    lifecycle!("[window] 重建主窗口（无 WebView 窗口可用）");
    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            MAIN_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[window] 读取主窗口配置失败：{err}");
            return None;
        }
    };
    // Windows 由前端自绘标题栏：重建的窗口同样要去掉系统边框，否则空闲销毁
    // 后再打开会重新露出系统标题栏。启动时的窗口在 `setup()` 里已关过装饰。
    #[cfg(windows)]
    let builder = builder.decorations(false);
    let main = match builder
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || MAIN_GENERATION.load(Ordering::SeqCst) != generation
            {
                return;
            }
            MAIN_LOADING.store(false, Ordering::SeqCst);
            if shown_for_load.swap(true, Ordering::SeqCst) {
                return;
            }
            lifecycle!(
                "[window] 主窗口重建加载耗时 {} ms",
                started.elapsed().as_millis()
            );
            let app = app_for_load.clone();
            let window = window.clone();
            let _ = app_for_load.run_on_main_thread(move || show_main_now(&app, &window));
        })
        .build()
    {
        Ok(main) => main,
        Err(err) => {
            MAIN_LOADING.store(false, Ordering::SeqCst);
            lifecycle!("[window] 重建主窗口失败：{err}");
            return None;
        }
    };

    attach_main_handlers(&main);
    let fallback = main.clone();
    thread::spawn(move || {
        thread::sleep(LOAD_FALLBACK);
        if MAIN_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        MAIN_LOADING.store(false, Ordering::SeqCst);
        if shown.swap(true, Ordering::SeqCst) || !MAIN_WANT_VISIBLE.load(Ordering::SeqCst) {
            // 已经显示过，或用户在重建期间又把它关掉了：不再兜底弹出。
            return;
        }
        lifecycle!("[window] 主窗口加载超时，直接显示");
        show_main_on_main_thread(&fallback);
        retry_show_main(fallback);
    });
    Some(main)
}

/// 在兜底路径上显示主窗口：窗口的显示操作必须回到主线程执行。
fn show_main_on_main_thread(window: &WebviewWindow) {
    let handle = window.app_handle().clone();
    let window = window.clone();
    let _ = handle.run_on_main_thread(move || {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    });
}

/// 兜底显示后仍不可见时补一次，只补一次，避免与正在进行的重建互相覆盖。
fn retry_show_main(window: WebviewWindow) {
    thread::spawn(move || {
        thread::sleep(SHOW_RETRY);
        if !MAIN_WANT_VISIBLE.load(Ordering::SeqCst) || window.is_visible().unwrap_or(true) {
            return;
        }
        lifecycle!("[window] 主窗口仍不可见，重试显示");
        show_main_on_main_thread(&window);
    });
}

/// 关闭主窗口只隐藏并进入空闲销毁倒计时；由该窗口自己的事件处理，
/// 因此每次重建后都要重新挂一次。
fn attach_main_handlers(target: &WebviewWindow) {
    let window = target.clone();
    target.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            MAIN_WANT_VISIBLE.store(false, Ordering::SeqCst);
            let _ = window.hide();
            #[cfg(target_os = "macos")]
            {
                // 先隐藏窗口再切形态，避免附件形态下窗口仍处于 key 状态。
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            schedule_idle_destroy(MAIN_LABEL);
        }
    });
}

/// 从 Tauri 配置里取窗口定义；窗口是按配置按需创建的，删掉配置会返回 None。
fn window_config(app: &AppHandle, label: &str) -> Option<WindowConfig> {
    app.config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .cloned()
}

fn schedule_idle_destroy(label: &'static str) {
    schedule_idle_destroy_at(label, Instant::now() + WINDOW_IDLE_TIMEOUT);
}

fn schedule_idle_destroy_at(label: &'static str, deadline: Instant) {
    let mut state = lock_idle();
    match label {
        MAIN_LABEL => state.main_deadline = Some(deadline),
        PANEL_LABEL => state.panel_deadline = Some(deadline),
        _ => {}
    }
}

fn cancel_idle_destroy(label: &'static str) {
    let mut state = lock_idle();
    match label {
        MAIN_LABEL => state.main_deadline = None,
        PANEL_LABEL => state.panel_deadline = None,
        _ => {}
    }
}

/// 取出已到期的窗口标签；到期的条目同时从状态里移除。
fn take_due(now: Instant) -> Vec<&'static str> {
    let mut state = lock_idle();
    let mut due = Vec::new();
    if take_deadline(&mut state.main_deadline, now) {
        due.push(MAIN_LABEL);
    }
    if take_deadline(&mut state.panel_deadline, now) {
        due.push(PANEL_LABEL);
    }
    due
}

fn take_deadline(deadline: &mut Option<Instant>, now: Instant) -> bool {
    if deadline.is_some_and(|value| now >= value) {
        *deadline = None;
        true
    } else {
        false
    }
}

/// 单个后台线程处理两个窗口的空闲销毁：每两秒检查一次，到期且仍隐藏时才销毁。
fn start_idle_destroyer(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(IDLE_TICK);
        for label in take_due(Instant::now()) {
            if !destroy_idle_window(&app, label) {
                // 有登录 / 下载在进行：这一轮不销毁，稍后再试。
                schedule_idle_destroy(label);
            }
        }
    });
}

/// 返回 true 表示这次已经处理完（销毁或无需销毁）；false 表示应当推迟。
fn destroy_idle_window(app: &AppHandle, label: &'static str) -> bool {
    if label == MAIN_LABEL && windows_busy(app) {
        return false;
    }
    let Some(window) = app.get_webview_window(label) else {
        return true;
    };
    if window.is_visible().unwrap_or(false) {
        return true;
    }
    let app_for_task = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_for_task.get_webview_window(label) {
            if !window.is_visible().unwrap_or(false) {
                lifecycle!("[window] 销毁空闲窗口 {label}");
                #[cfg(target_os = "macos")]
                if label == PANEL_LABEL {
                    macos::restore(&window);
                }
                let _ = window.destroy();
            }
        }
    });
    true
}

/// 主窗口前端持有登录弹窗与更新下载进度；这两类流程进行期间不销毁它。
fn windows_busy(app: &AppHandle) -> bool {
    let login = app.state::<Arc<StateStore>>().has_active_login();
    let update = app.state::<UpdateStore>().is_downloading();
    login || update
}

/// 通过菜单打开面板时没有图标矩形，用鼠标位置作为锚点。
fn cursor_rect(app: &AppHandle) -> Rect {
    let Ok(cursor) = app.cursor_position() else {
        // 取不到鼠标位置时交给通用定位贴到工作区角落。
        return Rect::default();
    };
    Rect {
        // 给一个 1px 的矩形，让它走“以图标中心对齐”的分支。
        position: tauri::Position::Physical(PhysicalPosition::new(
            cursor.x.round() as i32,
            cursor.y.round() as i32,
        )),
        size: Size::Physical(PhysicalSize::new(1, 1)),
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.plugin(tauri_nspanel::init())?;

    // 关闭主窗口只隐藏：应用继续在菜单栏驻留并后台刷新；隐藏满 60 秒后
    // 销毁 WebView，下次打开时重建。macOS 同时退成附件形态，Dock 图标、
    // Cmd+Tab 与应用菜单栏一并消失，只留菜单栏图标。
    if let Some(main) = app.get_webview_window(MAIN_LABEL) {
        attach_main_handlers(&main);
    }

    // `tray-panel` 不再随启动创建（配置里是 create: false），这里只启动
    // 空闲销毁器，负责到点释放两个窗口的 WebView。
    start_idle_destroyer(app.clone());

    let app_name = app.package_info().name.clone();
    let open_main = MenuItem::with_id(app, "open-main", "打开主窗口", true, None::<&str>)?;
    let view_quota = MenuItem::with_id(app, "view-quota", "查看额度", true, None::<&str>)?;
    let quit_label = format!("退出 {app_name}");
    let quit = MenuItem::with_id(app, "quit", &quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_main,
            &view_quota,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("token-refill-tray")
        .tooltip(app_name)
        .menu(&menu)
        // 左键用于快速查看，右键才展开菜单。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-main" => {
                lifecycle!("[tray] 菜单：打开主窗口");
                show_main_window(app);
            }
            "view-quota" => {
                lifecycle!("[tray] 菜单：查看额度");
                let rect = cursor_rect(app);
                show_panel(app, rect);
            }
            "quit" => {
                lifecycle!("[tray] 菜单：退出");
                app.exit(0);
            }
            other => lifecycle!("[tray] 未处理的菜单项 {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            let TrayIconEvent::Click {
                rect,
                button,
                button_state,
                ..
            } = event
            else {
                return;
            };
            // 这条日志同时是“事件循环还活着”的信号：点了托盘没有新日志，
            // 说明主线程被阻塞，而不是窗口创建失败。
            lifecycle!("[tray] 点击 {button:?}/{button_state:?}");
            if matches!(button, MouseButton::Left) && matches!(button_state, MouseButtonState::Up) {
                toggle_panel(tray.app_handle(), rect);
            }
        });

    #[cfg(target_os = "macos")]
    {
        // 模板图标只保留 alpha 轮廓，菜单栏会按浅色/深色自动反色。
        // 测试版用带右下圆点的变体，两个版本同时常驻时可以一眼区分。
        let icon = if crate::channel::is_test(app) {
            include_bytes!("../icons/tray-test-template.png").as_slice()
        } else {
            include_bytes!("../icons/tray-template.png").as_slice()
        };
        builder = builder
            .icon(tauri::image::Image::from_bytes(icon)?)
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
    }

    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon_bounds(x: f64, y: f64) -> Bounds {
        Bounds {
            x,
            y,
            width: 24.0,
            height: 24.0,
        }
    }

    fn work_area() -> Bounds {
        Bounds {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_panel_can_join_desktop_and_fullscreen_spaces() {
        use objc2_app_kit::NSWindowCollectionBehavior as Behavior;

        for initial in [
            Behavior::Default,
            Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary,
            Behavior::FullScreenNone,
        ] {
            let behavior = panel_collection_behavior(initial);
            assert!(behavior.contains(Behavior::CanJoinAllSpaces));
            assert!(behavior.contains(Behavior::FullScreenAuxiliary));
            assert!(!behavior.intersects(
                Behavior::MoveToActiveSpace
                    | Behavior::FullScreenPrimary
                    | Behavior::FullScreenNone
            ));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_panel_space_behavior_preserves_other_flags_and_is_idempotent() {
        use objc2_app_kit::NSWindowCollectionBehavior as Behavior;

        let unrelated = Behavior::Transient | Behavior::IgnoresCycle;
        let behavior = panel_collection_behavior(unrelated);
        assert!(behavior.contains(unrelated));
        assert_eq!(panel_collection_behavior(behavior), behavior);
    }

    #[test]
    fn panel_is_centered_below_the_icon_on_macos() {
        let (x, y) = panel_origin(
            icon_bounds(1200.0, 0.0),
            (380.0, 560.0),
            Some(work_area()),
            PanelPlacement::BelowIcon,
        );
        assert_eq!(x, 1022);
        assert_eq!(y, 30);
    }

    #[test]
    fn panel_expands_above_the_taskbar_on_windows() {
        let work = Bounds {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 1040.0,
        };
        let (x, y) = panel_origin(
            icon_bounds(1380.0, 1044.0),
            (380.0, 560.0),
            Some(work),
            PanelPlacement::AboveIcon,
        );
        // 任务栏图标贴近右缘，面板向左收进可用区域。
        assert_eq!(x, 1060);
        assert_eq!(y, 478);
    }

    #[test]
    fn panel_is_clamped_inside_a_monitor_on_the_left() {
        let work = Bounds {
            x: -1920.0,
            y: 25.0,
            width: 1920.0,
            height: 1055.0,
        };
        let (x, y) = panel_origin(
            icon_bounds(-1900.0, 0.0),
            (380.0, 560.0),
            Some(work),
            PanelPlacement::BelowIcon,
        );
        assert_eq!(x, -1920);
        assert_eq!(y, 30);
    }

    #[test]
    fn missing_icon_rect_falls_back_to_the_work_area_corner() {
        let empty = Bounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(
            panel_origin(
                empty,
                (380.0, 560.0),
                Some(work_area()),
                PanelPlacement::BelowIcon
            ),
            (1054, 31)
        );
        assert_eq!(
            panel_origin(
                empty,
                (380.0, 560.0),
                Some(work_area()),
                PanelPlacement::AboveIcon
            ),
            (1054, 334)
        );
    }

    #[test]
    fn panel_without_monitor_information_keeps_the_icon_anchor() {
        let (x, y) = panel_origin(
            icon_bounds(600.0, 0.0),
            (380.0, 560.0),
            None,
            PanelPlacement::BelowIcon,
        );
        assert_eq!(x, 422);
        assert_eq!(y, 30);
    }

    /// 失焦收起的判定必须同时满足“窗口可见、确实拿到过焦点、已经过了宽限期”，
    /// 否则 Windows 冷启动时的杂散失焦会让面板一闪即隐。
    #[test]
    fn panel_blur_hides_only_after_focus_and_grace() {
        assert!(!should_hide_on_blur(
            true,
            false,
            Some(Duration::from_secs(5))
        ));
        assert!(!should_hide_on_blur(
            true,
            true,
            Some(Duration::from_millis(50))
        ));
        assert!(should_hide_on_blur(
            true,
            true,
            Some(Duration::from_secs(1))
        ));
        assert!(!should_hide_on_blur(
            false,
            true,
            Some(Duration::from_secs(1))
        ));
        assert!(should_hide_on_blur(true, true, None));
    }

    #[test]
    fn idle_deadline_is_taken_only_once_and_only_after_it_expires() {
        let now = Instant::now();
        let mut future = Some(now + WINDOW_IDLE_TIMEOUT);

        assert!(!take_deadline(&mut future, now));
        assert!(future.is_some(), "未到期的窗口不能销毁");
        assert!(take_deadline(&mut future, now + WINDOW_IDLE_TIMEOUT));
        assert!(future.is_none(), "到期后只销毁一次");
        assert!(!take_deadline(&mut future, now + WINDOW_IDLE_TIMEOUT));
    }

    /// 面板必须保持 `create: false`，否则 Tauri 会在启动时预创建，
    /// 空闲销毁省下的那份 WebView 内存又会被拿回去。
    #[test]
    fn panel_window_is_created_on_demand_while_main_is_created_at_startup() {
        let raw = include_str!("../tauri.conf.json");
        let config: serde_json::Value =
            serde_json::from_str(raw).expect("tauri.conf.json 应为合法 JSON");
        let windows = config["app"]["windows"]
            .as_array()
            .expect("windows 应为数组");

        let panel = windows
            .iter()
            .find(|window| window["label"] == PANEL_LABEL)
            .expect("tray-panel 配置必须存在");
        assert_eq!(panel["create"], serde_json::json!(false));

        let main = windows
            .iter()
            .find(|window| window["label"] == MAIN_LABEL)
            .expect("main 配置必须存在");
        assert!(
            main.get("create").is_none() || main["create"] == serde_json::json!(true),
            "主窗口仍需随启动创建"
        );
    }
}
