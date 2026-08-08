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
    #[serde(default)]
    pub scenario: ScenarioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScenarioConfig {
    #[serde(default)]
    pub finance: FinanceScenarioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceScenarioConfig {
    #[serde(default = "default_finance_role_bindings")]
    pub role_bindings: Vec<FinanceRoleBinding>,
    #[serde(default = "default_finance_skill_bindings")]
    pub skill_bindings: Vec<FinanceSkillBinding>,
    #[serde(default = "default_finance_connector_bindings")]
    pub connector_bindings: Vec<FinanceConnectorBinding>,
    #[serde(default)]
    pub risk_preset: FinanceRiskPreset,
}

impl Default for FinanceScenarioConfig {
    fn default() -> Self {
        Self {
            role_bindings: default_finance_role_bindings(),
            skill_bindings: default_finance_skill_bindings(),
            connector_bindings: default_finance_connector_bindings(),
            risk_preset: FinanceRiskPreset::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceRoleBinding {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub pup_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceSkillBinding {
    #[serde(default)]
    pub skill: String,
    #[serde(default = "default_finance_skill_binding_mode")]
    pub mode: String,
    #[serde(default)]
    pub skill_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceConnectorBinding {
    #[serde(default)]
    pub connector: String,
    #[serde(default = "default_finance_connector_binding_mode")]
    pub mode: String,
    #[serde(default)]
    pub server_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceRiskPreset {
    #[serde(default = "default_true")]
    pub force_leashed: bool,
    #[serde(default = "default_true")]
    pub require_manual_approval: bool,
    #[serde(default = "default_single_position_limit_pct")]
    pub single_position_limit_pct: u32,
    #[serde(default = "default_single_sector_limit_pct")]
    pub single_sector_limit_pct: u32,
    #[serde(default = "default_daily_loss_circuit_breaker_pct")]
    pub daily_loss_circuit_breaker_pct: u32,
    #[serde(default = "default_board_lot_size")]
    pub board_lot_size: u32,
    #[serde(default = "default_true")]
    pub block_st_suspended_delisting: bool,
    #[serde(default = "default_true")]
    pub enforce_trading_window: bool,
    #[serde(default = "default_true")]
    pub enforce_t1: bool,
}

impl Default for FinanceRiskPreset {
    fn default() -> Self {
        Self {
            force_leashed: true,
            require_manual_approval: true,
            single_position_limit_pct: default_single_position_limit_pct(),
            single_sector_limit_pct: default_single_sector_limit_pct(),
            daily_loss_circuit_breaker_pct: default_daily_loss_circuit_breaker_pct(),
            board_lot_size: default_board_lot_size(),
            block_st_suspended_delisting: true,
            enforce_trading_window: true,
            enforce_t1: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Legacy compatibility mirror of the primary route provider.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Legacy compatibility mirror of the primary route model.
    #[serde(default = "default_model")]
    pub model: String,
    /// Legacy compatibility mirror of the mini route model.
    #[serde(default)]
    pub mini_model: String,
    /// Legacy compatibility mirror of the primary route API key.
    #[serde(default)]
    pub api_key: String,
    /// Legacy compatibility mirror of the primary route API base.
    #[serde(default)]
    pub api_base: String,
    /// Legacy compatibility mirror of the embedding route model.
    #[serde(default = "default_embed_model")]
    pub embed_model: String,
    /// Saved provider registry for flexible routing.
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    /// Per-slot model routing.
    #[serde(default)]
    pub routing: LlmRoutingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmProviderConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Provider kind used by the local runtime.
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    /// Logical provider/vendor name, e.g. "openai", "openrouter", "anthropic".
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_base: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmRoutingConfig {
    #[serde(default)]
    pub primary: LlmRouteTarget,
    #[serde(default)]
    pub mini: LlmRouteTarget,
    #[serde(default)]
    pub embedding: LlmRouteTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmRouteTarget {
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model: String,
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
    /// Hide to tray or minimize instead of quitting when the main window closes.
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    /// Start the desktop app automatically when the user logs in.
    #[serde(default)]
    pub launch_at_startup: bool,
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
    "openai_compatible".into()
}
fn default_provider_kind() -> String {
    "openai_compatible".into()
}
pub fn canonical_provider_value(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "openai_responses" => "openai_responses".to_string(),
        "anthropic" | "anthropic_messages" => "anthropic".to_string(),
        "ollama" => "ollama".to_string(),
        "openai_compatible" | "openai" | "openrouter" | "deepseek" | "siliconflow" => {
            "openai_compatible".to_string()
        }
        _ => default_provider(),
    }
}
pub fn infer_provider_kind(provider: &str) -> String {
    match canonical_provider_value(provider).as_str() {
        "openai_responses" => "openai_responses".to_string(),
        "anthropic" => "anthropic_messages".to_string(),
        "ollama" => "ollama".to_string(),
        _ => default_provider_kind(),
    }
}
fn normalize_provider_identity(provider: &mut LlmProviderConfig) {
    provider.provider = canonical_provider_value(&provider.provider);
    provider.kind = infer_provider_kind(&provider.provider);
}
fn default_provider_enabled() -> bool {
    true
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
fn default_minimize_to_tray_on_close() -> bool {
    true
}
fn default_enabled_pups() -> Vec<String> {
    vec!["alpha", "dev", "writer", "ops", "research", "life_admin"]
        .into_iter()
        .map(String::from)
        .collect()
}
fn default_true() -> bool {
    true
}
fn default_kb_auto_ingest() -> bool {
    true
}
fn default_kb_summary_frequency() -> String {
    "standard".into()
}
fn default_finance_skill_binding_mode() -> String {
    "scenario_preset".into()
}
fn default_finance_connector_binding_mode() -> String {
    "mcp_server".into()
}
fn default_single_position_limit_pct() -> u32 {
    20
}
fn default_single_sector_limit_pct() -> u32 {
    40
}
fn default_daily_loss_circuit_breaker_pct() -> u32 {
    3
}
fn default_board_lot_size() -> u32 {
    100
}
fn default_finance_role_bindings() -> Vec<FinanceRoleBinding> {
    vec![
        FinanceRoleBinding { role: "researcher".into(), pup_key: Some("research".into()) },
        FinanceRoleBinding { role: "strategist".into(), pup_key: Some("strategist".into()) },
        FinanceRoleBinding { role: "risk_officer".into(), pup_key: Some("risk_officer".into()) },
        FinanceRoleBinding { role: "executor".into(), pup_key: Some("executor".into()) },
        FinanceRoleBinding { role: "reviewer".into(), pup_key: Some("reviewer".into()) },
    ]
}
fn default_finance_skill_bindings() -> Vec<FinanceSkillBinding> {
    vec![
        FinanceSkillBinding { skill: "premarket_scan".into(), mode: default_finance_skill_binding_mode(), skill_name: None },
        FinanceSkillBinding { skill: "intraday_check".into(), mode: default_finance_skill_binding_mode(), skill_name: None },
        FinanceSkillBinding { skill: "postmarket_review".into(), mode: default_finance_skill_binding_mode(), skill_name: None },
        FinanceSkillBinding { skill: "watchlist_cleanup".into(), mode: default_finance_skill_binding_mode(), skill_name: None },
        FinanceSkillBinding { skill: "emergency_stop".into(), mode: default_finance_skill_binding_mode(), skill_name: None },
    ]
}
fn default_finance_connector_bindings() -> Vec<FinanceConnectorBinding> {
    vec![
        FinanceConnectorBinding { connector: "intel".into(), mode: default_finance_connector_binding_mode(), server_name: Some("intel".into()) },
        FinanceConnectorBinding { connector: "risk".into(), mode: default_finance_connector_binding_mode(), server_name: Some("risk".into()) },
        FinanceConnectorBinding { connector: "exec".into(), mode: default_finance_connector_binding_mode(), server_name: Some("exec".into()) },
    ]
}

pub fn default_api_base_for_provider(kind: &str, provider: &str) -> String {
    match canonical_provider_value(provider).as_str() {
        "ollama" => "http://127.0.0.1:11434/v1".to_string(),
        "anthropic" => "https://api.anthropic.com/v1".to_string(),
        "openai_responses" => "https://api.openai.com/v1".to_string(),
        _ if kind == "openai_responses" => "https://api.openai.com/v1".to_string(),
        _ if kind == "anthropic_messages" => "https://api.anthropic.com/v1".to_string(),
        _ => "https://api.openai.com/v1".to_string(),
    }
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
            providers: Vec::new(),
            routing: LlmRoutingConfig::default(),
        }
    }
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            execution_mode: default_execution_mode(),
            theme: default_theme(),
            language: default_language(),
            minimize_to_tray_on_close: default_minimize_to_tray_on_close(),
            launch_at_startup: false,
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
fn load_internal(resolve_secrets: bool) -> AppConfig {
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
    if resolve_secrets {
        match crate::crypto::ensure_decrypted(&cfg.llm.api_key) {
            Ok(plain) => cfg.llm.api_key = plain,
            Err(e) => warn!("failed to decrypt api_key: {e}"),
        }
        for provider in &mut cfg.llm.providers {
            match crate::crypto::ensure_decrypted(&provider.api_key) {
                Ok(plain) => provider.api_key = plain,
                Err(e) => warn!("failed to decrypt provider api_key ({}): {e}", provider.id),
            }
        }
        match crate::crypto::ensure_decrypted(&cfg.xmtp.identity_private_key) {
            Ok(plain) => cfg.xmtp.identity_private_key = plain,
            Err(e) => warn!("failed to decrypt xmtp identity_private_key: {e}"),
        }
        match crate::crypto::ensure_decrypted(&cfg.xmtp.db_encryption_key) {
            Ok(plain) => cfg.xmtp.db_encryption_key = plain,
            Err(e) => warn!("failed to decrypt xmtp db_encryption_key: {e}"),
        }
    }
    normalize_llm_config(&mut cfg.llm);
    cfg
}

/// Returns defaults if the file does not exist.
/// `api_key` is transparently decrypted if stored as enc2.
pub fn load() -> AppConfig {
    load_internal(true)
}

/// Lightweight config load for read-only settings/state snapshots.
pub fn load_fast() -> AppConfig {
    load_internal(false)
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
    normalize_llm_config(&mut cfg_to_write.llm);
    if !cfg_to_write.llm.api_key.is_empty() {
        cfg_to_write.llm.api_key = crate::crypto::ensure_encrypted(&cfg_to_write.llm.api_key)
            .context("encrypt api_key")?;
    }
    for provider in &mut cfg_to_write.llm.providers {
        if !provider.api_key.is_empty() {
            provider.api_key = crate::crypto::ensure_encrypted(&provider.api_key)
                .context("encrypt provider api_key")?;
        }
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
    apply_llm_env_overrides(&mut cfg.llm);
    normalize_llm_config(&mut cfg.llm);
    cfg
}

fn normalize_llm_config(llm: &mut LlmConfig) {
    migrate_legacy_provider(llm);
    ensure_provider_defaults(llm);
    ensure_routing_defaults(llm);
    sync_legacy_fields_from_routing(llm);
}

fn apply_llm_env_overrides(llm: &mut LlmConfig) {
    let primary_provider_id = llm.routing.primary.provider_id.clone();

    let primary_provider_index = llm
        .providers
        .iter_mut()
        .position(|provider| provider.id == primary_provider_id)
        .or_else(|| (!llm.providers.is_empty()).then_some(0));

    if let Some(index) = primary_provider_index {
        let primary_provider = &mut llm.providers[index];
        if let Ok(v) = std::env::var("OPENPUP_LLM_PROVIDER") {
            primary_provider.provider = v;
            normalize_provider_identity(primary_provider);
            if primary_provider.api_base.trim().is_empty() {
                primary_provider.api_base = default_api_base_for_provider(
                    &primary_provider.kind,
                    &primary_provider.provider,
                );
            }
        }

        if let Ok(v) = std::env::var("OPENPUP_API_KEY") {
            primary_provider.api_key = v;
        } else if let Ok(v) = std::env::var("OPENAI_API_KEY") {
            if primary_provider.api_key.is_empty() {
                primary_provider.api_key = v;
            }
        }

        if let Ok(v) = std::env::var("OPENAI_BASE_URL") {
            primary_provider.api_base = v;
        }
    }

    if let Ok(v) = std::env::var("OPENAI_MODEL") {
        llm.routing.primary.model = v;
    }
}

fn migrate_legacy_provider(llm: &mut LlmConfig) {
    if !llm.providers.is_empty() {
        return;
    }

    let id = "default".to_string();
    llm.provider = canonical_provider_value(&llm.provider);
    let kind = infer_provider_kind(&llm.provider);
    let name = if llm.provider.eq_ignore_ascii_case("ollama") {
        "Local Ollama".to_string()
    } else {
        "Default Provider".to_string()
    };
    let mut models = Vec::new();
    for model in [&llm.model, &llm.mini_model, &llm.embed_model] {
        let trimmed = model.trim();
        if !trimmed.is_empty() && !models.iter().any(|item| item == trimmed) {
            models.push(trimmed.to_string());
        }
    }
    llm.providers.push(LlmProviderConfig {
        id: id.clone(),
        name,
        kind,
        provider: llm.provider.clone(),
        api_base: llm.api_base.clone(),
        api_key: llm.api_key.clone(),
        enabled: true,
        models,
    });
    llm.routing.primary.provider_id = id.clone();
    llm.routing.primary.model = llm.model.clone();
    llm.routing.mini.provider_id = id.clone();
    llm.routing.mini.model = if llm.mini_model.trim().is_empty() {
        llm.model.clone()
    } else {
        llm.mini_model.clone()
    };
    llm.routing.embedding.provider_id = id;
    llm.routing.embedding.model = if llm.embed_model.trim().is_empty() {
        default_embed_model()
    } else {
        llm.embed_model.clone()
    };
}

fn ensure_provider_defaults(llm: &mut LlmConfig) {
    for (idx, provider) in llm.providers.iter_mut().enumerate() {
        if provider.id.trim().is_empty() {
            provider.id = format!("provider-{}", idx + 1);
        }
        if provider.name.trim().is_empty() {
            provider.name = provider.id.clone();
        }
        normalize_provider_identity(provider);
        if provider.api_base.trim().is_empty() {
            provider.api_base = default_api_base_for_provider(&provider.kind, &provider.provider);
        }
        provider.models.retain(|model| !model.trim().is_empty());
        provider.models.sort();
        provider.models.dedup();
    }
}

fn ensure_routing_defaults(llm: &mut LlmConfig) {
    let Some(primary_provider) = llm
        .providers
        .iter()
        .find(|provider| provider.enabled)
        .or_else(|| llm.providers.first())
        .map(|provider| provider.id.clone())
    else {
        return;
    };

    if llm.routing.primary.provider_id.trim().is_empty() {
        llm.routing.primary.provider_id = primary_provider.clone();
    }
    if llm.routing.primary.model.trim().is_empty() {
        llm.routing.primary.model = if llm.model.trim().is_empty() {
            default_model()
        } else {
            llm.model.clone()
        };
    }
    if llm.routing.mini.provider_id.trim().is_empty() {
        llm.routing.mini.provider_id = llm.routing.primary.provider_id.clone();
    }
    if llm.routing.mini.model.trim().is_empty() {
        llm.routing.mini.model = if llm.mini_model.trim().is_empty() {
            llm.routing.primary.model.clone()
        } else {
            llm.mini_model.clone()
        };
    }
    if llm.routing.embedding.provider_id.trim().is_empty() {
        llm.routing.embedding.provider_id = llm.routing.primary.provider_id.clone();
    }
    if llm.routing.embedding.model.trim().is_empty() {
        llm.routing.embedding.model = if llm.embed_model.trim().is_empty() {
            default_embed_model()
        } else {
            llm.embed_model.clone()
        };
    }

    for route in [
        &mut llm.routing.primary,
        &mut llm.routing.mini,
        &mut llm.routing.embedding,
    ] {
        if let Some(provider) = llm
            .providers
            .iter_mut()
            .find(|item| item.id == route.provider_id)
        {
            let model = route.model.trim();
            if !model.is_empty() && !provider.models.iter().any(|item| item == model) {
                provider.models.push(model.to_string());
                provider.models.sort();
                provider.models.dedup();
            }
        }
    }
}

fn sync_legacy_fields_from_routing(llm: &mut LlmConfig) {
    let primary_provider = llm
        .providers
        .iter()
        .find(|provider| provider.id == llm.routing.primary.provider_id)
        .or_else(|| llm.providers.first())
        .cloned();

    if let Some(provider) = primary_provider {
        llm.provider = provider.provider.clone();
        llm.api_base = provider.api_base;
        llm.api_key = provider.api_key;
    }
    llm.model = llm.routing.primary.model.clone();
    llm.mini_model = llm.routing.mini.model.clone();
    llm.embed_model = llm.routing.embedding.model.clone();
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
