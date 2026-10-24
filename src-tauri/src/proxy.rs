//! 全局代理设置与系统代理检测。
//!
//! 用户在设置页选择三种模式：跟随系统、不走代理、指定代理。该设置统一作用于
//! 应用发起的 HTTP 请求，以及需要通过环境变量配置代理的子进程（`agy`、
//! Codex app-server）。
//!
//! - Windows：读取当前用户 Internet Settings 的 `ProxyEnable` / `ProxyServer`
//!   / `ProxyOverride`；
//! - macOS：读取 `scutil --proxy`；
//! - 其他平台：不主动探测系统设置。
//!
//! 跟随系统时，进程已有的 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 优先；
//! 不走代理时会从子进程环境中移除这些变量；指定代理覆盖已有环境变量。
//! PAC / 自动配置脚本无法安全地转换成代理地址，这里不做解析。

use std::process::Command;
use std::sync::{LazyLock, RwLock};

use serde::{Deserialize, Serialize};
use url::{Host, Url};

const LOCAL_NO_PROXY: &str = "localhost,127.0.0.1,::1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProxyMode {
    #[default]
    System,
    #[serde(rename = "none")]
    Direct,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Settings {
    mode: ProxyMode,
    manual_url: Option<String>,
}

static SETTINGS: LazyLock<RwLock<Settings>> = LazyLock::new(|| RwLock::new(Settings::default()));

fn store() -> &'static RwLock<Settings> {
    &SETTINGS
}

/// 由状态层在启动与偏好变更后调用。
pub fn configure(mode: ProxyMode, manual_url: Option<String>) {
    let mut settings = store()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *settings = Settings { mode, manual_url };
}

fn current() -> Settings {
    store()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// 解析后的代理。`None` 表示直连。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProxy {
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Option<String>,
}

impl ResolvedProxy {
    fn from_url(url: String) -> Self {
        Self {
            http: Some(url.clone()),
            https: Some(url),
            no_proxy: Some(LOCAL_NO_PROXY.to_string()),
        }
    }
}

/// 当前设置解析出的代理；直连或没有可用代理时返回 `None`。
pub fn resolve() -> Option<ResolvedProxy> {
    let settings = current();
    match settings.mode {
        ProxyMode::Direct => None,
        ProxyMode::Manual => settings
            .manual_url
            .as_deref()
            .and_then(|url| normalize_proxy_url(url).ok())
            .map(ResolvedProxy::from_url),
        ProxyMode::System => system_proxy(),
    }
}

/// 给子进程写入代理环境变量。`agy`、Codex app-server 等外部 CLI 都走这里。
pub fn apply_to_command(command: &mut Command) {
    apply_settings(&current(), command);
}

fn apply_settings(settings: &Settings, command: &mut Command) {
    match settings.mode {
        ProxyMode::Direct => remove_proxy_env(command),
        ProxyMode::Manual => {
            let Some(url) = settings
                .manual_url
                .as_deref()
                .and_then(|url| normalize_proxy_url(url).ok())
            else {
                return;
            };
            remove_proxy_env(command);
            set_proxy_env(command, "HTTP_PROXY", &url);
            set_proxy_env(command, "HTTPS_PROXY", &url);
            set_proxy_env(command, "NO_PROXY", LOCAL_NO_PROXY);
        }
        ProxyMode::System => {
            // 已有环境变量优先，避免覆盖用户显式配置。
            if env_var("ALL_PROXY").is_some()
                || env_var("HTTP_PROXY").is_some()
                || env_var("HTTPS_PROXY").is_some()
            {
                return;
            }
            let Some(proxy) = detect_os() else {
                return;
            };
            if let Some(https) = proxy.https.as_ref().or(proxy.http.as_ref()) {
                set_proxy_env(command, "HTTPS_PROXY", https);
            }
            if let Some(http) = proxy.http.as_ref().or(proxy.https.as_ref()) {
                set_proxy_env(command, "HTTP_PROXY", http);
            }
            if let Some(no_proxy) = &proxy.no_proxy {
                set_proxy_env(command, "NO_PROXY", no_proxy);
            }
        }
    }
}

