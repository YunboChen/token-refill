//! 版本检查与应用内更新。
//!
//! 检查阶段只查询 GitHub Releases 的最新正式版本；下载与安装是两个独立命令，
//! 都由用户在界面上显式触发，不做后台静默下载或安装。更新包由
//! `tauri-plugin-updater` 用 minisign 公钥校验后交给系统安装：
//! macOS 替换当前 `.app` 后重启，Windows 启动 NSIS 安装器。
//! Windows 的免安装单文件 exe 没有安装目录可替换，只提供发布页跳转。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::proxy::ProxyMode;

/// 发布仓库；与 release workflow 创建 Release 的仓库一致。
pub const RELEASES_REPO: &str = "YunboChen/token-refill";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
/// 下载进度事件；前端据此更新进度条。
pub const DOWNLOAD_PROGRESS_EVENT: &str = "update-download-progress";
/// 合并高频进度回调，避免每个网络分片都触发一次 IPC。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    /// 当前安装形态是否支持应用内下载与替换；Windows 免安装 exe 为 false。
    pub self_update_supported: bool,
    pub release_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// 已下载并通过签名校验、等待用户确认安装的更新包。
struct DownloadedUpdate {
    update: Update,
    bytes: Vec<u8>,
}

/// 应用管理的更新状态；同一时间只保留一份待安装的更新。
#[derive(Default)]
pub struct UpdateStore {
    downloaded: Mutex<Option<DownloadedUpdate>>,
}

/// GitHub Releases API 中本功能用到的字段；其余忽略。
#[derive(Debug, Deserialize)]
struct ReleasePayload {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// 去掉 tag 的 `v` 前缀，交给 semver 解析。
fn normalize_tag(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

fn parse_version(raw: &str) -> Option<Version> {
    Version::parse(normalize_tag(raw)).ok()
}

/// 只有两端都能解析成 semver 时才比较；格式不认识的 tag 不宣称有新版本。
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn status_from_release(
    payload: ReleasePayload,
    current: &str,
    self_update_supported: bool,
) -> UpdateStatus {
    let latest = normalize_tag(&payload.tag_name).to_string();
    let update_available =
        !payload.draft && !payload.prerelease && is_newer(&payload.tag_name, current);
    UpdateStatus {
        current_version: current.to_string(),
        latest_version: Some(latest),
        update_available,
        self_update_supported,
        release_url: payload.html_url,
        release_notes: payload.body,
        published_at: payload.published_at,
    }
}

fn map_request_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => "发布仓库暂无正式版本。".to_string(),
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub 接口访问受限（可能是频率限制），请稍后再试。".to_string()
        }
        other => format!("检查更新失败：{other}"),
    }
}

/// 测试渠道的本地状态：不请求 GitHub，也不提供任何下载/安装入口。
fn test_channel_status(current: &str) -> UpdateStatus {
    UpdateStatus {
        current_version: current.to_string(),
        latest_version: None,
        update_available: false,
        self_update_supported: false,
        release_url: None,
        release_notes: None,
        published_at: None,
    }
}

/// 读取最新正式 Release 并与当前版本比较；测试渠道直接返回本地状态。
pub fn check_for_update(app: &AppHandle) -> Result<UpdateStatus, String> {
    if crate::channel::is_test(app) {
        return Ok(test_channel_status(&app.package_info().version.to_string()));
    }

    let current = app.package_info().version.to_string();
    let url = format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest");
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(format!("TokenRefill/{current}"));
    if let Some(proxy) = crate::proxy::ureq_proxy_for(&url) {
        config = config.proxy(Some(proxy));
    }
    let agent: ureq::Agent = config.build().into();

    let mut response = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(map_request_error)?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("读取版本信息失败：{err}"))?;
    let payload: ReleasePayload = serde_json::from_str(&body)
        .map_err(|_| "版本信息格式无法识别，请稍后再试。".to_string())?;

    Ok(status_from_release(
        payload,
        &current,
        self_update_supported(app),
    ))
}

