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
