mod codex;
mod state;
mod update;

use std::fs;
#[cfg(not(target_os = "windows"))]
use std::process::Command;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;

use state::{LoginStartPayload, PersistedState, Preferences, StateStore};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
    arch: &'static str,
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        name: "Token Refill",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// 返回脱敏的完整状态；前端首次打开窗口或重连时以此为准。
#[tauri::command]
fn get_app_state(store: tauri::State<'_, Arc<StateStore>>) -> PersistedState {
    store.snapshot()
}

/// 接受刷新请求后立即返回；实际查询在后台线程执行，结果通过事件广播。
#[tauri::command]
fn refresh_usage(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: Option<String>,
) -> Result<(), String> {
    let store = Arc::clone(&store);
    store.request_refresh(account_id.as_deref())
}

#[tauri::command]
fn update_preferences(
    store: tauri::State<'_, Arc<StateStore>>,
    preferences: Preferences,
) -> Result<PersistedState, String> {
    store.update_preferences(preferences)
}

/// 在独立 CODEX_HOME 中发起官方浏览器授权；结果通过事件推送。
#[tauri::command]
fn begin_account_login(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: Option<String>,
    display_name: Option<String>,
) -> Result<LoginStartPayload, String> {
    let store = Arc::clone(&store);
    store.begin_login(account_id, display_name)
}

#[tauri::command]
fn cancel_account_login(
    store: tauri::State<'_, Arc<StateStore>>,
    login_id: String,
) -> Result<(), String> {
    store.cancel_login(&login_id)
}

#[tauri::command]
fn rename_account(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: String,
    display_name: String,
) -> Result<PersistedState, String> {
    store.rename_account(&account_id, &display_name)
}

#[tauri::command]
fn remove_account(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: String,
) -> Result<PersistedState, String> {
    store.remove_account(&account_id)
}

/// 修改开机自启：先写入系统登录项，确认系统实际状态后才回写偏好。
/// 失败直接返回错误，不把本地开关改成未生效的值。
#[tauri::command]
fn set_auto_launch(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<StateStore>>,
    enabled: bool,
) -> Result<PersistedState, String> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch
            .enable()
            .map_err(|err| format!("启用开机自启失败：{err}"))?;
    } else {
        autolaunch
            .disable()
            .map_err(|err| format!("关闭开机自启失败：{err}"))?;
    }

    let actual = autolaunch
        .is_enabled()
        .map_err(|err| format!("读取开机自启状态失败：{err}"))?;
    if actual != enabled {
        return Err("系统未接受开机自启设置，请检查登录项权限。".to_string());
    }
    store.set_auto_launch_preference(actual)
}

/// 设置页的显式测试入口，用于确认系统通知是否可用。
/// 桌面端没有应用内授权弹窗；通知是否展示由系统通知设置决定。
#[tauri::command]
fn send_test_notification(app: tauri::AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("Token Refill · 测试通知")
        .body("如果你看到这条通知，说明系统通知已经可用。")
        .show()
        .map_err(|err| format!("发送测试通知失败：{err}"))
}

/// 仅允许打开 HTTPS 授权地址，避免把任意输入交给系统 shell。
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() > 2048 {
        return Err("只允许打开 HTTPS 授权链接".to_string());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("授权链接包含无效字符".to_string());
    }

    open_in_browser(url)
}

/// Windows 不能走 `cmd /C start`：授权 URL 的查询串含有 `&`，会被 cmd.exe
/// 当成命令分隔符。直接调用 ShellExecuteW，URL 作为独立参数交给系统。
#[cfg(target_os = "windows")]
fn open_in_browser(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation: Vec<u16> = "open\0".encode_utf16().collect();
    let target: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    // ShellExecuteW 的返回值大于 32 才代表成功，小于等于 32 是错误码。
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err(format!("打开浏览器失败：ShellExecuteW 返回 {result}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");

    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开浏览器失败：{err}"))
}

/// 查询 GitHub Releases 的最新正式版本；只比较版本与返回下载页，不下载文件。
/// 网络请求是阻塞的，放到专用线程，避免占用 IPC 处理线程。
#[tauri::command]
async fn check_for_update() -> Result<update::UpdateStatus, String> {
    tauri::async_runtime::spawn_blocking(update::check_for_update)
        .await
        .map_err(|err| format!("检查更新失败：{err}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let store = Arc::new(StateStore::load(
                app.handle().clone(),
                data_dir.join("state.json"),
            ));
            app.manage(Arc::clone(&store));

            // 自启以系统登录项为准：启动时回写实际状态，避免本地开关与系统不一致。
            match app.autolaunch().is_enabled() {
                Ok(actual) => store.reconcile_auto_launch(actual),
                Err(err) => eprintln!("读取开机自启状态失败：{err}"),
            }

            state::start_scheduler(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            get_app_state,
            refresh_usage,
            update_preferences,
            begin_account_login,
            cancel_account_login,
            rename_account,
            remove_account,
            open_external_url,
            send_test_notification,
            set_auto_launch,
            check_for_update
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Token Refill");

    app.run(|app_handle, event| {
        // 退出时让调度器停止发起新任务；在途查询随后随进程结束。
        if let tauri::RunEvent::Exit = event {
            app_handle.state::<Arc<StateStore>>().shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::get_app_info;

    #[test]
    fn app_info_reports_the_current_build_and_target() {
        let info = get_app_info();

        assert_eq!(info.name, "Token Refill");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.platform, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
    }
}
