//! 悬浮球与弹层在 macOS 上使用非激活 NSPanel：
//! 点击/悬停不会激活整个应用，也不会把用户从当前全屏空间切回桌面。

use objc2_app_kit::{NSWindowCollectionBehavior, NSWindowStyleMask};
use tauri::{Manager, WebviewWindow};
use tauri_nspanel::{ManagerExt, WebviewWindowExt};

tauri_nspanel::tauri_panel! {
    TokenRefillFloatPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    }
}

fn collection_behavior(mut behavior: NSWindowCollectionBehavior) -> NSWindowCollectionBehavior {
    behavior.remove(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenPrimary
            | NSWindowCollectionBehavior::FullScreenNone,
    );
    behavior.insert(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    behavior
}

/// 窗口尚未显示时转换，保留 Tauri 的 delegate、WebView 与窗口所有权。
fn prepare(window: &WebviewWindow) -> tauri::Result<()> {
    let panel = window.to_panel::<TokenRefillFloatPanel>()?;
    panel.set_released_when_closed(false);
    panel.set_hides_on_deactivate(false);
    panel.set_becomes_key_only_if_needed(false);
    if panel
        .add_style_mask(NSWindowStyleMask::NonactivatingPanel)
        .is_err()
    {
        panel.to_window();
        return Err(tauri::Error::Io(std::io::Error::other(
            "无法设置悬浮球的非激活样式",
        )));
    }
    panel.set_collection_behavior(collection_behavior(panel.as_panel().collectionBehavior()));
    Ok(())
}

pub(super) fn prepare_native_ball(window: &WebviewWindow) {
    if let Err(err) = prepare(window) {
        eprintln!("准备悬浮球窗口失败：{err}");
    }
}

pub(super) fn prepare_native_popover(window: &WebviewWindow) {
    if let Err(err) = prepare(window) {
        eprintln!("准备悬浮球弹层失败：{err}");
    }
}

/// 球不调用 make_key_window：它只响应鼠标，不需要抢占键盘焦点。
pub(super) fn show_native_ball(window: &WebviewWindow) {
    match window.app_handle().get_webview_panel(window.label()) {
        Ok(panel) => panel.show(),
        Err(err) => {
            // 插件里还没有登记该窗口时退回普通显示，至少保证窗口可见。
            eprintln!("显示悬浮球面板失败：{err:?}");
            let _ = window.show();
        }
    }
}

/// 菜单需要 key 焦点才能收到 Esc 与失焦事件；详情卡不抢焦点。
pub(super) fn show_native_popover(window: &WebviewWindow, focus: bool) {
    match window.app_handle().get_webview_panel(window.label()) {
        Ok(panel) => {
            panel.show();
            if focus {
                panel.make_key_window();
            }
        }
        Err(err) => {
            eprintln!("显示悬浮球弹层失败：{err:?}");
            let _ = window.show();
            if focus {
                let _ = window.set_focus();
            }
        }
    }
}

/// 先移除插件持有的强引用并恢复原始窗口类，再由 Tauri 销毁 WebView。
pub(super) fn restore_native(window: &WebviewWindow) {
    if let Ok(panel) = window.app_handle().get_webview_panel(window.label()) {
        panel.to_window();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_window_is_a_native_panel() {
        assert_eq!(
            RawTokenRefillFloatPanel::class().superclass(),
            Some(NSPanel::class())
        );
    }
}
