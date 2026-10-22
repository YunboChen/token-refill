mod codex;
mod state;

use std::fs;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use state::{PersistedState, Preferences, StateStore};

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
            update_preferences
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
