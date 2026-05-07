//! Unified application configuration — `~/.openpup/config.toml`
//!
//! All config fields have sensible defaults so a missing or empty file
//! never causes a panic.

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

/// Top-level config file structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub app: AppSettings,
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    #[serde(default)]
    pub pups: PupsConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub bridge: Option<crate::bridge::types::BridgeConfig>,
    #[serde(default)]
    pub xmtp: XmtpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// "openai" or "ollama"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Primary model (e.g. "gpt-4o", "deepseek-ai/DeepSeek-V3.2")
    #[serde(default = "default_model")]
    pub model: String,
    /// Cheaper model for classification tasks
    #[serde(default)]
    pub mini_model: String,
    /// API key (or set OPENPUP_API_KEY env var)
    #[serde(default)]
    pub api_key: String,
    /// Base URL override (leave empty for provider default)
    #[serde(default)]
    pub api_base: String,
    /// Embedding model
    #[serde(default = "default_embed_model")]
    pub embed_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct XmtpConfig {
    /// XMTP EOA private key, stored as enc2 in config.toml.
    #[serde(default)]
    pub identity_private_key: String,
    /// XMTP local DB encryption key, stored as enc2 in config.toml.
    #[serde(default)]
    pub db_encryption_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// "leashed" or "freerun"
    #[serde(default = "default_execution_mode")]
    pub execution_mode: String,
    /// "dark" or "light"
    #[serde(default = "default_theme")]
    pub theme: String,
    /// "zh" or "en"
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeConfig {
    /// Whether OpenPup may auto-ingest conversation summaries into KB.
    #[serde(default = "default_kb_auto_ingest")]
    pub auto_ingest_summaries: bool,
    /// Whether OpenPup may auto-ingest collaboration artifacts into KB.
    #[serde(default = "default_kb_auto_ingest")]
    pub auto_ingest_artifacts: bool,
    /// "frequent", "standard", or "conservative".
    #[serde(default = "default_kb_summary_frequency")]
    pub summary_frequency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PupsConfig {
    #[serde(default = "default_enabled_pups")]
    pub enabled: Vec<String>,
}

/// Skills configuration — user-installed skill directory search paths.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    /// Extra directories to scan for user-created skills.
    /// Supports `~` expansion (e.g. "~/.openpup/my_skills" or "~/custom_skills").
    /// If empty, only built-in skills and ~/.openpup/skills/ are loaded.
    #[serde(default)]
    pub search_paths: Vec<String>,
}

// ── default helpers ──────────────────────────────────────────────────────────

fn default_provider() -> String {
    "openai".into()
}
fn default_model() -> String {
    "gpt-4o".into()
}
fn default_embed_model() -> String {
    "BAAI/bge-m3".into()
}
fn default_execution_mode() -> String {
    "leashed".into()
}
fn default_theme() -> String {
    "dark".into()
}
fn default_language() -> String {
    "zh".into()
}
fn default_enabled_pups() -> Vec<String> {
    vec!["alpha", "dev", "writer", "ops", "research", "life_admin"]
        .into_iter()
        .map(String::from)
        .collect()
}
fn default_kb_auto_ingest() -> bool {
    true
}
fn default_kb_summary_frequency() -> String {
    "standard".into()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            mini_model: String::new(),
            api_key: String::new(),
            api_base: String::new(),
            embed_model: default_embed_model(),
        }
    }
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            execution_mode: default_execution_mode(),
            theme: default_theme(),
            language: default_language(),
        }
    }
}
impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            auto_ingest_summaries: default_kb_auto_ingest(),
            auto_ingest_artifacts: default_kb_auto_ingest(),
            summary_frequency: default_kb_summary_frequency(),
        }
    }
}
impl Default for PupsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled_pups(),
        }
    }
}

/// Expand `~` in a path string to the home directory.
/// Falls back to `OPENPUP_APP_ROOT` on platforms where `dirs::home_dir()`
/// is unavailable (e.g. Android / iOS).
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = dirs::home_dir()
            .or_else(|| std::env::var("OPENPUP_APP_ROOT").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(stripped)
    } else {
        PathBuf::from(path)
    }
}

// ── I/O ──────────────────────────────────────────────────────────────────────

pub fn app_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("OPENPUP_APP_ROOT") {
        return Ok(PathBuf::from(root));
    }
    if let Some(home) = dirs::home_dir() {
        return Ok(home.join(".openpup"));
    }
    // Fallback for mobile platforms where neither OPENPUP_APP_ROOT nor
    // home_dir() is available.  Derive the app-private files directory
    // from the process UID on Android; on other platforms give up.
    #[cfg(target_os = "android")]
    {
        if let Some(dir) = android_app_root_fallback() {
            return Ok(dir);
        }
    }
    anyhow::bail!(
        "cannot determine app root: set OPENPUP_APP_ROOT or ensure a home directory exists"
    )
}

/// Last-resort app root on Android: read the process UID from
/// `/proc/self/status` and construct the canonical data path.
#[cfg(target_os = "android")]
fn android_app_root_fallback() -> Option<PathBuf> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let uid_line = status.lines().find(|l| l.starts_with("Uid:"))?;
    let uid: u32 = uid_line.split_whitespace().nth(1)?.parse().ok()?;
    let android_user_id = uid / 100_000;
    // com.openpup.app is the only consumer; hardcode is acceptable as
    // a last-ditch fallback.
    Some(PathBuf::from(format!(
        "/data/user/{android_user_id}/com.openpup.app/files/openpup-mobile"
    )))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(app_root()?.join("config.toml"))
}

