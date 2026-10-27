//! 从 cc-switch、Claude Code 与 Codex 的本地配置中读取中转站地址与密钥。
//!
//! 只读以下位置里与 provider 有关的字段，不读取 `~/.codex/auth.json` 等登录态
//! 凭证文件，也不执行配置里的任何命令：
//! - cc-switch：`~/.cc-switch/cc-switch.db`（SQLite）`providers` 表中
//!   `app_type` 为 `claude` / `codex` 的行；Claude 行取 `settings_config.env`，
//!   Codex 行取 `settings_config.auth.OPENAI_API_KEY` 与 `config` TOML；
//! - Claude Code：`$CLAUDE_CONFIG_DIR/settings.json` 或 `~/.claude/settings.json`
//!   的 `env.ANTHROPIC_BASE_URL` + `env.ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY`；
//! - Codex：`$CODEX_HOME/config.toml` 或 `~/.codex/config.toml` 中
//!   `[model_providers.<id>]` 的 `base_url` 与 `env_key`/`experimental_bearer_token`。
//!
//! cc-switch 的数据库只以只读方式打开，不修改、不复制其中的任何数据。密钥只在
//! Rust 内存中短暂存在，不写日志、不返回前端；识别失败时只报告来源、地址与原因。

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use url::Url;

use crate::relay::RelayOrigin;

const MAX_CONFIG_BYTES: u64 = 512 * 1024;
const MAX_DB_BYTES: u64 = 64 * 1024 * 1024;
/// 扫描只列出候选，真正查询额度发生在用户勾选导入之后，因此上限可以放宽。
const MAX_CANDIDATES: usize = 64;

#[derive(Clone)]
pub(crate) struct ConfigCandidate {
    pub origin: RelayOrigin,
    /// cc-switch 的 `app_type`（`claude` / `codex`）；当前配置文件没有这一项。
    pub app: Option<String>,
    /// 源配置里的稳定标识：cc-switch 的 provider 行 id，或 Codex
    /// `[model_providers.<id>]` 的表名。用于重启后按引用找回同一条配置。
    pub source_id: Option<String>,
    /// cc-switch 中用户给 provider 起的名字；当前配置文件没有名字。
    pub label: Option<String>,
    pub base_url: String,
    pub api_key: String,
}

/// 扫描本机已知配置，按“cc-switch → 激活的 Codex provider → Claude Code”的顺序
/// 返回去重后的候选。cc-switch 中当前启用的 provider 排在前面。
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

    let mut candidates = read_cc_switch(&home.join(".cc-switch").join("cc-switch.db"));
    candidates.extend(read_codex(&codex_dir.join("config.toml")));
    candidates.extend(read_claude(&claude_dir.join("settings.json")));
    dedupe(candidates)
}

fn dedupe(candidates: Vec<ConfigCandidate>) -> Vec<ConfigCandidate> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for candidate in candidates {
        // 官方订阅/API 地址不是中转站，探测只会失败并制造噪声。
        if is_first_party(&candidate.base_url) {
            continue;
        }
        if !seen.insert((candidate.base_url.clone(), candidate.api_key.clone())) {
            continue;
        }
        result.push(candidate);
        if result.len() >= MAX_CANDIDATES {
            break;
        }
    }
    result
}

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

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn read_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

// ---------- cc-switch ----------

