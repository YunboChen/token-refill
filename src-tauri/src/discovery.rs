//! 从 Claude Code 与 Codex 的本地配置中读取中转站地址与密钥。
//!
//! 只读两类文件里与 provider 有关的字段，不读取 `~/.codex/auth.json` 等登录态
//! 凭证文件，也不执行配置里的任何命令：
//! - Claude Code：`$CLAUDE_CONFIG_DIR/settings.json` 或 `~/.claude/settings.json`
//!   的 `env.ANTHROPIC_BASE_URL` + `env.ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY`；
//! - Codex：`$CODEX_HOME/config.toml` 或 `~/.codex/config.toml` 中
//!   `[model_providers.<id>]` 的 `base_url` 与 `env_key`/`experimental_bearer_token`。
//!
//! 密钥只在 Rust 内存中短暂存在，不写日志、不返回前端；识别失败时只报告来源、
//! 地址与原因。

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use url::Url;

use crate::relay::RelayOrigin;

const MAX_CONFIG_BYTES: u64 = 512 * 1024;
const MAX_CANDIDATES: usize = 8;

#[derive(Clone)]
pub(crate) struct ConfigCandidate {
    pub origin: RelayOrigin,
    pub base_url: String,
    pub api_key: String,
}

/// 扫描本机已知配置文件，按“激活的 Codex provider → 其他 provider → Claude Code”
/// 的顺序返回去重后的候选，最多 8 条。
pub(crate) fn discover_candidates() -> Vec<ConfigCandidate> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };

    let claude_dir = env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"));
    let codex_dir = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));

    let mut candidates = read_codex(&codex_dir.join("config.toml"));
    candidates.extend(read_claude(&claude_dir.join("settings.json")));

    let mut seen = HashSet::new();
    candidates.retain(|candidate| {
        !is_first_party(&candidate.base_url)
            && seen.insert((candidate.base_url.clone(), candidate.api_key.clone()))
    });
    candidates.truncate(MAX_CANDIDATES);
    candidates
}

/// 官方订阅/API 地址不是中转站，探测只会失败并制造噪声。
fn is_first_party(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };
    match url.host_str() {
        Some(host) => {
            let host = host.to_ascii_lowercase();
            host == "api.anthropic.com"
                || host == "api.openai.com"
                || host == "chatgpt.com"
                || host.ends_with(".openai.com")
                || host.ends_with(".anthropic.com")
        }
        None => false,
    }
}