/// Windows 的免安装单文件 exe 没有安装目录，NSIS 更新器无法原地替换；
/// 只有通过 NSIS 安装、且当前 exe 位于注册表记录的安装目录时才启用应用内更新。
/// 测试渠道永远不支持应用内更新，避免被正式版更新包替换。
pub fn self_update_supported(app: &AppHandle) -> bool {
    if crate::channel::is_test(app) {
        return false;
    }
    #[cfg(windows)]
    {
        windows_install::is_installed_copy(&app.package_info().name)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn updater_for(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    match crate::proxy::resolve() {
        Some(proxy) => {
            // 代理地址可能含认证信息，解析失败时只给通用提示，不回显原文。
            let raw = proxy
                .https
                .or(proxy.http)
                .ok_or_else(|| "代理设置无效，无法下载更新。".to_string())?;
            let url =
                url::Url::parse(&raw).map_err(|_| "代理设置无效，无法下载更新。".to_string())?;
            // 更新器只接了 HTTP(S) 代理；SOCKS 无法转换时让用户走发布页，避免静默直连。
            if !matches!(url.scheme(), "http" | "https") {
                return Err("当前代理类型不支持应用内更新，请前往发布页手动下载。".to_string());
            }
            builder = builder.proxy(url);
        }
        // 用户明确选择直连时不能沿用环境变量或系统代理。
        None if crate::proxy::mode() == ProxyMode::Direct => {
            builder = builder.no_proxy();
        }
        None => {}
    }
    builder
        .build()
        .map_err(|_| "无法初始化更新器。".to_string())
}

/// 下载最新可用更新到内存并校验签名；只有用户点击后才调用。
///
/// 返回更新版本号；下载失败时返回中文提示，内部错误不回显，避免把代理地址
/// 或远端响应带进界面。
#[tauri::command]
pub async fn download_update(app: AppHandle) -> Result<String, String> {
    if crate::channel::is_test(&app) {
        return Err("测试版不支持应用内更新，请下载新的测试包后手动安装。".to_string());
    }
    if !self_update_supported(&app) {
        return Err("当前是免安装版本，请在发布页下载新版本后手动替换。".to_string());
    }

    let update = updater_for(&app)?
        .check()
        .await
        .map_err(|_| "检查更新失败，请稍后重试。".to_string())?
        .ok_or_else(|| "已是最新版本。".to_string())?;

    let version = update.version.clone();
    let received = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total = Arc::new(Mutex::new(None::<u64>));
    let last_emit = Arc::new(Mutex::new(None::<Instant>));
    let event_app = app.clone();
    let received_for_event = Arc::clone(&received);
    let total_for_event = Arc::clone(&total);
    let last_for_event = Arc::clone(&last_emit);

    let bytes = update
        .download(
            move |chunk_length, content_length| {
                let downloaded = received_for_event
                    .fetch_add(chunk_length as u64, Ordering::Relaxed)
                    + chunk_length as u64;
                let total_now = {
                    let mut slot = total_for_event
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if content_length.is_some() {
                        *slot = content_length;
                    }
                    *slot
                };
                let due = {
                    let mut last = last_for_event
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let due = last.is_none_or(|at: Instant| at.elapsed() >= PROGRESS_INTERVAL);
                    if due {
                        *last = Some(Instant::now());
                    }
                    due
                };
                if due {
                    let _ = event_app.emit(
                        DOWNLOAD_PROGRESS_EVENT,
                        UpdateProgress {
                            downloaded,
                            total: total_now,
                        },
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|_| "下载更新失败，请检查网络或代理设置。".to_string())?;

    // 收尾事件：最后一批回调可能被节流掉，这里补上完整大小。
    let final_total = total
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .or(Some(bytes.len() as u64));
    let _ = app.emit(
        DOWNLOAD_PROGRESS_EVENT,
        UpdateProgress {
            downloaded: bytes.len() as u64,
            total: final_total,
        },
    );

    let store = app.state::<UpdateStore>();
    *store
        .downloaded
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some(DownloadedUpdate { update, bytes });

    Ok(version)
}

/// 已下载但尚未安装的更新版本；设置页重新打开时用它恢复“重启并安装”状态。
#[tauri::command]
pub fn downloaded_update_version(store: tauri::State<'_, UpdateStore>) -> Option<String> {
    store
        .downloaded
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|downloaded| downloaded.update.version.clone())
}

/// 安装已下载的更新。macOS 替换应用包后重启；Windows 启动 NSIS 安装器并退出。
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    if crate::channel::is_test(&app) {
        return Err("测试版不支持应用内更新，请下载新的测试包后手动安装。".to_string());
    }
    let downloaded = {
        let store = app.state::<UpdateStore>();
        let mut slot = store
            .downloaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slot.take().ok_or_else(|| "更新尚未下载。".to_string())?
    };

    downloaded
        .update
        .install(&downloaded.bytes)
        .map_err(|_| "安装更新失败，请前往发布页手动下载。".to_string())?;

    #[cfg(target_os = "windows")]
    {
        // Windows 的 install 会在启动安装器后退出进程，正常运行不会走到这里。
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS 已替换应用包；这里不会返回。
        app.restart();
    }
}

#[cfg(windows)]
mod windows_install {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };

    // NSIS 把卸载键命名为 `${PRODUCTNAME}`；正式版与测试版的 productName 不同。
    fn uninstall_key(app_name: &str) -> String {
        format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{app_name}")
    }
    const INSTALL_LOCATION: &str = "InstallLocation";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_install_location(root: HKEY, key_path: &str) -> Option<String> {
        let key = wide(key_path);
        let value = wide(INSTALL_LOCATION);
        let mut kind = 0u32;
        let mut size = 0u32;

        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != 0 || size < 2 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }

        let units: Vec<u16> = buffer[..size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16(&units).ok()
    }

    /// NSIS 把安装目录写成带引号的字符串；比较时统一大小写与尾部斜杠。
    fn normalize(path: &str) -> String {
        path.trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }

    pub fn is_installed_copy(app_name: &str) -> bool {
        // 默认按用户安装写 HKCU；允许提权安装时也可能在 HKLM。
        let key = uninstall_key(app_name);
        let Some(location) = read_install_location(HKEY_CURRENT_USER, &key)
            .or_else(|| read_install_location(HKEY_LOCAL_MACHINE, &key))
        else {
            return false;
        };
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let Some(parent) = exe.parent() else {
            return false;
        };
        normalize(&parent.to_string_lossy()) == normalize(&location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_never_offers_update_or_self_update() {
        let status = test_channel_status("0.2.6-test.a1b2c3d");

        assert_eq!(status.current_version, "0.2.6-test.a1b2c3d");
        assert!(status.latest_version.is_none());
        assert!(!status.update_available);
        assert!(!status.self_update_supported);
        assert!(status.release_url.is_none());
    }

    fn payload(tag: &str) -> ReleasePayload {
        ReleasePayload {
            tag_name: tag.to_string(),
            html_url: Some(format!(
                "https://github.com/{RELEASES_REPO}/releases/tag/{tag}"
            )),
            body: Some("release notes".to_string()),
            published_at: Some("2026-10-23T00:00:00Z".to_string()),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn newer_tag_with_prefix_marks_update_available() {
        let status = status_from_release(payload("v0.2.0"), "0.1.0", true);
        assert!(status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("0.2.0"));
        assert_eq!(status.current_version, "0.1.0");
        assert!(status.self_update_supported);
    }

    #[test]
    fn same_or_older_version_does_not_offer_update() {
        assert!(!status_from_release(payload("v0.1.0"), "0.1.0", true).update_available);
        assert!(!status_from_release(payload("v0.0.9"), "0.1.0", true).update_available);
    }

    #[test]
    fn prerelease_and_draft_never_offer_update() {
        let mut prerelease = payload("v0.3.0");
        prerelease.prerelease = true;
        assert!(!status_from_release(prerelease, "0.1.0", true).update_available);

        let mut draft = payload("v0.3.0");
        draft.draft = true;
        assert!(!status_from_release(draft, "0.1.0", true).update_available);
    }

    #[test]
    fn unparsable_tag_is_not_treated_as_newer() {
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("v0.2.0", "dev-build"));
        let status = status_from_release(payload("nightly"), "0.1.0", true);
        assert!(!status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("nightly"));
    }

    #[test]
    fn tag_prefix_and_whitespace_are_normalized() {
        assert_eq!(normalize_tag(" v1.2.3 "), "1.2.3");
        assert!(is_newer("V1.10.0", "1.9.0"));
    }
}
