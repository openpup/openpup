//! Unified application configuration — `~/.openpup/config.toml`
//!
//! All config fields have sensible defaults so a missing or empty file
//! never causes a panic.

use anyhow::{Context, Result};
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
    pub pups: PupsConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub bridge: Option<crate::bridge::types::BridgeConfig>,
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
pub struct PupsConfig {
    #[serde(default = "default_enabled_pups")]
    pub enabled: Vec<String>,
}

/// Skills configuration — user-installed skill directory search paths.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    /// Extra directories to scan for user-created skills.
    /// Supports `~` expansion (e.g. "~/.openpup/my_skills" or "~/custom_skills").
    /// If empty, only built-in skills and ~/.openpup/skills_cache/ are loaded.
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
impl Default for PupsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled_pups(),
        }
    }
}

/// Expand `~` in a path string to the home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(&path[2..])
    } else {
        PathBuf::from(path)
    }
}

// ── I/O ──────────────────────────────────────────────────────────────────────

pub fn app_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("OPENPUP_APP_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".openpup"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(app_root()?.join("config.toml"))
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
