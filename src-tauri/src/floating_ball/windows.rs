//! Windows 上的悬浮球窗口：置顶、不进任务栏，球不抢焦点（WS_EX_NOACTIVATE），
//! 弹层需要焦点才能接收 Esc 与失焦事件，因此只加 WS_EX_TOOLWINDOW。

use tauri::WebviewWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

fn apply_tool_window(window: &WebviewWindow, no_activate: bool) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // Tauri 返回 `windows` crate 的 HWND 包装类型，windows-sys 需要裸句柄。
    let hwnd = hwnd.0;
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        style |= WS_EX_TOOLWINDOW as isize;
        if no_activate {
            style |= WS_EX_NOACTIVATE as isize;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
    }
}

pub(super) fn prepare_native_ball(window: &WebviewWindow) {
    apply_tool_window(window, true);
}

pub(super) fn prepare_native_popover(window: &WebviewWindow) {
    apply_tool_window(window, false);
}

pub(super) fn show_native_ball(window: &WebviewWindow) {
    let _ = window.show();
}

pub(super) fn show_native_popover(window: &WebviewWindow, focus: bool) {
    let _ = window.show();
    if focus {
        let _ = window.set_focus();
    }
}

pub(super) fn restore_native(_window: &WebviewWindow) {}
