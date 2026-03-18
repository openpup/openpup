use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// The current user message.
    pub intent: String,
    /// Recent conversation history (role/content pairs, no system messages).
    pub context: Vec<Message>,
    /// OWNER.md content — passed through so specialists can personalise their response.
    pub owner_context: String,
    /// Top relevant long-term memories injected by Alpha before routing.
    pub relevant_memories: Vec<String>,
    /// Optional system-prompt override from PupConfig (None = use built-in default).
    pub system_prompt_override: Option<String>,
    pub assigned_pup: Option<String>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub output: String,
}

/// Tool access permissions for a pup's conversation loop.
///
/// Alpha calls `run_agent_with_tools` with these flags to determine which
/// primitive tools and MCP tools are available during the pup's turn.
#[derive(Debug, Clone)]
pub struct PupToolPermissions {
    /// Allow `shell_exec` primitive.
    pub shell: bool,
    /// Allow `file_read` / `file_write` primitives.
    pub filesystem: bool,
    /// Allow `http_get` primitive.
    pub network: bool,
    /// Inject all cached MCP server tools.
    pub mcp: bool,
}

impl Default for PupToolPermissions {
    fn default() -> Self {
        Self {
            shell: false,
            filesystem: false,
            network: false,
            mcp: true,
        }
    }
}

/// A specialist pup — provides a system prompt and tool permissions.
/// The actual LLM call + tool-call loop is owned by Alpha so that all
/// pups automatically benefit from MCP tool injection and future improvements.
pub trait SpecialistPup: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Vec<String>;

    /// Build the system prompt for this pup, incorporating task context,
    /// owner profile, and relevant memories.
    fn build_system_prompt(&self, task: &Task) -> String;

    /// Which tools this pup is allowed to call.  Default: MCP only.
    fn tool_permissions(&self) -> PupToolPermissions {
        PupToolPermissions::default()
    }
}
