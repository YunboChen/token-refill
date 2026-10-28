mod antigravity;
mod api_key;
mod bailian;
mod cli;
mod codex;
mod commandcode;
mod discovery;
mod notification;
mod opencode;
mod proxy;
pub mod quota;
mod relay;
mod state;
mod tray;
mod update;
mod usage_api;

use std::fs;
#[cfg(not(target_os = "windows"))]
use std::process::Command;
use std::sync::Arc;

use serde::Serialize;
#[cfg(windows)]
use tauri::Listener;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use state::{LoginStartPayload, PersistedState, Preferences, StateStore};

/// 前端首帧就绪（自绘标题栏已画出）。Windows 主窗口以隐藏状态创建，
/// 收到该事件后再显示，避免 React 首绘前先露出约 1 秒的系统标题栏。
#[cfg(windows)]
const MAIN_UI_READY_EVENT: &str = "main-ui-ready";

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

/// 发起百炼控制台浏览器授权；令牌只写入应用私有目录，不经过前端。
#[tauri::command]
fn begin_bailian_login(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: Option<String>,
    display_name: Option<String>,
) -> Result<LoginStartPayload, String> {
    let store = Arc::clone(&store);
    store.begin_bailian_login(account_id, display_name)
}

#[tauri::command]
fn rename_account(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: String,
    display_name: String,
) -> Result<PersistedState, String> {
    store.rename_account(&account_id, &display_name)
}

/// 用 API Key 接入 OpenCode Go / Command Code；密钥只交给 Rust，不进入前端状态。
#[tauri::command]
async fn add_api_key_account(
    store: tauri::State<'_, Arc<StateStore>>,
    platform: String,
    display_name: Option<String>,
    api_key: String,
) -> Result<PersistedState, String> {
    let store = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || {
        store.add_api_key_account(&platform, display_name.as_deref(), &api_key)
    })
    .await
    .map_err(|err| format!("添加账户失败：{err}"))?
}

/// 替换已有 API Key 账户的密钥；验证通过才覆盖。
#[tauri::command]
async fn replace_api_key(
    store: tauri::State<'_, Arc<StateStore>>,
    account_id: String,
    api_key: String,
) -> Result<PersistedState, String> {
    let store = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || store.replace_api_key(&account_id, &api_key))
        .await
        .map_err(|err| format!("更新 API Key 失败：{err}"))?
}