/// 为 ureq 请求选择代理。回环地址与命中 NO_PROXY 的目标保持直连。
pub fn ureq_proxy_for(url: &str) -> Option<ureq::Proxy> {
    let proxy = resolve()?;
    let target = Url::parse(url).ok()?;
    let host = target.host_str()?;
    if is_loopback_host(target.host()?) || bypasses_proxy(host, proxy.no_proxy.as_deref()) {
        return None;
    }
    let proxy_url = if target.scheme() == "http" {
        proxy.http.as_ref().or(proxy.https.as_ref())
    } else {
        proxy.https.as_ref().or(proxy.http.as_ref())
    }?;
    ureq::Proxy::new(proxy_url).ok()
}

fn is_loopback_host(host: Host<&str>) -> bool {
    match host {
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
        Host::Domain(domain) => {
            let domain = domain.to_ascii_lowercase();
            domain == "localhost" || domain.ends_with(".localhost")
        }
    }
}

fn bypasses_proxy(host: &str, no_proxy: Option<&str>) -> bool {
    let Some(no_proxy) = no_proxy else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    no_proxy.split(',').any(|entry| {
        let entry = entry.trim().to_ascii_lowercase();
        if entry.is_empty() {
            return false;
        }
        if entry == "*" {
            return true;
        }
        if entry.eq_ignore_ascii_case("<local>") {
            return is_loopback_host(Host::Domain(host.as_str()));
        }
        // Windows 的 `127.*` / `192.168.*` 形式。
        if let Some(prefix) = entry.strip_suffix('*') {
            return host.starts_with(prefix);
        }
        let entry = entry.trim_start_matches('.');
        host == entry || host.ends_with(&format!(".{entry}"))
    })
}

/// 校验并规范化“指定代理”地址；不接受用户名密码，避免把凭据写入状态文件。
pub fn normalize_proxy_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("指定代理时需要填写代理地址。".to_string());
    }
    let url = Url::parse(trimmed)
        .map_err(|_| "代理地址格式不正确，例如 http://127.0.0.1:7890。".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("代理地址只支持 http:// 或 https://。".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("代理地址不能包含用户名或密码。".to_string());
    }
    if url.host().is_none() {
        return Err("代理地址缺少主机名。".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("代理地址不能包含查询参数或片段。".to_string());
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err("代理地址不能包含路径。".to_string());
    }
    Ok(format!("{}://{}", url.scheme(), url.authority()))
}

fn env_var(name: &str) -> Option<String> {
    let read = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    if let Some(value) = read(name) {
        return Some(value);
    }
    // Windows 的变量查找本身不区分大小写；Unix 需要单独检查。
    #[cfg(not(windows))]
    if let Some(value) = read(&name.to_ascii_lowercase()) {
        return Some(value);
    }
    None
}

fn set_proxy_env(command: &mut Command, name: &str, value: &str) {
    command.env(name, value);
    #[cfg(not(windows))]
    command.env(name.to_ascii_lowercase(), value);
}

fn remove_proxy_env(command: &mut Command) {
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"] {
        command.env_remove(name);
        #[cfg(not(windows))]
        command.env_remove(name.to_ascii_lowercase());
    }
}

/// 跟随系统：环境变量优先，其次读取系统设置。
fn system_proxy() -> Option<ResolvedProxy> {
    if let Some(all) = env_var("ALL_PROXY") {
        return Some(ResolvedProxy {
            http: Some(all.clone()),
            https: Some(all),
            no_proxy: env_var("NO_PROXY"),
        });
    }

    let http = env_var("HTTP_PROXY");
    let https = env_var("HTTPS_PROXY");
    if http.is_none() && https.is_none() {
        return detect_os();
    }

    let no_proxy = env_var("NO_PROXY");
    let http = http.or_else(|| https.clone());
    let https = https.or_else(|| http.clone());
    Some(ResolvedProxy {
        http,
        https,
        no_proxy,
    })
}

fn detect_os() -> Option<ResolvedProxy> {
    platform::detect()
}

