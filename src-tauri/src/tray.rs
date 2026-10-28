//! 菜单栏常驻与快速查看面板。
//!
//! 菜单栏图标左键切换一个无边框面板窗口（label 为 `tray-panel`），
//! 面板定位在图标下方（Windows 在任务栏上方），失焦后自动隐藏。
//! 右键菜单提供打开主窗口、查看额度与退出；关闭主窗口只隐藏，
//! 应用继续在菜单栏驻留并刷新。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, Size, WindowEvent,
};

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

static LAST_PANEL_BLUR: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

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

/// 打开快速查看面板。macOS 用 Cocoa 原生坐标系定位（多显示器与
/// Retina/非 Retina 混排时 Tauri 的物理坐标不可靠），其他平台按
/// 托盘图标矩形与显示器可用区域计算。
pub fn show_panel(app: &AppHandle, rect: Rect) {
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    // 托盘回调不保证在主线程：AppKit 的定位与显示统一投递到主线程执行。
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        show_panel_now(&handle, &panel, rect);
    });
}

fn show_panel_now(app: &AppHandle, panel: &tauri::WebviewWindow, rect: Rect) {
    #[cfg(target_os = "macos")]
    let placed = place_panel_on_macos(panel).unwrap_or(false);
    #[cfg(not(target_os = "macos"))]
    let placed = false;

    if !placed {
        place_panel_generic(app, panel, &rect);
    }

    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = app.emit(PANEL_SHOWN_EVENT, ());
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

    let pointer = panel.ns_window()? as *mut NSWindow;
    // SAFETY: 托盘事件在主线程触发，`ns_window` 返回当前窗口的 NSWindow 指针。
    let window = unsafe { &*pointer };
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
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    if blurred_recently() {
        return;
    }
    show_panel(app, rect);
}

/// 把主窗口带到前台；面板若打开则先收起，避免两个窗口同时盖在界面上。
pub fn show_main_window(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let _ = panel.hide();
    }
    #[cfg(target_os = "macos")]
    {
        // 关闭主窗口时应用已退成附件形态；必须切回常规形态再显示窗口，否则
        // 窗口拿不到 key 焦点，也没有应用菜单栏。应用被 Cmd+H 隐藏时一并取消。
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = app.show();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
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
    // 关闭主窗口只隐藏：应用继续在菜单栏驻留并后台刷新。macOS 同时退成附件
    // 形态，Dock 图标、Cmd+Tab 与应用菜单栏一并消失，只留菜单栏图标。
    if let Some(main) = app.get_webview_window("main") {
        let window = main.clone();
        main.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                {
                    // 先隐藏窗口再切形态，避免附件形态下窗口仍处于 key 状态。
                    let _ = window
                        .app_handle()
                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
        });
    }

    // 面板失焦自动收起；菜单栏点击造成的失焦留给托盘事件判断。
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        let window = panel.clone();
        panel.on_window_event(move |event| {
            if let WindowEvent::Focused(false) = event {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                    note_panel_blur();
                }
            }
        });
    }

    let open_main = MenuItem::with_id(app, "open-main", "打开主窗口", true, None::<&str>)?;
    let view_quota = MenuItem::with_id(app, "view-quota", "查看额度", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Token Refill", true, None::<&str>)?;
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
        .tooltip("Token Refill")
        .menu(&menu)
        // 左键用于快速查看，右键才展开菜单。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-main" => show_main_window(app),
            "view-quota" => {
                let rect = cursor_rect(app);
                show_panel(app, rect);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), rect);
            }
        });

    #[cfg(target_os = "macos")]
    {
        // 模板图标只保留 alpha 轮廓，菜单栏会按浅色/深色自动反色。
        builder = builder
            .icon(tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray-template.png"
            ))?)
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
}
