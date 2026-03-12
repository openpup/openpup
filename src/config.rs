use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{Read, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenpupConfig {
    pub autonomy: AutonomyConfig,
    /// 可选：定时 Loop 的调度表；缺省时 scheduler 使用内置默认（7:00 work_morning, 9:30 invest_morning, 15:00 invest_close）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleConfig>,
    /// 可选：外部系统集成（只存非敏感配置；敏感 token 通过 env 引用）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrations: Option<IntegrationsConfig>,
    /// 可选：LLM / Agent 相关配置（只存非敏感字段，如 base_url/model/temperature）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmConfigDisk>,
    /// 可选：向 LLM 暴露的工具列表（人类/配置层面的 schema）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolExposeConfig>>,
    /// 可选：消息通道配置（如 Telegram 等）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<ChannelsConfig>,
    /// 可选：本地网关（HTTP/WS）配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_assistant: Option<HomeAssistantConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<MarketConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub news_rss: Option<NewsRssConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imap: Option<ImapConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caldav: Option<CaldavConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    /// Telegram 机器人通道配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramChannelConfig>,
    /// 飞书（Feishu / Lark）机器人通道配置。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feishu: Option<FeishuChannelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramChannelConfig {
    /// Bot token 所在环境变量名，例如 TELEGRAM_BOT_TOKEN。
    pub bot_token_env: String,
    /// 允许触发 agent 的 chat id 白名单（字符串形式，避免溢出）。
    pub allowed_chat_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuChannelConfig {
    /// 自建应用 app_id 所在的环境变量名，例如 FEISHU_APP_ID。
    pub app_id_env: String,
    /// 自建应用 app_secret 所在的环境变量名，例如 FEISHU_APP_SECRET。
    pub app_secret_env: String,
    /// 默认发送的群聊 chat_id（字符串形式）。
    /// 若留空，则调用方必须显式指定 chat_id。
    pub default_chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeAssistantConfig {
    /// 例如：http://homeassistant.local:8123
    pub base_url: String,
    /// token 所在环境变量名，例如 HOME_ASSISTANT_TOKEN（不在磁盘保存明文 token）
    pub token_env: String,
    /// 允许读取的实体白名单；为空表示不限制（不推荐，建议至少填几个实体）
    pub allowed_entities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    /// 目前固定：stooq（无需 key）。示例：AAPL.US、TSLA.US、0700.HK
    pub provider: String,
    /// 可选：默认关注列表（不影响 tool market:quote 直接指定 symbol）
    pub watchlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsRssConfig {
    /// RSS feed URL 列表
    pub feeds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    /// username 所在环境变量名（不在磁盘保存）
    pub username_env: String,
    /// password/token 所在环境变量名（不在磁盘保存）
    pub password_env: String,
    /// 允许读取的 mailbox 白名单；为空表示仅 INBOX
    pub allowed_mailboxes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaldavConfig {
    /// CalDAV base URL（含 https://）
    pub base_url: String,
    pub username_env: String,
    pub password_env: String,
    /// 可选：日历集合 URL 白名单（为空则使用 base_url）
    pub calendar_urls: Vec<String>,
}

/// 单个工具在配置文件中的声明：决定哪些底层 ToolKind 会暴露给 LLM/agent。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExposeConfig {
    /// 内部工具 ID，例如 "market_quote" / "email_unread_subjects"。
    pub id: String,
    /// 暴露给 LLM 的工具名称（tool 字段值），例如 "email_unread_subjects"。
    pub name: String,
    /// 人类可读的说明，写进 system prompt。
    #[serde(default)]
    pub description: String,
    /// 安全等级标签，例如 "L1" / "L2"（目前仅用于提示）。
    #[serde(default)]
    pub level: String,
    /// 参数提示字符串，例如 "{\"symbol\": string}"。
    #[serde(default)]
    pub args: String,
}

/// LLM 配置在磁盘上的持久化形式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfigDisk {
    /// OpenAI base_url，例如 https://api.openai.com/v1
    pub base_url: String,
    /// OpenAI 侧暴露的模型名/别名
    pub model: String,
    /// 采样温度
    pub temperature: f32,
    /// 可选：直接在配置文件中保存的 API key（你若更信任 env，可留空）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub jobs: Vec<ScheduleJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleJob {
    pub hour: u8,
    pub minute: u8,
    pub loop_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    pub spawn: SpawnConfig,
    /// 全局执行模式：控制是否允许执行 L2/L3/L4 工具。
    /// 四个取值："readonly" | "draft-only" | "full" | "approval"
    pub execution_mode: String,
    /// 是否用内建 LLM 角色（security_reviewer）对 shell_exec 做等级审查；未配置或 false 时仅用规则。
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_llm_security_review: Option<bool>,
}

/// 全局执行模式：与 config.toml [autonomy] execution_mode 对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// 仅允许 L1（只读）工具。
    Readonly,
    /// 允许 L1、L2（草稿/低风险写），禁止 L3/L4。
    DraftOnly,
    /// 允许 L1–L3 执行；L4 不暴露。
    Full,
    /// 允许 L1–L4；L4 暴露且可执行（走显式审批）。
    Approval,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionMode::Readonly => "readonly",
            ExecutionMode::DraftOnly => "draft-only",
            ExecutionMode::Full => "full",
            ExecutionMode::Approval => "approval",
        }
    }
    /// 从 config 中使用的字符串解析；无效时默认 Readonly。
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "draft-only" => ExecutionMode::DraftOnly,
            "full" => ExecutionMode::Full,
            "approval" => ExecutionMode::Approval,
            _ => ExecutionMode::Readonly,
        }
    }
}

