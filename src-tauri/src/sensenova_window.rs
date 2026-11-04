//! 商汤登录窗口：加载官方授权页，只截获跳回控制台的授权码。
//!
//! 远端的商汤页面不获得任何 Tauri IPC 能力；窗口只允许在商汤域名内跳转。
//! 登录窗口使用独立的 incognito 会话，不读取用户日常浏览器的 Cookie。

use std::sync::mpsc::Sender;

use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

use crate::sensenova::{self, Callback};

/// 登录窗口向协调线程发送的唯一结果。
#[derive(Debug)]
pub enum Capture {
    Code { code: String, state: Option<String> },
    Error(String),
    Closed,
}

/// 在主线程创建登录窗口。窗口标签由调用方保证唯一。
pub fn open(
    app: &AppHandle,
    label: &str,
    auth_url: &str,
    capture: Sender<Capture>,
) -> Result<WebviewWindow, String> {
    let url = url::Url::parse(auth_url).map_err(|_| "商汤授权地址无效。".to_string())?;
    let navigation = capture.clone();
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title("商汤登录")
        .inner_size(520.0, 760.0)
        .min_inner_size(380.0, 560.0)
        .center()
        .focused(true)
        .incognito(true)
        .on_navigation(move |url| {
            if let Some(callback) = sensenova::parse_callback(url) {
                match callback {
                    Callback::Code { code, state } => {
                        let _ = navigation.send(Capture::Code { code, state });
                    }
                    Callback::Error(message) => {
                        let _ = navigation.send(Capture::Error(message));
                    }
                }
                // 授权码已经取到，阻止控制台页面继续加载。
                return false;
            }
            sensenova::navigation_allowed(url)
        })
        // 登录页不需要弹出新窗口；一律拒绝，避免出现不受导航限制的 WebView。
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|err| format!("无法打开商汤登录窗口：{err}"))?;

    let events = capture;
    window.on_window_event(move |event| {
        if matches!(
            event,
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
        ) {
            let _ = events.send(Capture::Closed);
        }
    });
    Ok(window)
}

/// 从任意线程销毁登录窗口。窗口操作必须回到主线程执行；
/// 用 `destroy` 而不是 `close`，避免关闭请求在后台线程上被吞掉后留下隐藏窗口。
pub fn close(window: &WebviewWindow) {
    let window = window.clone();
    let app = window.app_handle().clone();
    let _ = app.run_on_main_thread(move || {
        let _ = window.destroy();
    });
}