#[derive(Debug, Deserialize)]
struct ClaudeSettings {
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct CcSwitchCodex {
    #[serde(default)]
    auth: Option<CcSwitchCodexAuth>,
    #[serde(default)]
    config: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CcSwitchCodexAuth {
    #[serde(rename = "OPENAI_API_KEY", default)]
    openai_api_key: Option<String>,
}

/// 只读打开 cc-switch 的数据库；数据库不存在、结构不匹配或被占用时返回空列表。
fn read_cc_switch(path: &Path) -> Vec<ConfigCandidate> {
    let Ok(metadata) = fs::metadata(path) else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() > MAX_DB_BYTES {
        return Vec::new();
    }

    let Ok(connection) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    // cc-switch 写入时会短暂持有写锁；读不到就当作本轮没有候选，不阻塞页面。
    let _ = connection.busy_timeout(Duration::from_millis(300));

    let Ok(mut statement) = connection.prepare(
        "SELECT id, app_type, name, settings_config FROM providers \
         WHERE app_type IN ('claude', 'codex') \
         ORDER BY is_current DESC, app_type, name",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    }) else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    for (id, app_type, name, settings_config) in rows.flatten() {
        if let Some(candidate) = cc_switch_candidate(&id, &app_type, &name, &settings_config) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn cc_switch_candidate(
    provider_id: &str,
    app_type: &str,
    name: &str,
    settings: &str,
) -> Option<ConfigCandidate> {
    let (base_url, api_key) = match app_type {
        "claude" => {
            let settings: ClaudeSettings = serde_json::from_str(settings).ok()?;
            claude_values(&settings.env)?
        }
        "codex" => {
            let settings: CcSwitchCodex = serde_json::from_str(settings).ok()?;
            let config = settings.config.unwrap_or_default();
            let base_url = codex_base_url(&config)?;
            let api_key = settings
                .auth
                .and_then(|auth| auth.openai_api_key)
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty())
                .or_else(|| codex_config_key(&config))?;
            (base_url, api_key)
        }
        _ => return None,
    };

    Some(ConfigCandidate {
        origin: RelayOrigin::CcSwitch,
        app: Some(app_type.to_string()),
        source_id: non_empty(Some(provider_id)).map(str::to_string),
        label: non_empty(Some(name)).map(str::to_string),
        base_url,
        api_key,
    })
}

// ---------- Claude Code ----------

fn read_claude(path: &Path) -> Vec<ConfigCandidate> {
    read_text(path)
        .and_then(|text| claude_candidates(&text))
        .into_iter()
        .collect()
}

fn claude_candidates(text: &str) -> Option<ConfigCandidate> {
    let settings: ClaudeSettings = serde_json::from_str(text).ok()?;
    let (base_url, api_key) = claude_values(&settings.env)?;
    Some(ConfigCandidate {
        origin: RelayOrigin::ClaudeCode,
        app: None,
        source_id: None,
        label: None,
        base_url,
        api_key,
    })
}

fn claude_values(env: &BTreeMap<String, String>) -> Option<(String, String)> {
    let base_url = non_empty(env.get("ANTHROPIC_BASE_URL").map(String::as_str))?;
    let api_key = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .map(String::as_str)
        .and_then(|value| non_empty(Some(value)))?;
    Some((base_url.to_string(), api_key.to_string()))
}

// ---------- Codex ----------

#[derive(Debug, Default, Deserialize)]
struct CodexConfig {
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    model_providers: BTreeMap<String, CodexProvider>,
    /// 旧版 Codex / cc-switch 可能把地址写在顶层。
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    openai_base_url: Option<String>,
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
        let Some(base_url) = non_empty(provider.base_url.as_deref()) else {
            continue;
        };
        let api_key = non_empty(provider.experimental_bearer_token.as_deref())
            .map(str::to_string)
            .or_else(|| {
                let name = non_empty(provider.env_key.as_deref())?;
                lookup_env(name).filter(|value| !value.trim().is_empty())
            });
        let Some(api_key) = api_key else {
            continue;
        };

        let candidate = ConfigCandidate {
            origin: RelayOrigin::Codex,
            app: None,
            source_id: Some(id.clone()),
            label: None,
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

/// cc-switch 的 Codex provider：地址优先取激活的 `[model_providers.<id>]`，
/// 其次取顶层 `base_url` / `openai_base_url`；都没有时仅在恰好一个 provider
/// 带地址的情况下采用它，避免把无关的旧 provider 当成当前路由。
fn codex_base_url(text: &str) -> Option<String> {
    let config: CodexConfig = toml::from_str(text).ok()?;
    if let Some(id) = non_empty(config.model_provider.as_deref()) {
        if let Some(provider) = config.model_providers.get(id) {
            if let Some(base_url) = non_empty(provider.base_url.as_deref()) {
                return Some(base_url.to_string());
            }
        }
    }
    for base_url in [
        config.base_url.as_deref(),
        config.openai_base_url.as_deref(),
    ] {
        if let Some(base_url) = non_empty(base_url) {
            return Some(base_url.to_string());
        }
    }

    let mut urls = config
        .model_providers
        .values()
        .filter_map(|provider| non_empty(provider.base_url.as_deref()));
    let first = urls.next()?;
    if urls.next().is_some() {
        return None;
    }
    Some(first.to_string())
}

/// cc-switch 的密钥主要在 `auth.OPENAI_API_KEY`；这里只兜底读取 config 里
/// 激活 provider 的 `experimental_bearer_token` 或 `env_key` 指向的环境变量。
fn codex_config_key(text: &str) -> Option<String> {
    let config: CodexConfig = toml::from_str(text).ok()?;
    let provider = non_empty(config.model_provider.as_deref())
        .and_then(|id| config.model_providers.get(id))
        .or_else(|| {
            config
                .model_providers
                .values()
                .find(|provider| non_empty(provider.experimental_bearer_token.as_deref()).is_some())
        })?;

    if let Some(token) = non_empty(provider.experimental_bearer_token.as_deref()) {
        return Some(token.to_string());
    }
    let name = non_empty(provider.env_key.as_deref())?;
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::params;

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
        assert_eq!(candidates[0].source_id.as_deref(), Some("relay"));
        assert_eq!(candidates[1].source_id.as_deref(), Some("backup"));
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

    #[test]
    fn cc_switch_claude_provider_is_read_from_env() {
        let candidate = cc_switch_candidate(
            "claude-uuid",
            "claude",
            "千问AI平台 Token Plan",
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com",
                "ANTHROPIC_AUTH_TOKEN":"sk-cc"}}"#,
        )
        .unwrap();

        assert_eq!(candidate.origin, RelayOrigin::CcSwitch);
        assert_eq!(candidate.app.as_deref(), Some("claude"));
        assert_eq!(candidate.source_id.as_deref(), Some("claude-uuid"));
        assert_eq!(candidate.label.as_deref(), Some("千问AI平台 Token Plan"));
        assert_eq!(candidate.base_url, "https://relay.example.com");
        assert_eq!(candidate.api_key, "sk-cc");
    }

    #[test]
    fn cc_switch_codex_provider_reads_auth_key_and_active_base_url() {
        let settings = serde_json::json!({
            "auth": {"OPENAI_API_KEY": "sk-cc"},
            "config": concat!(
                "model_provider = \"relay\"\n",
                "[model_providers.relay]\n",
                "base_url = \"https://relay.example.com/v1\"\n",
                "[model_providers.other]\n",
                "base_url = \"https://other.example.com\"\n",
            ),
        })
        .to_string();
        let candidate =
            cc_switch_candidate("codex-uuid", "codex", "Codex Relay", &settings).unwrap();

        assert_eq!(candidate.origin, RelayOrigin::CcSwitch);
        assert_eq!(candidate.app.as_deref(), Some("codex"));
        assert_eq!(candidate.base_url, "https://relay.example.com/v1");
        assert_eq!(candidate.api_key, "sk-cc");
    }

    #[test]
    fn cc_switch_codex_provider_falls_back_to_bearer_token() {
        let settings = serde_json::json!({
            "auth": {},
            "config": concat!(
                "model_provider = \"relay\"\n",
                "[model_providers.relay]\n",
                "base_url = \"https://relay.example.com\"\n",
                "experimental_bearer_token = \"sk-toml\"\n",
            ),
        })
        .to_string();
        let candidate =
            cc_switch_candidate("codex-uuid", "codex", "Codex Relay", &settings).unwrap();
        assert_eq!(candidate.api_key, "sk-toml");
    }

    #[test]
    fn cc_switch_codex_without_model_provider_uses_single_table() {
        let config = concat!(
            "[model_providers.only]\n",
            "base_url = \"https://relay.example.com\"\n",
            "experimental_bearer_token = \"sk-only\"\n",
        );
        let settings = serde_json::json!({"auth": {}, "config": config}).to_string();
        let candidate = cc_switch_candidate("codex-uuid", "codex", "Only", &settings).unwrap();
        assert_eq!(candidate.base_url, "https://relay.example.com");
    }

    #[test]
    fn cc_switch_rows_without_key_or_url_are_ignored() {
        assert!(cc_switch_candidate("id", "claude", "官方", r#"{"env":{}}"#).is_none());
        assert!(cc_switch_candidate("id", "codex", "官方", r#"{"auth":{},"config":""}"#).is_none());
        assert!(cc_switch_candidate("id", "gemini", "Gemini", r#"{"env":{}}"#).is_none());
        assert!(cc_switch_candidate("id", "claude", "坏数据", "{not json").is_none());
    }

    fn write_cc_switch_db(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join("cc-switch.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE providers (
                    id TEXT NOT NULL,
                    app_type TEXT NOT NULL,
                    name TEXT NOT NULL,
                    settings_config TEXT NOT NULL,
                    is_current BOOLEAN NOT NULL DEFAULT 0,
                    PRIMARY KEY (id, app_type)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO providers (id, app_type, name, settings_config, is_current) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "claude-1",
                    "claude",
                    "Claude Relay",
                    r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com",
                        "ANTHROPIC_AUTH_TOKEN":"sk-claude"}}"#,
                    true,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO providers (id, app_type, name, settings_config, is_current) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "codex-1",
                    "codex",
                    "Codex Relay",
                    serde_json::json!({
                        "auth": {"OPENAI_API_KEY": "sk-codex"},
                        "config": "[model_providers.relay]\nbase_url = \"https://codex.example.com\"\n",
                    })
                    .to_string(),
                    false,
                ],
            )
            .unwrap();
        path
    }

    #[test]
    fn reads_both_app_types_from_a_cc_switch_database() {
        let dir = env::temp_dir().join(format!(
            "token-refill-cc-switch-{}-{}",
            std::process::id(),
            "read"
        ));
        let path = write_cc_switch_db(&dir);

        let candidates = read_cc_switch(&path);
        assert_eq!(candidates.len(), 2);
        assert!(candidates
            .iter()
            .all(|item| item.origin == RelayOrigin::CcSwitch));
        assert!(candidates
            .iter()
            .any(|item| item.app.as_deref() == Some("claude") && item.api_key == "sk-claude"));
        assert!(candidates
            .iter()
            .any(|item| item.app.as_deref() == Some("codex") && item.api_key == "sk-codex"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_or_missing_cc_switch_database_is_ignored() {
        let dir = env::temp_dir().join(format!(
            "token-refill-cc-switch-{}-{}",
            std::process::id(),
            "broken"
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cc-switch.db");
        fs::write(&path, "not a sqlite database").unwrap();

        assert!(read_cc_switch(&path).is_empty());
        assert!(read_cc_switch(&dir.join("missing.db")).is_empty());

        let _ = fs::remove_dir_all(&dir);
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
        assert!(read_cc_switch(&missing.join("cc-switch.db")).is_empty());
    }

    #[test]
    fn first_party_endpoints_are_filtered() {
        assert!(is_first_party("https://api.anthropic.com/v1"));
        assert!(is_first_party("https://api.openai.com/v1"));
        assert!(is_first_party("https://chatgpt.com/backend-api"));
        assert!(!is_first_party("https://relay.example.com/v1"));
    }

    #[test]
    fn duplicate_and_first_party_candidates_are_removed_in_order() {
        let candidate = |origin, base_url: &str, api_key: &str| ConfigCandidate {
            origin,
            app: None,
            source_id: None,
            label: None,
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
        };
        let candidates = dedupe(vec![
            candidate(RelayOrigin::CcSwitch, "https://relay.example.com", "sk-one"),
            candidate(
                RelayOrigin::ClaudeCode,
                "https://api.anthropic.com",
                "sk-official",
            ),
            candidate(
                RelayOrigin::ClaudeCode,
                "https://relay.example.com",
                "sk-one",
            ),
            candidate(RelayOrigin::Codex, "https://second.example.com", "sk-two"),
        ]);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].origin, RelayOrigin::CcSwitch);
        assert_eq!(candidates[1].origin, RelayOrigin::Codex);
    }

    #[test]
    fn malformed_codex_config_is_ignored() {
        assert!(codex_candidates("not toml [", |_| None).is_none());
        assert!(codex_candidates("", |_| None).unwrap().is_empty());
    }
}
