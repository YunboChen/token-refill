mod codex;
mod state;

use std::fs;
use std::process::Command;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

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

    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");

    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开浏览器失败：{err}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let store = Arc::new(StateStore::load(
                app.handle().clone(),
                data_dir.join("state.json"),
            ));
            app.manage(Arc::clone(&store));
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
            open_external_url
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
