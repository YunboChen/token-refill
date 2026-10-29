//! 快速面板使用非激活 NSPanel，避免聚焦时激活整个应用并切回桌面空间。

use objc2_app_kit::NSWindowStyleMask;
use tauri::{Manager, WebviewWindow};
use tauri_nspanel::{ManagerExt, WebviewWindowExt};

tauri_nspanel::tauri_panel! {
    TokenRefillTrayPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    }
}

/// 窗口尚未显示时转换，保留 Tauri 的 delegate、WebView 与窗口所有权。
pub(super) fn prepare(window: &WebviewWindow) -> tauri::Result<()> {
    let panel = window.to_panel::<TokenRefillTrayPanel>()?;
    panel.set_released_when_closed(false);
    panel.set_hides_on_deactivate(false);
    panel.set_becomes_key_only_if_needed(false);
    if panel
        .add_style_mask(NSWindowStyleMask::NonactivatingPanel)
        .is_err()
    {
        panel.to_window();
        return Err(tauri::Error::Io(std::io::Error::other(
            "无法设置快速面板的非激活样式",
        )));
    }
    panel.set_collection_behavior(super::panel_collection_behavior(
        panel.as_panel().collectionBehavior(),
    ));
    Ok(())
}

/// 不调用 Tauri set_focus：它会 activateIgnoringOtherApps，触发空间切换。
pub(super) fn show(window: &WebviewWindow) -> Result<(), tauri_nspanel::Error> {
    let panel = window.app_handle().get_webview_panel(window.label())?;
    panel.show();
    panel.make_key_window();
    Ok(())
}

/// 先移除插件持有的强引用并恢复原始窗口类，再由 Tauri 销毁 WebView。
pub(super) fn restore(window: &WebviewWindow) {
    if let Ok(panel) = window.app_handle().get_webview_panel(window.label()) {
        panel.to_window();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_panel_is_a_native_panel_instead_of_a_regular_window() {
        assert_eq!(
            RawTokenRefillTrayPanel::class().superclass(),
            Some(NSPanel::class())
        );
    }
}