fn random_hex(bytes: usize) -> String {
    let mut value = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut value);
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn ensure_xmtp_config() -> Result<XmtpConfig> {
    let mut cfg = load();
    let mut changed = false;
    if cfg.xmtp.identity_private_key.trim().is_empty() {
        cfg.xmtp.identity_private_key = format!("0x{}", random_hex(32));
        changed = true;
    }
    if cfg.xmtp.db_encryption_key.trim().is_empty() {
        cfg.xmtp.db_encryption_key = random_hex(32);
        changed = true;
    }
    let xmtp = cfg.xmtp.clone();
    if changed {
        save(&cfg)?;
    }
    Ok(xmtp)
}

/// Load config from `~/.openpup/config.toml`.
/// Returns defaults if the file does not exist.
/// `api_key` is transparently decrypted if stored as enc2.
pub fn load() -> AppConfig {
    let Ok(path) = config_path() else {
        return AppConfig::default();
    };
    if !path.exists() {
        return AppConfig::default();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AppConfig::default();
    };
    let mut cfg: AppConfig = toml::from_str(&text).unwrap_or_default();
    // Transparent decrypt — in-memory value is always plaintext
    match crate::crypto::ensure_decrypted(&cfg.llm.api_key) {
        Ok(plain) => cfg.llm.api_key = plain,
        Err(e) => warn!("failed to decrypt api_key: {e}"),
    }
    match crate::crypto::ensure_decrypted(&cfg.xmtp.identity_private_key) {
        Ok(plain) => cfg.xmtp.identity_private_key = plain,
        Err(e) => warn!("failed to decrypt xmtp identity_private_key: {e}"),
    }
    match crate::crypto::ensure_decrypted(&cfg.xmtp.db_encryption_key) {
        Ok(plain) => cfg.xmtp.db_encryption_key = plain,
        Err(e) => warn!("failed to decrypt xmtp db_encryption_key: {e}"),
    }
    cfg
}

/// Persist config to `~/.openpup/config.toml`.
/// `api_key` is transparently encrypted before writing (enc2 scheme).
pub fn save(cfg: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Clone and encrypt api_key — never write plaintext to disk
    let mut cfg_to_write = cfg.clone();
    if !cfg_to_write.llm.api_key.is_empty() {
        cfg_to_write.llm.api_key = crate::crypto::ensure_encrypted(&cfg_to_write.llm.api_key)
            .context("encrypt api_key")?;
    }
    if !cfg_to_write.xmtp.identity_private_key.is_empty() {
        cfg_to_write.xmtp.identity_private_key =
            crate::crypto::ensure_encrypted(&cfg_to_write.xmtp.identity_private_key)
                .context("encrypt xmtp identity_private_key")?;
    }
    if !cfg_to_write.xmtp.db_encryption_key.is_empty() {
        cfg_to_write.xmtp.db_encryption_key =
            crate::crypto::ensure_encrypted(&cfg_to_write.xmtp.db_encryption_key)
                .context("encrypt xmtp db_encryption_key")?;
    }
    let text = toml::to_string_pretty(&cfg_to_write).context("serialize config")?;
    std::fs::write(&path, text).context("write config.toml")?;
    Ok(())
}

/// Load config and apply env-var overrides.
pub fn load_with_env() -> AppConfig {
    let mut cfg = load();
    // env vars take precedence over file
    if let Ok(v) = std::env::var("OPENPUP_API_KEY") {
        cfg.llm.api_key = v;
    }
    if let Ok(v) = std::env::var("OPENAI_API_KEY") {
        if cfg.llm.api_key.is_empty() {
            cfg.llm.api_key = v;
        }
    }
    if let Ok(v) = std::env::var("OPENAI_BASE_URL") {
        if cfg.llm.api_base.is_empty() {
            cfg.llm.api_base = v;
        }
    }
    if let Ok(v) = std::env::var("OPENPUP_LLM_PROVIDER") {
        cfg.llm.provider = v;
    }
    if let Ok(v) = std::env::var("OPENAI_MODEL") {
        if cfg.llm.model == default_model() {
            cfg.llm.model = v;
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::{config_path, load, save, AppConfig};
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn knowledge_config_round_trips_through_config_file() {
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempfile::tempdir().expect("tempdir");
        let _root = EnvVarGuard::set("OPENPUP_APP_ROOT", temp.path());

        let mut cfg = AppConfig::default();
        cfg.knowledge.auto_ingest_summaries = false;
        cfg.knowledge.auto_ingest_artifacts = true;
        cfg.knowledge.summary_frequency = "conservative".to_string();
        save(&cfg).expect("save config");

        let saved_path = config_path().expect("config path");
        assert!(saved_path.exists(), "config file should be written");

        let loaded = load();
        assert!(!loaded.knowledge.auto_ingest_summaries);
        assert!(loaded.knowledge.auto_ingest_artifacts);
        assert_eq!(loaded.knowledge.summary_frequency, "conservative");
    }
}
