//! 悬浮球与弹层在 macOS 上使用非激活 NSPanel：
//! 点击/悬停不会激活整个应用，也不会把用户从当前全屏空间切回桌面。

use objc2_app_kit::{NSWindowCollectionBehavior, NSWindowStyleMask};
use tauri::{Manager, WebviewWindow};
use tauri_nspanel::{FromWindow, ManagerExt, WebviewWindowExt};

// 两种面板要分开定义：`canBecomeKeyWindow` 是类方法，无法按窗口实例区分。
//
// - 球：永远不能成为 key。非 key 窗口的第一次点击会让 AppKit 先取 key 而
//   吞掉这次 mouseDown；不能成为 key 的窗口则直接把点击交给 WebView，
//   同时也不会把键盘焦点从用户正在使用的应用里抢走。
// - 弹层：菜单需要 key 才能收到 Esc 与失焦；详情卡不取 key，但首次点击
//   如果被吞掉，“切换套餐”就要点两次。`install_first_click` 在 mouseDown
//   进入窗口时先取 key，再让 AppKit 正常派发这次点击。
mod ball_panel {
    tauri_nspanel::tauri_panel! {
        TokenRefillBallPanel {
            config: {
                can_become_key_window: false,
                can_become_main_window: false,
                is_floating_panel: true
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ball_window_is_a_native_panel() {
            assert_eq!(
                RawTokenRefillBallPanel::class().superclass(),
                Some(NSPanel::class())
            );
        }
    }
}

mod popover_panel {
    use std::sync::OnceLock;

    use objc2::runtime::{AnyClass, Imp, Sel};
    use objc2::sel;
    use objc2_app_kit::NSEventType;

    tauri_nspanel::tauri_panel! {
        TokenRefillFloatPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        }
    }

    /// 父类的 `sendEvent:` 实现；子类只做“先取 key”的补丁，其余照常派发。
    static SUPER_SEND_EVENT: OnceLock<Imp> = OnceLock::new();

    unsafe extern "C-unwind" fn send_event_after_taking_key(
        this: &AnyObject,
        selector: Sel,
        event: &NSEvent,
    ) {
        // 弹层不是 key 时，AppKit 会把首次 mouseDown 用于取 key 而不派发；
        // 先同步取 key，随后原实现就会把这次点击交给 WebView。
        if event.r#type() == NSEventType::LeftMouseDown {
            let is_key: bool = unsafe { msg_send![this, isKeyWindow] };
            if !is_key {
                let _: () = unsafe { msg_send![this, makeKeyWindow] };
            }
        }
        if let Some(super_imp) = SUPER_SEND_EVENT.get() {
            let super_send: unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSEvent) =
                unsafe { std::mem::transmute(*super_imp) };
            unsafe { super_send(this, selector, event) };
        }
    }

    /// 只给弹层窗口补首击；球不能成为 key，点击本来就直达 WebView，不需要这一步。
    pub(super) fn install_first_click() {
        static INSTALLED: OnceLock<()> = OnceLock::new();
        INSTALLED.get_or_init(|| {
            let class = RawTokenRefillFloatPanel::class();
            let selector = sel!(sendEvent:);
            let Some(super_method) = class
                .superclass()
                .and_then(|superclass| superclass.instance_method(selector))
            else {
                eprintln!("无法读取弹层窗口的 sendEvent: 实现");
                return;
            };
            if SUPER_SEND_EVENT.set(super_method.implementation()).is_err() {
                return;
            }
            let implementation: Imp = unsafe {
                std::mem::transmute::<_, Imp>(
                    send_event_after_taking_key
                        as unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSEvent),
                )
            };
            let added = unsafe {
                objc2::ffi::class_addMethod(
                    class as *const AnyClass as *mut AnyClass,
                    selector,
                    implementation,
                    c"v@:@".as_ptr(),
                )
            };
            if !added.as_bool() {
                eprintln!("无法为弹层窗口补上首击处理");
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn popover_window_is_a_native_panel() {
            assert_eq!(
                RawTokenRefillFloatPanel::class().superclass(),
                Some(NSPanel::class())
            );
        }
    }
}

use ball_panel::TokenRefillBallPanel;
use popover_panel::{install_first_click, TokenRefillFloatPanel};

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
fn prepare<P>(window: &WebviewWindow) -> tauri::Result<()>
where
    P: FromWindow<tauri::Wry> + 'static,
{
    let panel = window.to_panel::<P>()?;
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
    if let Err(err) = prepare::<TokenRefillBallPanel>(window) {
        eprintln!("准备悬浮球窗口失败：{err}");
    }
}

pub(super) fn prepare_native_popover(window: &WebviewWindow) {
    if let Err(err) = prepare::<TokenRefillFloatPanel>(window) {
        eprintln!("准备悬浮球弹层失败：{err}");
        return;
    }
    install_first_click();
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