fn home_dir() -> Option<PathBuf> {
    // Windows 优先 USERPROFILE：HOME 可能被 Git Bash 等环境改成 POSIX 路径。
    // 用 `cfg!` 而不是条件编译，让两个分支在所有平台上都参与类型检查。
    let primary = if cfg!(target_os = "windows") {
        env::var_os("USERPROFILE")
    } else {
        env::var_os("HOME")
    };

    primary
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn read_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

// ---------- Claude Code ----------

#[derive(Debug, Deserialize)]
struct ClaudeSettings {
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn read_claude(path: &Path) -> Vec<ConfigCandidate> {
    read_text(path)
        .and_then(|text| claude_candidates(&text))
        .into_iter()
        .collect()
}

fn claude_candidates(text: &str) -> Option<ConfigCandidate> {
    let settings: ClaudeSettings = serde_json::from_str(text).ok()?;
    let base_url = settings
        .env
        .get("ANTHROPIC_BASE_URL")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;
    let api_key = settings
        .env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| settings.env.get("ANTHROPIC_API_KEY"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;

    Some(ConfigCandidate {
        origin: RelayOrigin::ClaudeCode,
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
    })
}

// ---------- Codex ----------

#[derive(Debug, Default, Deserialize)]
struct CodexConfig {
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    model_providers: BTreeMap<String, CodexProvider>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexProvider {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    env_key: Option<String>,
    #[serde(default)]
    experimental_bearer_token: Option<String>,
}

fn read_codex(path: &Path) -> Vec<ConfigCandidate> {
    read_text(path)
        .and_then(|text| codex_candidates(&text, |name| env::var(name).ok()))
        .unwrap_or_default()
}

fn codex_candidates(
    text: &str,
    lookup_env: impl Fn(&str) -> Option<String>,
) -> Option<Vec<ConfigCandidate>> {
    let config: CodexConfig = toml::from_str(text).ok()?;
    let active = config.model_provider.as_deref();
    let mut active_candidate = None;
    let mut others = Vec::new();

    for (id, provider) in &config.model_providers {
        let Some(base_url) = provider
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let api_key = provider
            .experimental_bearer_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                let name = provider.env_key.as_deref()?.trim();
                if name.is_empty() {
                    return None;
                }
                lookup_env(name).filter(|value| !value.trim().is_empty())
            });
        let Some(api_key) = api_key else {
            continue;
        };

        let candidate = ConfigCandidate {
            origin: RelayOrigin::Codex,
            base_url: base_url.to_string(),
            api_key,
        };
        if active == Some(id.as_str()) {
            active_candidate = Some(candidate);
        } else {
            others.push(candidate);
        }
    }

    // 激活的 provider 排在前面，其余保留供用户查看。
    let mut candidates = Vec::with_capacity(others.len() + 1);
    if let Some(candidate) = active_candidate {
        candidates.push(candidate);
    }
    candidates.extend(others);
    Some(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_provide_base_url_and_token() {
        let candidate = claude_candidates(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com/v1",
                "ANTHROPIC_AUTH_TOKEN":"sk-claude"}}"#,
        )
        .unwrap();

        assert_eq!(candidate.origin, RelayOrigin::ClaudeCode);
        assert_eq!(candidate.base_url, "https://relay.example.com/v1");
        assert_eq!(candidate.api_key, "sk-claude");
    }

    #[test]
    fn claude_api_key_fallback_and_missing_values() {
        let candidate = claude_candidates(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com",
                "ANTHROPIC_API_KEY":"sk-fallback"}}"#,
        )
        .unwrap();
        assert_eq!(candidate.api_key, "sk-fallback");

        assert!(claude_candidates(r#"{"env":{"ANTHROPIC_API_KEY":"sk-only"}}"#).is_none());
        assert!(claude_candidates(r#"{"env":{}}"#).is_none());
        assert!(claude_candidates("not json").is_none());
    }

    #[test]
    fn codex_prefers_active_provider_and_reads_env_key() {
        let config = r#"
model_provider = "relay"

[model_providers.relay]
base_url = "https://relay.example.com/v1"
env_key = "RELAY_KEY"

[model_providers.backup]
base_url = "https://backup.example.com"
experimental_bearer_token = "sk-backup"
"#;
        let candidates = codex_candidates(config, |name| {
            (name == "RELAY_KEY").then(|| "sk-active".to_string())
        })
        .unwrap();

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].base_url, "https://relay.example.com/v1");
        assert_eq!(candidates[0].api_key, "sk-active");
        assert_eq!(candidates[0].origin, RelayOrigin::Codex);
        assert_eq!(candidates[1].base_url, "https://backup.example.com");
        assert_eq!(candidates[1].api_key, "sk-backup");
    }

    #[test]
    fn codex_skips_providers_without_base_or_key() {
        let config = r#"
[model_providers.no_base]
env_key = "KEY"

[model_providers.no_key]
base_url = "https://relay.example.com"
env_key = "MISSING"

[model_providers.token]
base_url = "https://relay.example.com"
experimental_bearer_token = "sk-token"
"#;
        let candidates = codex_candidates(config, |_| None).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].api_key, "sk-token");
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "token-refill-discovery-{}-{}",
            std::process::id(),
            name
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_claude_settings_from_disk() {
        let path = temp_file(
            "settings.json",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com",
                "ANTHROPIC_AUTH_TOKEN":"sk-disk"}}"#,
        );
        let candidates = read_claude(&path);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].api_key, "sk-disk");
    }

    #[test]
    fn reads_codex_config_from_disk() {
        let path = temp_file(
            "config.toml",
            "[model_providers.relay]\nbase_url = \"https://relay.example.com\"\n\
             experimental_bearer_token = \"sk-disk\"\n",
        );
        let candidates = read_codex(&path);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].base_url, "https://relay.example.com");
    }

    #[test]
    fn missing_files_yield_no_candidates() {
        let missing = env::temp_dir().join("token-refill-discovery-missing");
        assert!(read_claude(&missing.join("settings.json")).is_empty());
        assert!(read_codex(&missing.join("config.toml")).is_empty());
    }

    #[test]
    fn first_party_endpoints_are_filtered() {
        assert!(is_first_party("https://api.anthropic.com/v1"));
        assert!(is_first_party("https://api.openai.com/v1"));
        assert!(is_first_party("https://chatgpt.com/backend-api"));
        assert!(!is_first_party("https://relay.example.com/v1"));
    }

    #[test]
    fn malformed_codex_config_is_ignored() {
        assert!(codex_candidates("not toml [", |_| None).is_none());
        assert!(codex_candidates("", |_| None).unwrap().is_empty());
    }
}