/// 接入本机 Antigravity CLI 登录态；额度在后台刷新。
#[tauri::command]
fn add_antigravity_account(
    store: tauri::State<'_, Arc<StateStore>>,
    display_name: Option<String>,
) -> Result<PersistedState, String> {
    let store = Arc::clone(&store);
    store.add_antigravity_account(display_name.as_deref())
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
/// 授权在启动时申请；被拒绝或尚未决定时，这里返回可直接展示的提示。
#[tauri::command]
async fn send_test_notification(app: tauri::AppHandle) -> Result<(), String> {
    // 查询授权状态会等系统回调，放到阻塞线程，避免卡住界面。
    tauri::async_runtime::spawn_blocking(move || {
        notification::send(
            &app,
            "Token Refill · 测试通知",
            "如果你看到这条通知，说明系统通知已经可用。",
        )
    })
    .await
    .map_err(|err| format!("发送测试通知失败：{err}"))?
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
    // macOS 固定使用系统自带的 `open`，不走 PATH 查找：某些终端（例如 cmux）会把
    // 自带的 `open` 垫片排在 PATH 前面，把 https 链接转进终端内置的网页标签。
    // 授权页必须交给系统默认浏览器，同时避免被同名可执行文件劫持。
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
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

/// 中转站条目变更后广播完整列表：主窗口与菜单栏面板各自持有前端副本，
/// 没有事件时只有发起操作的窗口会更新，另一个窗口会停留在旧的失败/成功状态。
fn broadcast_relay_accounts(app: &tauri::AppHandle) {
    let store = app.state::<Arc<relay::RelayStore>>();
    let _ = app.emit(relay::RELAY_ACCOUNTS_EVENT, store.view_with_revision());
}

/// 自动识别上游（New API / Sub2API）并查询密钥额度。
/// 手动输入的凭证由 Rust 保存到 `relay-profiles/`，不经过前端。
#[tauri::command]
async fn relay_query(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<relay::RelayStore>>,
    base_url: String,
    api_key: String,
    account_token: Option<String>,
    new_api_user_id: Option<String>,
) -> Result<relay::RelayAccount, String> {
    let store = Arc::clone(&store);
    let result = tauri::async_runtime::spawn_blocking(move || {
        store.query(
            &base_url,
            &api_key,
            account_token.as_deref(),
            new_api_user_id.as_deref(),
        )
    })
    .await
    .map_err(|err| format!("查询中转站额度失败：{err}"))?;
    if result.is_ok() {
        broadcast_relay_accounts(&app);
    }
    result
}

/// 为已有的 New API 账户补充个人访问令牌，并立即刷新账户额度。
#[tauri::command]
async fn relay_set_account_token(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<relay::RelayStore>>,
    id: String,
    account_token: String,
    new_api_user_id: Option<String>,
) -> Result<relay::RelayAccount, String> {
    let store = Arc::clone(&store);
    let result = tauri::async_runtime::spawn_blocking(move || {
        store.set_account_token(&id, &account_token, new_api_user_id.as_deref())
    })
    .await
    .map_err(|err| format!("保存访问令牌失败：{err}"))?;
    if result.is_ok() {
        broadcast_relay_accounts(&app);
    }
    result
}

/// 扫描 cc-switch、Claude Code / Codex 的本地配置，只返回候选，不发起网络请求。
#[tauri::command]
async fn relay_scan(
    store: tauri::State<'_, Arc<relay::RelayStore>>,
) -> Result<Vec<relay::RelayCandidate>, String> {
    let store = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || store.scan())
        .await
        .map_err(|err| format!("扫描本地配置失败：{err}"))
}

/// 导入用户勾选的候选：识别上游并查询额度。
#[tauri::command]
async fn relay_import(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<relay::RelayStore>>,
    ids: Vec<String>,
) -> Result<relay::DiscoveryReport, String> {
    let store = Arc::clone(&store);
    let result = tauri::async_runtime::spawn_blocking(move || store.import(&ids))
        .await
        .map_err(|err| format!("导入本地配置失败：{err}"))?;
    broadcast_relay_accounts(&app);
    Ok(result)
}

#[tauri::command]
async fn relay_refresh(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<relay::RelayStore>>,
    id: String,
) -> Result<relay::RelayAccount, String> {
    let store = Arc::clone(&store);
    let result = tauri::async_runtime::spawn_blocking(move || store.refresh(&id))
        .await
        .map_err(|err| format!("刷新中转站额度失败：{err}"))?;
    if result.is_ok() {
        broadcast_relay_accounts(&app);
    }
    result
}

/// 返回条目列表与修订号；窗口首次加载或重新显示时据此对齐，再靠事件增量同步。
#[tauri::command]
fn relay_list(store: tauri::State<'_, Arc<relay::RelayStore>>) -> relay::RelayAccountsEvent {
    store.view_with_revision()
}

/// 菜单栏面板里的“打开主窗口”：先收起面板，再把管理窗口带到前台。
#[tauri::command]
fn open_main_window(app: tauri::AppHandle) {
    tray::show_main_window(&app);
}

/// 面板按 Esc 收起；隐藏而不是销毁，保持已加载的状态与订阅。
#[tauri::command]
fn hide_tray_panel(app: tauri::AppHandle) {
    if let Some(panel) = app.get_webview_window(tray::PANEL_LABEL) {
        let _ = panel.hide();
    }
}

/// 菜单栏面板里的明确退出入口；RunEvent::Exit 负责让调度器停止。
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn relay_remove(
    app: tauri::AppHandle,
    store: tauri::State<'_, Arc<relay::RelayStore>>,
    id: String,
) -> Result<(), String> {
    let store = Arc::clone(&store);
    let result = tauri::async_runtime::spawn_blocking(move || store.remove(&id))
        .await
        .map_err(|err| format!("移除中转站账户失败：{err}"))?;
    if result.is_ok() {
        broadcast_relay_accounts(&app);
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // 单实例插件必须最先注册：第二个进程在这里退出，并把已有窗口带到前台，
        // 否则两个进程共用同一份 state.json 与授权目录。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        // 更新器只从 Rust 侧调用；前端没有 updater 权限，下载与安装均需显式命令。
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let store = Arc::new(StateStore::load(
                app.handle().clone(),
                data_dir.join("state.json"),
            ));
            app.manage(Arc::clone(&store));
            app.manage(Arc::new(relay::RelayStore::load(
                data_dir.join("relay-profiles"),
            )));
            app.manage(update::UpdateStore::default());

            // 自启以系统登录项为准：启动时回写实际状态，避免本地开关与系统不一致。
            match app.autolaunch().is_enabled() {
                Ok(actual) => store.reconcile_auto_launch(actual),
                Err(err) => eprintln!("读取开机自启状态失败：{err}"),
            }

            // macOS 在启动时申请通知授权，避免首次提醒才弹授权框。
            notification::prepare();

            // Windows 使用前端自绘标题栏，需要去掉系统边框与标题按钮；主窗口在配置里
            // 以隐藏状态创建，等前端画出标题栏再显示，否则启动瞬间会先露出系统标题栏。
            // 平台配置文件走 JSON Merge Patch，覆盖 windows 数组会整体替换，
            // 因此装饰只在运行时按平台关掉。
            #[cfg(windows)]
            {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.set_decorations(false);
                }
                let handle = app.handle().clone();
                app.handle().listen(MAIN_UI_READY_EVENT, move |_event| {
                    tray::show_main_window(&handle);
                });
                // 前端没走到就绪事件（脚本异常等）时兜底显示，避免窗口一直隐藏、
                // 只能靠托盘菜单找回来。
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if let Some(main) = handle.get_webview_window("main") {
                        if !main.is_visible().unwrap_or(true) {
                            let _ = main.show();
                        }
                    }
                });
            }

            // macOS 保持 Overlay 原生标题栏（红绿灯与拖拽交给系统），窗口随启动显示。
            #[cfg(not(windows))]
            tray::show_main_window(app.handle());

            // 菜单栏图标与快速查看面板；关闭主窗口只隐藏，应用继续常驻。
            tray::setup(app.handle())?;

            state::start_scheduler(Arc::clone(&store));
            state::auto_detect_local_accounts(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            get_app_state,
            refresh_usage,
            update_preferences,
            begin_account_login,
            begin_bailian_login,
            cancel_account_login,
            rename_account,
            add_api_key_account,
            replace_api_key,
            add_antigravity_account,
            remove_account,
            open_external_url,
            send_test_notification,
            set_auto_launch,
            check_for_update,
            update::download_update,
            update::downloaded_update_version,
            update::install_update,
            relay_query,
            relay_set_account_token,
            relay_scan,
            relay_import,
            relay_refresh,
            relay_list,
            relay_remove,
            open_main_window,
            hide_tray_panel,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Token Refill");

    app.run(|app_handle, event| match event {
        // 从 Finder、Spotlight 等重新打开应用时，把被“关闭”（实际是隐藏）的
        // 管理窗口带回来。Dock 图标在窗口隐藏期间不显示，不会触发这里。
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => tray::show_main_window(app_handle),
        // 退出时让调度器停止发起新任务；在途查询随后随进程结束。
        tauri::RunEvent::Exit => {
            app_handle.state::<Arc<StateStore>>().shutdown();
        }
        _ => {}
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
