use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Leashed,
    FreeRun,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Leashed => "leashed",
            Self::FreeRun => "free_run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Ask,
    Deny,
}

impl PolicyDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRisk {
    Low,
    Medium,
    High,
}

impl PolicyRisk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Alpha,
    Pup,
    Skill,
    ScheduledJob,
    Bridge,
    Mcp,
    System,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Pup => "pup",
            Self::Skill => "skill",
            Self::ScheduledJob => "scheduled_job",
            Self::Bridge => "bridge",
            Self::Mcp => "mcp",
            Self::System => "system",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "alpha" => Self::Alpha,
            "skill" => Self::Skill,
            "scheduled_job" => Self::ScheduledJob,
            "bridge" => Self::Bridge,
            "mcp" => Self::Mcp,
            "system" => Self::System,
            _ => Self::Pup,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationSource {
    Ui,
    Cli,
    Bridge,
    Scheduler,
    System,
}

impl InvocationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Cli => "cli",
            Self::Bridge => "bridge",
            Self::Scheduler => "scheduler",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyActor {
    pub kind: ActorKind,
    pub name: String,
    pub source: InvocationSource,
}

impl PolicyActor {
    pub fn alpha(source: InvocationSource) -> Self {
        Self {
            kind: ActorKind::Alpha,
            name: "alpha".to_string(),
            source,
        }
    }

    pub fn from_agent_name(agent_name: &str, source: InvocationSource) -> Self {
        if let Some(skill) = agent_name.strip_prefix("skill:") {
            Self {
                kind: ActorKind::Skill,
                name: skill.to_string(),
                source,
            }
        } else if agent_name == "alpha" {
            Self::alpha(source)
        } else {
            Self {
                kind: ActorKind::Pup,
                name: agent_name.to_string(),
                source,
            }
        }
    }

    pub fn label(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    ReadLocal,
    ReadMemory,
    WriteMemory,
    WriteWorkspace,
    WriteOutsideWorkspace,
    NetworkRead,
    ExternalSend,
    Shell,
    McpCall,
    SkillActivation,
    SystemConfig,
}

impl EffectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadLocal => "read_local",
            Self::ReadMemory => "read_memory",
            Self::WriteMemory => "write_memory",
            Self::WriteWorkspace => "write_workspace",
            Self::WriteOutsideWorkspace => "write_outside_workspace",
            Self::NetworkRead => "network_read",
            Self::ExternalSend => "external_send",
            Self::Shell => "shell",
            Self::McpCall => "mcp_call",
            Self::SkillActivation => "skill_activation",
            Self::SystemConfig => "system_config",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "read_local" => Self::ReadLocal,
            "read_memory" => Self::ReadMemory,
            "write_memory" => Self::WriteMemory,
            "write_workspace" => Self::WriteWorkspace,
            "write_outside_workspace" => Self::WriteOutsideWorkspace,
            "network_read" => Self::NetworkRead,
            "external_send" => Self::ExternalSend,
            "shell" => Self::Shell,
            "mcp_call" => Self::McpCall,
            "skill_activation" => Self::SkillActivation,
            "system_config" => Self::SystemConfig,
            _ => Self::McpCall,
        }
    }

    pub fn is_read_only(self) -> bool {
        matches!(self, Self::ReadLocal | Self::ReadMemory | Self::NetworkRead)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_prefix: Option<String>,
}