/// Windows 的 `ProxyServer` 有两种写法：`host:port` 或
/// `http=host:port;https=host:port`。统一补成带 scheme 的地址。
fn parse_proxy_server(raw: &str) -> (Option<String>, Option<String>) {
    let raw = raw.trim();
    if raw.is_empty() {
        return (None, None);
    }

    if raw.contains('=') {
        let mut http = None;
        let mut https = None;
        for part in raw.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let value = normalize_proxy_value(value);
            match key.trim().to_ascii_lowercase().as_str() {
                "http" => http = value,
                "https" => https = value,
                // SOCKS 代理不能通过 HTTP(S)_PROXY 表达；留给用户自行设置
                // ALL_PROXY，避免把流量错误地导向 HTTP 代理。
                _ => {}
            }
        }
        (http, https)
    } else {
        let value = normalize_proxy_value(raw);
        (value.clone(), value)
    }
}

fn normalize_proxy_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.contains("://") {
        Some(raw.to_string())
    } else {
        Some(format!("http://{raw}"))
    }
}

/// 把 Windows 的 `ProxyOverride` 转成 `NO_PROXY`。
/// `<local>` 展开为回环地址；`*.example.com` 转成 `.example.com`。
fn parse_no_proxy(raw: &str) -> Option<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut bypass_local = false;
    for part in raw.split(';') {
        let value = part.trim();
        if value.is_empty() {
            continue;
        }
        if value.eq_ignore_ascii_case("<local>") {
            bypass_local = true;
            continue;
        }
        let value = value
            .strip_prefix("*.")
            .map(|suffix| format!(".{suffix}"))
            .unwrap_or_else(|| value.to_string());
        entries.push(value);
    }
    if bypass_local {
        entries.extend([
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "::1".to_string(),
        ]);
    }
    let mut deduped: Vec<String> = Vec::new();
    for entry in entries {
        if !deduped
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&entry))
        {
            deduped.push(entry);
        }
    }
    if deduped.is_empty() {
        None
    } else {
        Some(deduped.join(","))
    }
}

