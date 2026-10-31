//! 系统通知入口。
//!
//! 已打包的 macOS 应用使用 `UNUserNotificationCenter`：旧 `NSUserNotification` 已被系统
//! 废弃，且应用位于前台时只会把通知写入通知中心、不展示横幅。其余平台（以及未打包的
//! macOS 开发运行）继续使用 `tauri-plugin-notification`。

#[cfg(target_os = "macos")]
mod macos;

/// 应用启动时调用。macOS 会在这里提前申请通知授权，避免首次提醒触发时
/// 才弹授权框，把提醒本身吞掉。
pub fn prepare() {
    #[cfg(target_os = "macos")]
    macos::prepare();
}

/// 发送一条系统通知。返回的错误信息可直接展示给用户。
pub fn send(app: &tauri::AppHandle, title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if macos::available() {
            return macos::send(&app.package_info().name, title, body);
        }
    }

    send_using_plugin(app, title, body)
}

/// 使用同一标识重试一条逻辑通知。macOS 可替换同标识的待投递请求，
/// 但无法保证已投递通知不再展示；其他平台的插件不支持此标识。
/// 标识只由 Rust 调度器生成，不应包含账户信息或凭证。
pub fn send_with_identifier(
    app: &tauri::AppHandle,
    title: &str,
    body: &str,
    identifier: &str,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if macos::available() {
            return macos::send_with_identifier(&app.package_info().name, title, body, identifier);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = identifier;

    send_using_plugin(app, title, body)
}

fn send_using_plugin(app: &tauri::AppHandle, title: &str, body: &str) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|err| format!("发送通知失败：{err}"))
}
