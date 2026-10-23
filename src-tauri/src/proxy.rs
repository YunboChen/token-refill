//! 系统代理检测。
//!
//! 只用系统公开配置为子进程补齐代理环境变量，不修改系统设置：
//! - Windows：当前用户的 Internet Settings（`ProxyEnable` / `ProxyServer` /
//!   `ProxyOverride`）；
//! - macOS：`scutil --proxy`；
//! - 其他平台：不主动探测，沿用进程已有的代理环境变量。
//!
//! 环境变量优先级高于系统设置：只要用户已经设置了 `HTTP_PROXY` /
//! `HTTPS_PROXY` / `ALL_PROXY`（大小写均可），就不再覆盖。PAC / 自动配置脚本
//! 无法安全地转换成代理地址，这里不做解析。

use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemProxy {
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Option<String>,
}

impl SystemProxy {
    /// 读取当前系统代理；未启用或读取失败时返回 `None`。
    pub fn detect() -> Option<Self> {
        platform::detect().filter(|proxy| proxy.http.is_some() || proxy.https.is_some())
    }

    /// 把系统代理写进子进程环境，已存在的环境变量保持原样。
    pub fn apply_to_command(&self, command: &mut Command) {
        if env_present("ALL_PROXY") {
            return;
        }
        if !env_present("HTTPS_PROXY") {
            if let Some(proxy) = self.https.as_ref().or(self.http.as_ref()) {
                command.env("HTTPS_PROXY", proxy);
            }
        }
        if !env_present("HTTP_PROXY") {
            if let Some(proxy) = self.http.as_ref().or(self.https.as_ref()) {
                command.env("HTTP_PROXY", proxy);
            }
        }
        if !env_present("NO_PROXY") {
            if let Some(no_proxy) = &self.no_proxy {
                command.env("NO_PROXY", no_proxy);
            }
        }
    }
}

fn env_present(name: &str) -> bool {
    if std::env::var_os(name).is_some() {
        return true;
    }
    // Unix 上大小写是两个不同的变量；Windows 的查找本身不区分大小写。
    #[cfg(not(windows))]
    if std::env::var_os(name.to_ascii_lowercase()).is_some() {
        return true;
    }
    false
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
            let value = normalize_proxy(value);
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
        let value = normalize_proxy(raw);
        (value.clone(), value)
    }
}

fn normalize_proxy(raw: &str) -> Option<String> {
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
    use super::{parse_no_proxy, parse_proxy_server, SystemProxy};
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

        // 第一次只取所需字节数；pvData 传空不会读取内容。
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

    pub(super) fn detect() -> Option<SystemProxy> {
        if read_dword("ProxyEnable") != Some(1) {
            return None;
        }
        let server = read_string("ProxyServer")?;
        let (http, https) = parse_proxy_server(&server);
        if http.is_none() && https.is_none() {
            return None;
        }
        Some(SystemProxy {
            http,
            https,
            no_proxy: read_string("ProxyOverride").and_then(|value| parse_no_proxy(&value)),
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::SystemProxy;

    fn build(host: Option<String>, port: Option<u16>) -> Option<String> {
        let host = host?;
        let port = port?;
        Some(format!("http://{host}:{port}"))
    }

    pub(super) fn detect() -> Option<SystemProxy> {
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
        Some(SystemProxy {
            http,
            https,
            no_proxy: (!exceptions.is_empty()).then(|| exceptions.join(",")),
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::SystemProxy;

    pub(super) fn detect() -> Option<SystemProxy> {
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
    fn proxy_is_injected_when_no_env_override_exists() {
        if env_present("ALL_PROXY")
            || env_present("HTTPS_PROXY")
            || env_present("HTTP_PROXY")
            || env_present("NO_PROXY")
        {
            return;
        }
        let proxy = SystemProxy {
            http: Some("http://127.0.0.1:7890".to_string()),
            https: Some("http://127.0.0.1:7890".to_string()),
            no_proxy: Some("localhost,127.0.0.1".to_string()),
        };
        let mut command = Command::new("agy");
        proxy.apply_to_command(&mut command);

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
            envs.get("HTTP_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            envs.get("NO_PROXY").map(String::as_str),
            Some("localhost,127.0.0.1")
        );
    }
}