/// 工具风险等级：与 tools 模块中的 level 对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRiskLevel {
    L1,
    L2,
    L3,
    L4,
}

impl ToolRiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolRiskLevel::L1 => "L1",
            ToolRiskLevel::L2 => "L2",
            ToolRiskLevel::L3 => "L3",
            ToolRiskLevel::L4 => "L4",
        }
    }
}

/// Allowed values for autonomy.spawn.mode (tools-autonomy-safety.md §5.3).
pub const SPAWN_MODES: [&str; 4] = ["disabled", "proposal-only", "supervised", "autonomous"];

fn is_valid_spawn_mode(mode: &str) -> bool {
    SPAWN_MODES.contains(&mode)
}

/// Allowed values for autonomy.execution_mode（四个 execution_mode）。
const EXECUTION_MODES: [&str; 4] = ["readonly", "draft-only", "full", "approval"];

fn is_valid_execution_mode(mode: &str) -> bool {
    EXECUTION_MODES.contains(&mode)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnConfig {
    /// "disabled" | "proposal-only" | "supervised" | "autonomous"
    pub mode: String,
    pub allowed_hosts: Vec<String>,
    pub max_workers: u32,
    pub require_manual_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// 监听地址，例如 "127.0.0.1:8787"。
    #[serde(default)]
    pub bind: String,
    /// token 所在环境变量名（不在磁盘保存明文 token）。例如 "OPENPUP_GATEWAY_TOKEN"。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
    /// 是否要求 WS 首帧鉴权。若为 true 且 token 不存在，则仍视为未启用鉴权。
    #[serde(default)]
    pub require_auth: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        GatewayConfig {
            bind: "127.0.0.1:8787".to_string(),
            token_env: Some("OPENPUP_GATEWAY_TOKEN".to_string()),
            require_auth: false,
        }
    }
}

impl Default for OpenpupConfig {
    fn default() -> Self {
        OpenpupConfig {
            autonomy: AutonomyConfig {
                spawn: SpawnConfig {
                    mode: "disabled".to_string(),
                    allowed_hosts: Vec::new(),
                    max_workers: 0,
                    require_manual_approval: true,
                },
                execution_mode: "readonly".to_string(),
                use_llm_security_review: None,
            },
            schedule: None,
            integrations: None,
            llm: None,
            tools: None,
            channels: None,
            gateway: None,
        }
    }
}