impl PolicyScope {
    pub fn covers(&self, request: &PolicyScope) -> bool {
        if let Some(prefix) = &self.path_prefix {
            let Some(path) = request.path.as_ref() else {
                return false;
            };
            if !path.starts_with(prefix) {
                return false;
            }
        }
        if let Some(path) = &self.path {
            if request.path.as_deref() != Some(path.as_str()) {
                return false;
            }
        }
        if let Some(platform) = &self.platform {
            if request.platform.as_deref() != Some(platform.as_str()) {
                return false;
            }
        }
        if let Some(server) = &self.mcp_server {
            if request.mcp_server.as_deref() != Some(server.as_str()) {
                return false;
            }
        }
        if let Some(tool) = &self.mcp_tool {
            if request.mcp_tool.as_deref() != Some(tool.as_str()) {
                return false;
            }
        }
        if let Some(prefix) = &self.command_prefix {
            let Some(command) = request.command_prefix.as_ref() else {
                return false;
            };
            if !command.starts_with(prefix) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyDetails {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_destinations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRequest {
    pub actor: PolicyActor,
    pub tool_name: String,
    pub effect: EffectKind,
    pub risk: PolicyRisk,
    pub scope: PolicyScope,
    pub description: String,
    #[serde(default)]
    pub details: PolicyDetails,
}

impl PolicyRequest {
    pub fn grant_scope(&self) -> PolicyScope {
        match self.effect {
            EffectKind::WriteWorkspace => PolicyScope {
                path_prefix: self.scope.path_prefix.clone(),
                ..PolicyScope::default()
            },
            EffectKind::McpCall => PolicyScope {
                mcp_server: self.scope.mcp_server.clone(),
                mcp_tool: self.scope.mcp_tool.clone(),
                ..PolicyScope::default()
            },
            EffectKind::ExternalSend => PolicyScope {
                platform: self.scope.platform.clone(),
                ..PolicyScope::default()
            },
            EffectKind::Shell => PolicyScope {
                command_prefix: self.scope.command_prefix.clone(),
                ..PolicyScope::default()
            },
            _ => PolicyScope::default(),
        }
    }
}

// Default execution policy profiles live here. If we want to tune leashed vs
// free_run behavior globally, this is the one function to edit.
pub fn baseline_policy_decision(mode: ExecutionMode, request: &PolicyRequest) -> PolicyDecision {
    use EffectKind::{
        ExternalSend, McpCall, NetworkRead, ReadLocal, ReadMemory, Shell, SkillActivation,
        SystemConfig, WriteMemory, WriteOutsideWorkspace, WriteWorkspace,
    };
    use ExecutionMode::{FreeRun, Leashed};
    use PolicyDecision::{Allow, Ask, Deny};
    use PolicyRisk::{High, Low, Medium};

    match mode {
        Leashed => match (request.effect, request.risk) {
            (ReadLocal | ReadMemory | NetworkRead, _) => Allow,
            (WriteMemory | WriteWorkspace, _) => Allow,
            (McpCall, Low) => Allow,
            (Shell, Low) => Allow,
            (SkillActivation, _) => Allow,
            (Shell, High) => Deny,
            (WriteOutsideWorkspace, _)
            | (SystemConfig, _)
            | (Shell, Medium)
            | (McpCall, Medium | High)
            | (ExternalSend, _) => Ask,
        },
        FreeRun => match (request.effect, request.risk) {
            (ReadLocal | ReadMemory | NetworkRead, _) => Allow,
            (WriteMemory | WriteWorkspace, _) => Allow,
            (SkillActivation, _) => Allow,
            (McpCall, Low | Medium | High) => Allow,
            (Shell, Low | Medium) => Allow,
            (ExternalSend, _) => Allow,
            (Shell, High) => Deny,
            (WriteOutsideWorkspace, _) | (SystemConfig, _) => Ask,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyGrant {
    pub id: String,
    pub actor_kind: ActorKind,
    pub actor_name: String,
    pub effect_kind: EffectKind,
    pub tool_name: Option<String>,
    pub scope: PolicyScope,
    pub risk_ceiling: PolicyRisk,
    pub mode: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub created_by: InvocationSource,
}

impl PolicyGrant {
    pub fn matches_request(&self, request: &PolicyRequest, mode: ExecutionMode, now: i64) -> bool {
        if self.actor_kind != request.actor.kind || self.actor_name != request.actor.name {
            return false;
        }
        if self.effect_kind != request.effect {
            return false;
        }
        if let Some(tool_name) = &self.tool_name {
            if tool_name != &request.tool_name {
                return false;
            }
        }
        if !(self.mode == "both" || self.mode == mode.as_str()) {
            return false;
        }
        if self.expires_at.is_some_and(|expires| expires <= now) {
            return false;
        }
        if request.risk > self.risk_ceiling {
            return false;
        }
        self.scope.covers(&request.scope)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAuditRecord {
    pub id: String,
    pub actor_kind: ActorKind,
    pub actor_name: String,
    pub source: InvocationSource,
    pub tool_name: String,
    pub effect_kind: EffectKind,
    pub risk: PolicyRisk,
    pub decision: PolicyDecision,
    pub mode: ExecutionMode,
    pub args_summary: String,
    pub result_status: String,
    pub created_at: i64,
}

pub fn summarize_json(value: &Value, max_chars: usize) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string());
    if text.chars().count() <= max_chars {
        text
    } else {
        let mut truncated: String = text.chars().take(max_chars).collect();
        truncated.push('…');
        truncated
    }
}
