use serde::{Deserialize, Serialize};

use crate::agents::truncate_utf8;

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

/// Parse `PUPS.md` and extract the prompt section for a specific pup.
///
/// Expected format in PUPS.md:
/// ```markdown
/// ## dev
/// Your custom prompt for Dev Pup...
///
/// ## writer
/// Your custom prompt for Writer Pup...
/// ```
///
/// Returns `None` if the file doesn't exist, the pup has no section, or the
/// section is empty.
fn load_pup_prompt_from_pups_md(pup_name: &str) -> Option<String> {
    let path = crate::config::app_root().ok()?.join("PUPS.md");
    let content = std::fs::read_to_string(&path).ok()?;

    // Find `## {pup_name}` header (case-insensitive match on pup name)
    let target = format!("## {}", pup_name);
    let mut in_section = false;
    let mut lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(&target) {
            in_section = true;
            continue;
        }
        if in_section {
            // Stop at next h2 header
            if trimmed.starts_with("## ") {
                break;
            }
            lines.push(line);
        }
    }

    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Shared prompt builder used by all specialist pups.
///
/// Resolution order:
///   1. `PupConfig.system_prompt_override` (from pup_configs.json — set via UI)
///   2. `PUPS.md` section `## {pup_name}` (hand-edited Markdown file)
///   3. Hardcoded `default_prompt` compiled into the binary
///
/// After resolving the base prompt, appends owner profile and relevant memories.
pub fn build_prompt_with_template(pup_name: &str, default_prompt: &str, task: &Task) -> String {
    // 1. Resolve base prompt: pup_configs.json > PUPS.md > hardcoded default
    let base = task
        .system_prompt_override
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| load_pup_prompt_from_pups_md(pup_name))
        .unwrap_or_else(|| default_prompt.to_string());

    let mut system = base;

    // 2. Owner profile
    if task.owner_context.contains("## Boundaries") {
        system.push_str(&format!("\n\nOwner profile:\n{}", task.owner_context));
    }

    // 3. Relevant memories
    if !task.relevant_memories.is_empty() {
        let bullets: String = task
            .relevant_memories
            .iter()
            .map(|m| format!("- {}", truncate_utf8(m, 200)))
            .collect::<Vec<_>>()
            .join("\n");
        system.push_str(&format!("\n\n## Relevant Memories\n{bullets}"));
    }

    system
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
    /// Allow `sandbox_shell_exec` primitive.
    pub sandbox_shell: bool,
    /// Allow `file_read` primitive.
    pub file_read: bool,
    /// Allow `file_write` primitive.
    pub file_write: bool,
    /// Allow `http_get` primitive.
    pub network: bool,
    /// Inject all cached MCP server tools.
    pub mcp: bool,
}

impl Default for PupToolPermissions {
    fn default() -> Self {
        Self {
            shell: false,
            sandbox_shell: false,
            file_read: false,
            file_write: false,
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