/// 默认调度（UTC）：7:00 work_morning，9:30 invest_morning，15:00 invest_close，16:00 life_evening，14:00 life_morning。
pub fn default_schedule_jobs() -> Vec<ScheduleJob> {
    vec![
        ScheduleJob {
            hour: 7,
            minute: 0,
            loop_id: "work_morning".to_string(),
        },
        ScheduleJob {
            hour: 9,
            minute: 30,
            loop_id: "invest_morning".to_string(),
        },
        ScheduleJob {
            hour: 15,
            minute: 0,
            loop_id: "invest_close".to_string(),
        },
        ScheduleJob {
            hour: 14,
            minute: 0,
            loop_id: "life_morning".to_string(),
        },
        ScheduleJob {
            hour: 16,
            minute: 0,
            loop_id: "life_evening".to_string(),
        },
    ]
}

/// Primary config path: `~/.openpup/config.toml`.
pub fn config_path() -> Result<std::path::PathBuf> {
    let home = home_dir().context("failed to locate home directory")?;
    Ok(home.join(".openpup").join("config.toml"))
}

/// Legacy config path kept for backward compatibility: `~/.openpup/config.toml`.
fn legacy_config_path() -> Result<std::path::PathBuf> {
    let home = home_dir().context("failed to locate home directory")?;
    Ok(home.join(".openpup").join("config.toml"))
}

pub fn load_or_init() -> Result<OpenpupConfig> {
    let primary = config_path()?;
    let path_to_read = if !primary.exists() {
        let legacy = legacy_config_path()?;
        if legacy.exists() {
            legacy
        } else {
            save(&OpenpupConfig::default())?;
            return Ok(OpenpupConfig::default());
        }
    } else {
        primary.clone()
    };

    let mut buf = String::new();
    File::open(&path_to_read)
        .with_context(|| format!("failed to open openpup config at {:?}", path_to_read))?
        .read_to_string(&mut buf)
        .with_context(|| format!("failed to read openpup config at {:?}", path_to_read))?;

    let mut cfg: OpenpupConfig =
        toml::from_str(&buf).with_context(|| "failed to parse openpup config as TOML")?;

    // Safe default: invalid spawn.mode => fallback to "disabled"
    if !is_valid_spawn_mode(cfg.autonomy.spawn.mode.as_str()) {
        let invalid = std::mem::take(&mut cfg.autonomy.spawn.mode);
        cfg.autonomy.spawn.mode = "disabled".to_string();
        save(&cfg).context("failed to save config after correcting invalid spawn.mode")?;
        eprintln!(
            "openpup: spawn.mode {:?} is invalid; set to \"disabled\". Allowed: {:?}.",
            invalid, SPAWN_MODES
        );
    }

    // Safe default: invalid execution_mode => fallback to "readonly"
    if !is_valid_execution_mode(cfg.autonomy.execution_mode.as_str()) {
        let invalid = std::mem::take(&mut cfg.autonomy.execution_mode);
        cfg.autonomy.execution_mode = "readonly".to_string();
        save(&cfg).context("failed to save config after correcting invalid execution_mode")?;
        eprintln!(
            "openpup: execution_mode {:?} is invalid; set to \"readonly\". Allowed: {:?}.",
            invalid, EXECUTION_MODES
        );
    }
    Ok(cfg)
}

pub fn save(cfg: &OpenpupConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(dir) = path.parent() {
        create_dir_all(dir)
            .with_context(|| format!("failed to create openpup config dir {:?}", dir))?;
    }

    let toml_str =
        toml::to_string_pretty(cfg).with_context(|| "failed to serialize openpup config")?;
    let mut f = File::create(&path)
        .with_context(|| format!("failed to create openpup config at {:?}", path))?;
    f.write_all(toml_str.as_bytes())
        .with_context(|| format!("failed to write openpup config at {:?}", path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_spawn_mode_falls_back_to_disabled() {
        assert!(is_valid_spawn_mode("disabled"));
        assert!(!is_valid_spawn_mode("nope"));
    }

    #[test]
    fn execution_mode_validation() {
        assert!(is_valid_execution_mode("readonly"));
        assert!(is_valid_execution_mode("draft-only"));
        assert!(is_valid_execution_mode("full"));
        assert!(is_valid_execution_mode("approval"));
        assert!(!is_valid_execution_mode("something-else"));
    }
}
