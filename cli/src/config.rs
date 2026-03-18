use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Mirrors the `[llm]` section of `~/.openpup/config.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub mini_model: String,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
}

/// Wrapper matching the toml structure `[llm]`
#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    llm: LlmConfig,
}

fn default_provider() -> String {
    "openai".into()
}
fn default_model() -> String {
    "gpt-4o".into()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            mini_model: String::new(),
            api_key: None,
            api_base: None,
        }
    }
}

pub fn openpup_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".openpup"))
}

pub fn load_llm_config() -> Result<LlmConfig> {
    let path = openpup_dir()?.join("config.toml");

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Cannot read config at {}", path.display()))?;

    let file: ConfigFile = toml::from_str(&raw).with_context(|| "Invalid config.toml")?;

    let mut cfg = file.llm;

    // env var override for API key
    if cfg.api_key.is_none() {
        cfg.api_key = std::env::var("OPENPUP_API_KEY")
            .ok()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    }

    Ok(cfg)
}