#[cfg(windows)]
mod platform {
    use super::{parse_no_proxy, parse_proxy_server, ResolvedProxy};
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_string(name: &str) -> Option<String> {
        let key = wide(INTERNET_SETTINGS);
        let value = wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;

        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
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
                HKEY_CURRENT_USER,
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

    fn read_dword(name: &str) -> Option<u32> {
        let key = wide(INTERNET_SETTINGS);
        let value = wide(name);
        let mut kind = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut data = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                &mut kind,
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (status == 0).then_some(data)
    }

    pub(super) fn detect() -> Option<ResolvedProxy> {
        if read_dword("ProxyEnable") != Some(1) {
            return None;
        }
        let server = read_string("ProxyServer")?;
        let (http, https) = parse_proxy_server(&server);
        if http.is_none() && https.is_none() {
            return None;
        }
        Some(ResolvedProxy {
            http,
            https,
            no_proxy: read_string("ProxyOverride").and_then(|value| parse_no_proxy(&value)),
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ResolvedProxy;

    fn build(host: Option<String>, port: Option<u16>) -> Option<String> {
        let host = host?;
        let port = port?;
        Some(format!("http://{host}:{port}"))
    }

    pub(super) fn detect() -> Option<ResolvedProxy> {
        let output = std::process::Command::new("/usr/sbin/scutil")
            .arg("--proxy")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);

        let mut http_enabled = false;
        let mut https_enabled = false;
        let mut http_host = None;
        let mut http_port = None;
        let mut https_host = None;
        let mut https_port = None;
        let mut exceptions: Vec<String> = Vec::new();
        let mut in_exceptions = false;

        for line in text.lines() {
            let line = line.trim();
            if in_exceptions {
                if line.starts_with('}') {
                    in_exceptions = false;
                } else if let Some((_, value)) = line.split_once(':') {
                    let value = value.trim();
                    if !value.is_empty() {
                        exceptions.push(value.to_string());
                    }
                }
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "HTTPEnable" => http_enabled = value == "1",
                "HTTPProxy" => http_host = Some(value.to_string()),
                "HTTPPort" => http_port = value.parse::<u16>().ok(),
                "HTTPSEnable" => https_enabled = value == "1",
                "HTTPSProxy" => https_host = Some(value.to_string()),
                "HTTPSPort" => https_port = value.parse::<u16>().ok(),
                "ExceptionsList" => in_exceptions = true,
                _ => {}
            }
        }

        let http = if http_enabled {
            build(http_host, http_port)
        } else {
            None
        };
        let https = if https_enabled {
            build(https_host, https_port)
        } else {
            None
        };
        if http.is_none() && https.is_none() {
            return None;
        }
        Some(ResolvedProxy {
            http,
            https,
            no_proxy: (!exceptions.is_empty()).then(|| exceptions.join(",")),
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::ResolvedProxy;

    pub(super) fn detect() -> Option<ResolvedProxy> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_proxy_server_applies_to_both_schemes() {
        let (http, https) = parse_proxy_server("127.0.0.1:7890");
        assert_eq!(http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(https.as_deref(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn per_scheme_proxy_server_is_split() {
        let (http, https) =
            parse_proxy_server("http=10.0.0.2:8080;https=secure.proxy:8443;socks=127.0.0.1:1080");
        assert_eq!(http.as_deref(), Some("http://10.0.0.2:8080"));
        assert_eq!(https.as_deref(), Some("http://secure.proxy:8443"));
    }

    #[test]
    fn proxy_override_becomes_no_proxy() {
        let value = parse_no_proxy("<local>;*.example.com;localhost").unwrap();
        assert!(value.contains(".example.com"));
        assert!(value.contains("127.0.0.1"));
        assert_eq!(value.matches("localhost").count(), 1);
    }

    #[test]
    fn empty_override_has_no_no_proxy() {
        assert!(parse_no_proxy("  ;  ").is_none());
    }

    #[test]
    fn manual_proxy_url_is_normalized_and_credentials_are_rejected() {
        assert_eq!(
            normalize_proxy_url(" 127.0.0.1:7890 ").unwrap_err(),
            "代理地址格式不正确，例如 http://127.0.0.1:7890。"
        );
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:7890/").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert!(normalize_proxy_url("https://proxy.example.com:8443").is_ok());
        assert!(normalize_proxy_url("socks5://127.0.0.1:1080").is_err());
        assert!(normalize_proxy_url("http://user:pass@127.0.0.1:7890").is_err());
        assert!(normalize_proxy_url("http://127.0.0.1:7890/path").is_err());
    }

    #[test]
    fn no_proxy_matching_covers_suffix_and_wildcards() {
        assert!(bypasses_proxy("relay.example.com", Some(".example.com")));
        assert!(bypasses_proxy("api.example.com", Some("example.com")));
        assert!(bypasses_proxy("127.0.0.1", Some("127.*")));
        assert!(bypasses_proxy("192.168.1.5", Some("192.168.*")));
        assert!(!bypasses_proxy("opencode.ai", Some(".example.com")));
        assert!(!bypasses_proxy("opencode.ai", None));
    }

    #[test]
    fn proxy_is_injected_when_no_env_override_exists() {
        let settings = Settings {
            mode: ProxyMode::Manual,
            manual_url: Some("http://127.0.0.1:7890".to_string()),
        };
        let mut command = Command::new("agy");
        apply_settings(&settings, &mut command);

        let envs: std::collections::HashMap<String, String> = command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((
                    key.to_string_lossy().to_string(),
                    value?.to_string_lossy().to_string(),
                ))
            })
            .collect();
        assert_eq!(
            envs.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            envs.get("NO_PROXY").map(String::as_str),
            Some(LOCAL_NO_PROXY)
        );
    }

    #[test]
    fn direct_mode_removes_inherited_proxy_env() {
        let settings = Settings {
            mode: ProxyMode::Direct,
            manual_url: None,
        };
        let mut command = Command::new("agy");
        apply_settings(&settings, &mut command);

        let removed: Vec<String> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().to_string())
            .collect();
        assert!(removed.iter().any(|name| name == "HTTPS_PROXY"));
        assert!(removed.iter().any(|name| name == "HTTP_PROXY"));
        assert!(removed.iter().any(|name| name == "ALL_PROXY"));
    }
}
