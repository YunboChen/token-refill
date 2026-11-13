//! macOS 主窗口材质。
//!
//! 主窗口改为「系统玻璃/毛玻璃外壳 + 前端实色浮卡」：Rust 侧把窗口与
//! WKWebView 都设为透明，并在窗口底部铺一层系统材质；前端的侧栏、工具栏
//! 保持半透明，内容区用实色浮卡承接。macOS 26 及以上使用
//! `NSGlassEffectView`，12–15 回退 `NSVisualEffectView`（由 Tauri 的
//! `set_effects` 负责判断版本与回退）。
//!
//! 材质只在主窗口使用：菜单栏面板与悬浮球弹层保持实色，避免在小尺寸
//! 浮层里叠两层透明材质。

/// 给主窗口铺系统材质。必须在主线程调用；`setup()` 满足这个前提。
#[cfg(target_os = "macos")]
pub fn apply_main(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2_app_kit::{NSColor, NSWindow};
    use tauri::webview::Color;
    use tauri::window::{Effect, EffectState, EffectsBuilder};

    // 关闭 WKWebView 的默认白底（macOS 侧走 `drawsBackground` KVC），
    // 否则材质会被白色 WebView 背景完全盖住。
    window
        .set_background_color(Some(Color(0, 0, 0, 0)))
        .map_err(|err| format!("设置主窗口透明背景失败：{err}"))?;

    let ns_window = window
        .ns_window()
        .map_err(|err| format!("读取主窗口 NSWindow 失败：{err}"))?
        as *mut NSWindow;
    if ns_window.is_null() {
        return Err("主窗口 NSWindow 为空".into());
    }

    // SAFETY: 主线程；`ns_window` 来自 Tauri 当前存活的窗口，指针在调用期间有效。
    unsafe {
        let ns_window = &*ns_window;
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
        // Overlay 标题栏与内容连成一片，红绿灯直接浮在玻璃上。
        ns_window.setTitlebarAppearsTransparent(true);
    }

    // 26+ 先用液态玻璃；低于 26 时 Tauri 自动回退到 UnderWindowBackground 毛玻璃。
    window
        .set_effects(
            EffectsBuilder::new()
                .effect(Effect::LiquidGlassRegular)
                .effect(Effect::UnderWindowBackground)
                .state(EffectState::Active)
                .build(),
        )
        .map_err(|err| format!("应用主窗口材质失败：{err}"))?;

    Ok(())
}

/// 当前平台的窗口材质，前端据此决定是否铺实色回退。
#[cfg(target_os = "macos")]
pub fn material_kind() -> &'static str {
    "glass"
}

#[cfg(windows)]
pub fn material_kind() -> &'static str {
    if windows_material_supported() {
        "mica"
    } else {
        "solid"
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn material_kind() -> &'static str {
    "solid"
}

/// 主窗口使用 Mica。Windows 10 或关闭“透明效果”时保持实色，由 CSS 回退。
#[cfg(windows)]
pub fn apply_main(window: &tauri::WebviewWindow) -> Result<(), String> {
    use tauri::webview::Color;
    use tauri::window::{Effect, EffectsBuilder};

    if !windows_material_supported() {
        return Ok(());
    }
    window
        .set_background_color(Some(Color(0, 0, 0, 0)))
        .map_err(|err| format!("设置主窗口透明背景失败：{err}"))?;
    window
        .set_effects(EffectsBuilder::new().effect(Effect::Mica).build())
        .map_err(|err| format!("应用主窗口 Mica 失败：{err}"))?;
    Ok(())
}

/// 菜单栏面板与悬浮球弹层使用 Acrylic；不支持时由 CSS 回退实色。
#[cfg(windows)]
pub fn apply_panel(window: &tauri::WebviewWindow) -> Result<(), String> {
    use tauri::window::{Effect, EffectsBuilder};

    if !windows_material_supported() {
        return Ok(());
    }
    window
        .set_effects(EffectsBuilder::new().effect(Effect::Acrylic).build())
        .map_err(|err| format!("应用浮层 Acrylic 失败：{err}"))?;
    Ok(())
}

/// Windows 11 且系统开启透明效果时才使用 Mica / Acrylic。
#[cfg(windows)]
fn windows_material_supported() -> bool {
    use std::sync::OnceLock;

    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| windows_build_number() >= 22000 && transparency_enabled())
}

#[cfg(windows)]
#[repr(C)]
struct OsVersionInfoW {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    csd: [u16; 128],
}

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
}

#[cfg(windows)]
fn windows_build_number() -> u32 {
    let mut info = OsVersionInfoW {
        size: std::mem::size_of::<OsVersionInfoW>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        csd: [0; 128],
    };
    // SAFETY: `info` 是完整的 OSVERSIONINFOW 布局，调用只写入该结构。
    let _ = unsafe { RtlGetVersion(&mut info) };
    info.build
}

#[cfg(windows)]
fn transparency_enabled() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("EnableTransparency");
    let mut enabled: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: 子键与值名都是以 NUL 结尾的 UTF-16；缓冲区是有效的 u32。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut enabled as *mut u32 as *mut _,
            &mut size,
        )
    };
    // 读取失败时按开启处理：Windows 11 默认开启，避免误判为实色。
    status != 0 || enabled != 0
}
