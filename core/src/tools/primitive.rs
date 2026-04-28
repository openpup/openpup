/// Primitive tool registry — the only LLM-callable operations.
///
/// Tool implementations are split across sibling modules:
/// - `risk.rs`    — command risk assessment
/// - `shell.rs`   — shell_exec, sandbox_shell_exec
/// - `file.rs`    — file_read, file_write, skill resource access
/// - `network.rs` — http_get, web_fetch
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use openpup_capabilities::Capabilities;
use serde_json::Value;
use tracing::debug;

use super::risk::{assess_command_risk, format_risk_warning};
use crate::bridge::types::BridgeOutbox;
use crate::memory::system::MemorySystem;
use crate::policy::{
    summarize_json, EffectKind, PolicyActor, PolicyDetails, PolicyRequest, PolicyRisk, PolicyScope,
};
use crate::skills::permissions::{ExecutionMode, PermissionChecker};
use crate::skills::registry::SkillRegistry;

// Re-export risk types for external consumers
pub use super::risk::{
    assess_command_risk as assess_risk, format_risk_warning as format_risk, CommandRiskContext,
    CommandRiskLevel, ShellKind,
};

// ── Permission surface ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ToolPermissions {
    pub shell: bool,
    pub sandbox_shell: bool,
    pub file_read: bool,
    pub file_write: bool,
    pub network: bool,
}

impl ToolPermissions {
    /// Merge with skill permissions: OR each flag so the skill can elevate
    /// the pup's baseline but never restrict it.
    pub fn union_with_skill(&self, skill: &crate::skills::registry::SkillPermissions) -> Self {
        Self {
            shell: self.shell || skill.shell,
            sandbox_shell: self.sandbox_shell || skill.sandbox_shell,
            file_read: self.file_read || skill.file_read,
            file_write: self.file_write || skill.file_write,
            network: self.network || skill.network,
        }
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    pub workspace_root: PathBuf,
    pub memory: Arc<MemorySystem>,
    pub skill_registry: SkillRegistry,
    pub capabilities: Arc<Capabilities>,
    /// Context window limit in tokens — tool results are truncated proportionally.
    pub(crate) context_limit: std::sync::atomic::AtomicU64,
    /// Shared bridge outbound sender (filled when bridge starts).
    pub bridge_outbox: BridgeOutbox,
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn policy_risk_from_command(risk: CommandRiskLevel) -> PolicyRisk {
    match risk {
        CommandRiskLevel::Low => PolicyRisk::Low,
        CommandRiskLevel::Medium => PolicyRisk::Medium,
        CommandRiskLevel::High => PolicyRisk::High,
    }
}

impl ToolRegistry {
    pub fn new(
        workspace_root: PathBuf,
        memory: Arc<MemorySystem>,
        skill_registry: SkillRegistry,
        capabilities: Arc<Capabilities>,
        bridge_outbox: BridgeOutbox,
    ) -> Self {
        Self {
            workspace_root,
            memory,
            skill_registry,
            capabilities,
            context_limit: std::sync::atomic::AtomicU64::new(128_000),
            bridge_outbox,
        }
    }

    /// Update the context limit (called once after model is known).
    pub fn set_context_limit(&self, limit: u64) {
        self.context_limit
            .store(limit, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn command_risk_context(&self, kind: ShellKind) -> CommandRiskContext {
        let mut allowed_roots = vec![self.workspace_root.clone(), std::env::temp_dir()];
        if let Ok(app_root) = crate::config::app_root() {
            allowed_roots.push(app_root);
        }
        CommandRiskContext {
            kind,
            allowed_roots,
        }
    }

    fn maybe_allow_dynamic_root_for_path(&self, path: &std::path::Path) -> Result<()> {
        if let Err(err) = self.capabilities.fs.allow_root(path) {
            if !err.to_string().contains("filesystem.allow_root") {
                return Err(err);
            }
        }
        Ok(())
    }

    fn path_is_already_allowed(&self, path: &std::path::Path) -> Result<bool> {
        match self.capabilities.fs.is_path_allowed(path) {
            Ok(allowed) => Ok(allowed),
            Err(err) if err.to_string().contains("filesystem.is_path_allowed") => Ok(false),
            Err(err) => Err(err),
        }
    }

    async fn maybe_allow_dynamic_root_for_tool(
        &self,
        name: &str,
        args: &Value,
        permissions: &PermissionChecker,
        actor: &PolicyActor,
    ) -> Result<()> {
        let mode = permissions.get_mode().await;
        match name {
            "file_read" => {
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_read: missing 'path'"))?;
                let resolved = self.resolve_path(path);
                if self.path_is_already_allowed(&resolved)? {
                    return Ok(());
                }
                match mode {
                    ExecutionMode::FreeRun => self.maybe_allow_dynamic_root_for_path(&resolved)?,
                    ExecutionMode::Leashed => {
                        let approved = permissions
                            .authorize_boundary_access(
                                actor.clone(),
                                name,
                                &resolved.display().to_string(),
                            )
                            .await?;
                        if !approved {
                            return Err(anyhow!(
                                "file_read: boundary access denied for '{}'",
                                resolved.display()
                            ));
                        }
                        self.maybe_allow_dynamic_root_for_path(&resolved)?;
                    }
                }
            }
            "file_write" => {
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_write: missing 'path'"))?;
                let resolved = self.resolve_path(path);
                let root = resolved.parent().unwrap_or(resolved.as_path());
                if !self.path_is_already_allowed(root)? {
                    self.maybe_allow_dynamic_root_for_path(root)?;
                }
            }
            "skill_read_resource" => {
                let skill_name = args["skill_name"]
                    .as_str()
                    .ok_or_else(|| anyhow!("skill_read_resource: missing 'skill_name'"))?;
                let relpath = args["relpath"]
                    .as_str()
                    .ok_or_else(|| anyhow!("skill_read_resource: missing 'relpath'"))?;
                let resolved = self
                    .skill_registry
                    .resolve_skill_resource_path(skill_name, relpath)
                    .await?;
                if self.path_is_already_allowed(&resolved)? {
                    return Ok(());
                }
                match mode {
                    ExecutionMode::FreeRun => self.maybe_allow_dynamic_root_for_path(&resolved)?,
                    ExecutionMode::Leashed => {
                        let approved = permissions
                            .authorize_boundary_access(
                                actor.clone(),
                                name,
                                &resolved.display().to_string(),
                            )
                            .await?;
                        if !approved {
                            return Err(anyhow!(
                                "skill_read_resource: boundary access denied for '{}'",
                                resolved.display()
                            ));
                        }
                        self.maybe_allow_dynamic_root_for_path(&resolved)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn policy_request_for_tool(
        &self,
        actor: PolicyActor,
        name: &str,
        args: &Value,
    ) -> PolicyRequest {
        let mut details = PolicyDetails::default();
        let mut scope = PolicyScope::default();
        let (effect, risk, description) = match name {
            "shell_exec" => {
                let cmd = args["command"].as_str().unwrap_or_default();
                let risk = assess_command_risk(cmd, &self.command_risk_context(ShellKind::Real));
                scope.command_prefix = Some(cmd.to_string());
                (
                    EffectKind::Shell,
                    policy_risk_from_command(risk),
                    format!("Execute shell command: {}", truncate_chars(cmd, 240)),
                )
            }
            "sandbox_shell_exec" => {
                let cmd = args["command"].as_str().unwrap_or_default();
                let risk = assess_command_risk(cmd, &self.command_risk_context(ShellKind::Sandbox));
                scope.command_prefix = Some(cmd.to_string());
                (
                    EffectKind::Shell,
                    policy_risk_from_command(risk),
                    format!(
                        "Execute sandbox shell command: {}",
                        truncate_chars(cmd, 240)
                    ),
                )
            }
            "file_read" | "skill_list_resources" | "skill_read_resource" => (
                EffectKind::ReadLocal,
                PolicyRisk::Low,
                format!("Read local data with `{name}`"),
            ),
            "file_write" => {
                let path = args["path"].as_str().unwrap_or_default();
                let resolved = self.resolve_path(path);
                let resolved_s = resolved.display().to_string();
                details.affected_files.push(resolved_s.clone());
                scope.path = Some(resolved_s.clone());
                if resolved.starts_with(&self.workspace_root) {
                    scope.path_prefix = Some(self.workspace_root.display().to_string());
                    (
                        EffectKind::WriteWorkspace,
                        PolicyRisk::Medium,
                        format!("Write workspace file: {resolved_s}"),
                    )
                } else {
                    (
                        EffectKind::WriteOutsideWorkspace,
                        PolicyRisk::High,
                        format!("Write file outside workspace: {resolved_s}"),
                    )
                }
            }
            "http_get" | "web_fetch" => {
                let url = args["url"].as_str().unwrap_or_default();
                scope.url = Some(url.to_string());
                details.network_destinations.push(url.to_string());
                (
                    EffectKind::NetworkRead,
                    PolicyRisk::Low,
                    format!("Fetch network resource: {url}"),
                )
            }
            "memory_search" | "search_knowledge_base" | "search_knowledge_graph" => (
                EffectKind::ReadMemory,
                PolicyRisk::Low,
                format!("Read local memory or knowledge with `{name}`"),
            ),
            "memory_store" => (
                EffectKind::WriteMemory,
                PolicyRisk::Low,
                "Store a long-term memory".to_string(),
            ),
            "bridge_send" => {
                if let Some(platform) = args["platform"].as_str() {
                    scope.platform = Some(platform.to_string());
                }
                (
                    EffectKind::ExternalSend,
                    PolicyRisk::Medium,
                    format!("Send bridge message: {}", summarize_json(args, 240)),
                )
            }
            _ => (
                EffectKind::McpCall,
                PolicyRisk::Medium,
                format!("Call tool `{name}` with {}", summarize_json(args, 240)),
            ),
        };

        PolicyRequest {
            actor,
            tool_name: name.to_string(),
            effect,
            risk,
            scope,
            description,
            details,
        }
    }

    /// Dynamic max chars for tool results: 30% of context window × 4 chars/token, clamped.
    pub(crate) fn tool_result_max_chars(&self) -> usize {
        let limit = self
            .context_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        let max = ((limit as f64 * 0.30) * 4.0) as usize;
        max.clamp(2_000, 32_768)
    }

    /// Truncate tool results using head+tail strategy: keep the first ~70% and
    /// last ~30% of the budget so that error messages at the end aren't lost.
    pub(crate) fn dynamic_truncate(&self, text: &str) -> String {
        let max = self.tool_result_max_chars();
        let count = text.chars().count();
        if count <= max {
            return text.to_string();
        }
        let tail_budget = max * 3 / 10; // 30% for tail
        let head_budget = max - tail_budget - 80; // room for the marker line
        let head: String = text.chars().take(head_budget).collect();
        let tail: String = text.chars().skip(count - tail_budget).collect();
        format!(
            "{head}\n\n… [truncated {omitted} chars of {count} total] …\n\n{tail}",
            omitted = count - head_budget - tail_budget,
        )
    }

    // ── Schema generation ──────────────────────────────────────────────────────

    /// Return the OpenAI-compatible tool schemas for the given permission set.
    /// Tools not permitted by `perms` are excluded, so the LLM can never call
    /// something the skill manifest did not declare.
    pub fn available_tools(&self, perms: &ToolPermissions) -> Vec<Value> {
        let mut tools: Vec<Value> = Vec::new();

        if perms.shell {
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "shell_exec",
          "description": "Execute a shell command in the real workspace and return combined stdout + stderr. The command is automatically wrapped in the platform shell (sh -c on Unix, cmd /d /s /c on Windows). Do NOT wrap the command yourself in sh, bash, cmd, or powershell — just provide the raw command.",
          "parameters": {
            "type": "object",
            "properties": {
              "command": { "type": "string", "description": "Raw command to execute (e.g. 'python run.py', 'ls -la'). Do NOT prefix with sh/bash/cmd/powershell." }
            },
            "required": ["command"]
          }
        }
      }));
        }

        if perms.sandbox_shell {
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "sandbox_shell_exec",
          "description": "Execute a shell command with the same workspace-relative current directory as shell_exec, but with a reduced environment and temporary HOME/TMP locations isolated from the real workspace. Use this to test commands before using shell_exec in the real workspace.",
          "parameters": {
            "type": "object",
            "properties": {
              "command": { "type": "string", "description": "Raw command to execute inside the sandbox. Do NOT prefix with sh/bash/cmd/powershell." },
              "timeout_ms": { "type": "integer", "description": "Optional timeout in milliseconds (default: 10000, max: 30000)" }
            },
            "required": ["command"]
          }
        }
      }));
        }

        if perms.file_read {
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "file_read",
          "description": "Read a file and return its text content.",
          "parameters": {
            "type": "object",
            "properties": {
              "path": { "type": "string", "description": "Absolute path or path relative to the workspace root" }
            },
            "required": ["path"]
          }
        }
      }));
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "skill_list_resources",
          "description": "List indexed directories and files from a Claude-style skill bundle. Use this before reading when the exact relative path is not yet certain.",
          "parameters": {
            "type": "object",
            "properties": {
              "skill_name": { "type": "string", "description": "The active skill name" },
              "limit": { "type": "integer", "description": "Maximum number of files and directories to include per section (default: 20)" }
            },
            "required": ["skill_name"]
          }
        }
      }));
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "skill_read_resource",
          "description": "Read an indexed file from a Claude-style skill bundle using the skill name and the exact indexed relative path. Use this instead of guessing workspace paths for skill resources.",
          "parameters": {
            "type": "object",
            "properties": {
              "skill_name": { "type": "string", "description": "The active skill name" },
              "relpath": { "type": "string", "description": "Exact relative path from the skill resource index, such as scripts/run.py" }
            },
            "required": ["skill_name", "relpath"]
          }
        }
      }));
        }

        if perms.file_write {
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "file_write",
          "description": "Write text content to a file (creates parent directories if needed).",
          "parameters": {
            "type": "object",
            "properties": {
              "path": { "type": "string", "description": "Absolute path or path relative to the workspace root" },
              "content": { "type": "string", "description": "Text content to write" }
            },
            "required": ["path", "content"]
          }
        }
      }));
        }

        if perms.network {
            tools.push(serde_json::json!({
              "type": "function",
              "function": {
                "name": "http_get",
                "description": "Perform an HTTP GET request and return the response body.",
                "parameters": {
                  "type": "object",
                  "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                  },
                  "required": ["url"]
                }
              }
            }));
            tools.push(serde_json::json!({
        "type": "function",
        "function": {
          "name": "web_fetch",
          "description": "Fetch a web page, extract readable text, and return a structured summary including the final URL and page title.",
          "parameters": {
            "type": "object",
            "properties": {
              "url": { "type": "string", "description": "URL of the web page to fetch" }
            },
            "required": ["url"]
          }
        }
      }));
        }

        if perms.network {
            tools.push(serde_json::json!({
              "type": "function",
              "function": {
                "name": "bridge_send",
                "description": "Send a text message to the owner via a configured messaging bridge (Telegram, Discord, Weixin, etc.). If platform is omitted, sends to all configured bridges.",
                "parameters": {
                  "type": "object",
                  "properties": {
                    "text": { "type": "string", "description": "Message text to send" },
                    "platform": { "type": "string", "enum": ["telegram", "discord", "weixin", "qqbot"], "description": "Optional: 'telegram', 'discord', 'weixin', or 'qqbot'. Omit to send to all configured platforms." }
                  },
                  "required": ["text"]
                }
              }
            }));
        }

        // Memory & knowledge tools are always available — they don't need a permission flag.
        tools.push(serde_json::json!({
      "type": "function",
      "function": {
        "name": "search_knowledge_base",
        "description": "Search the owner's local knowledge base for relevant documents, notes, and reference material. The knowledge base contains documents the owner has manually imported. Use this to find specific technical documentation, project notes, or archived content. Different from memory_search: knowledge base stores imported documents, while memory stores facts extracted from conversations.",
        "parameters": {
          "type": "object",
          "properties": {
            "query": { "type": "string", "description": "Natural-language search query describing what you're looking for" },
            "limit": { "type": "integer", "description": "Maximum number of results (default: 5, max: 20)" }
          },
          "required": ["query"]
        }
      }
    }));
        tools.push(serde_json::json!({
      "type": "function",
      "function": {
        "name": "search_knowledge_graph",
        "description": "Search the knowledge graph for entity relationships. Use this for relationship-oriented queries like 'what depends on X', 'who created Y', 'what tools does project Z use'. Complements search_knowledge_base: that tool does semantic text search, this tool traverses entity relationships.",
        "parameters": {
          "type": "object",
          "properties": {
            "entity": { "type": "string", "description": "Entity name to start the graph traversal from, e.g. 'Rust', 'Alpha Pup', 'SQLite'" },
            "hops": { "type": "integer", "description": "Number of hops to traverse (1 = direct relations, 2 = second-degree). Default: 1, max: 2" }
          },
          "required": ["entity"]
        }
      }
    }));
        tools.push(serde_json::json!({
      "type": "function",
      "function": {
        "name": "memory_search",
        "description": "Search the owner's long-term memory store for relevant information.",
        "parameters": {
          "type": "object",
          "properties": {
            "query": { "type": "string", "description": "Natural-language search query" },
            "limit": { "type": "integer", "description": "Maximum number of results (default: 5)" }
          },
          "required": ["query"]
        }
      }
    }));
        tools.push(serde_json::json!({
      "type": "function",
      "function": {
        "name": "memory_store",
        "description": "Store a fact, insight, or preference into the owner's long-term memory.",
        "parameters": {
          "type": "object",
          "properties": {
            "content": { "type": "string", "description": "Text to persist as a long-term memory" }
          },
          "required": ["content"]
        }
      }
    }));

        tools
    }

    // ── Dispatch ───────────────────────────────────────────────────────────────

    pub async fn execute(
        &self,
        name: &str,
        args: &Value,
        perms: &ToolPermissions,
        permissions: &PermissionChecker,
        actor: &PolicyActor,
    ) -> Result<String> {
        match name {
            "shell_exec" => {
                if !perms.shell {
                    return Err(anyhow!(
                        "shell_exec: permission denied (skill requires permissions.shell = true)"
                    ));
                }
                let cmd = args["command"]
                    .as_str()
                    .ok_or_else(|| anyhow!("shell_exec: missing 'command'"))?;
                // Dynamic risk assessment: block high-risk commands
                let risk = assess_command_risk(cmd, &self.command_risk_context(ShellKind::Real));
                if let Some(warning) = format_risk_warning(cmd, risk) {
                    debug!(
                        "[tool/shell_exec] BLOCKED high-risk command: {}",
                        truncate_chars(cmd, 80)
                    );
                    return Ok(warning);
                }
                self.shell_exec(cmd).await
            }
            "sandbox_shell_exec" => {
                if !perms.sandbox_shell {
                    return Err(anyhow!(
                        "sandbox_shell_exec: permission denied (requires permissions.sandbox_shell = true)"
                    ));
                }
                let cmd = args["command"]
                    .as_str()
                    .ok_or_else(|| anyhow!("sandbox_shell_exec: missing 'command'"))?;
                let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(10_000);
                self.sandbox_shell_exec(cmd, timeout_ms).await
            }
            "file_read" => {
                if !perms.file_read {
                    return Err(anyhow!(
                        "file_read: permission denied (requires permissions.file_read = true)"
                    ));
                }
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_read: missing 'path'"))?;
                self.maybe_allow_dynamic_root_for_tool(name, args, permissions, actor)
                    .await?;
                self.file_read(path).await
            }
            "skill_list_resources" => {
                if !perms.file_read {
                    return Err(anyhow!(
                        "skill_list_resources: permission denied (requires permissions.file_read = true)"
                    ));
                }
                let skill_name = args["skill_name"]
                    .as_str()
                    .ok_or_else(|| anyhow!("skill_list_resources: missing 'skill_name'"))?;
                let limit = args["limit"].as_u64().unwrap_or(20) as usize;
                self.skill_list_resources(skill_name, limit).await
            }
            "skill_read_resource" => {
                if !perms.file_read {
                    return Err(anyhow!(
                        "skill_read_resource: permission denied (requires permissions.file_read = true)"
                    ));
                }
                let skill_name = args["skill_name"]
                    .as_str()
                    .ok_or_else(|| anyhow!("skill_read_resource: missing 'skill_name'"))?;
                let relpath = args["relpath"]
                    .as_str()
                    .ok_or_else(|| anyhow!("skill_read_resource: missing 'relpath'"))?;
                self.maybe_allow_dynamic_root_for_tool(name, args, permissions, actor)
                    .await?;
                self.skill_read_resource(skill_name, relpath).await
            }
            "file_write" => {
                if !perms.file_write {
                    return Err(anyhow!(
                        "file_write: permission denied (requires permissions.file_write = true)"
                    ));
                }
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_write: missing 'path'"))?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_write: missing 'content'"))?;
                self.maybe_allow_dynamic_root_for_tool(name, args, permissions, actor)
                    .await?;
                self.file_write(path, content).await
            }
            "http_get" => {
                if !perms.network {
                    return Err(anyhow!(
                        "http_get: permission denied (skill requires permissions.network = true)"
                    ));
                }
                let url = args["url"]
                    .as_str()
                    .ok_or_else(|| anyhow!("http_get: missing 'url'"))?;
                self.http_get(url).await
            }
            "web_fetch" => {
                if !perms.network {
                    return Err(anyhow!(
                        "web_fetch: permission denied (requires permissions.network = true)"
                    ));
                }
                let url = args["url"]
                    .as_str()
                    .ok_or_else(|| anyhow!("web_fetch: missing 'url'"))?;
                self.web_fetch(url).await
            }
            "search_knowledge_base" => {
                let query = args["query"]
                    .as_str()
                    .ok_or_else(|| anyhow!("search_knowledge_base: missing 'query'"))?;
                let limit = args["limit"].as_u64().unwrap_or(5).min(20) as usize;
                let retriever = crate::knowledge::retriever::KbRetriever::new(self.memory.clone());
                let results = retriever.search(query, limit, None).await?;
                if results.is_empty() {
                    Ok("No matching documents found in the knowledge base.".to_string())
                } else {
                    let formatted: Vec<String> = results
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            let source = r
                                .heading_path
                                .as_deref()
                                .map(|h| format!("{} > {}", r.source_title, h))
                                .unwrap_or_else(|| r.source_title.clone());
                            format!(
                                "[{}] Source: {} (relevance: {:.0}%)\n{}",
                                i + 1,
                                source,
                                r.score * 100.0,
                                r.content
                            )
                        })
                        .collect();
                    Ok(self.dynamic_truncate(&format!(
                        "Found {} results:\n\n{}",
                        results.len(),
                        formatted.join("\n\n---\n\n")
                    )))
                }
            }
            "search_knowledge_graph" => {
                let entity = args["entity"]
                    .as_str()
                    .ok_or_else(|| anyhow!("search_knowledge_graph: missing 'entity'"))?;
                let hops = args["hops"].as_u64().unwrap_or(1).min(2) as usize;
                let retriever =
                    crate::knowledge::graph_retriever::GraphRetriever::new(self.memory.clone());
                let results = retriever.search(entity, hops).await?;
                if results.is_empty() {
                    // Also try to show entity info even without chunks
                    let entities = self.memory.find_kg_entities(entity).await?;
                    if entities.is_empty() {
                        Ok(format!(
                            "No entity matching '{entity}' found in the knowledge graph."
                        ))
                    } else {
                        let mut out =
                            format!("Found {} entities matching '{entity}':\n", entities.len());
                        for (id, name, etype, desc) in &entities {
                            out.push_str(&format!("\n- {} [{}]", name, etype));
                            if let Some(d) = desc {
                                out.push_str(&format!(": {d}"));
                            }
                            // Show relations
                            if let Ok(rels) = self.memory.kg_entity_relations(id).await {
                                for (rel, other_name, _other_type, dir, conf) in &rels {
                                    let arrow = if dir == "out" { "→" } else { "←" };
                                    out.push_str(&format!(
                                        "\n    {arrow} {rel} {other_name} ({:.0}%)",
                                        conf * 100.0
                                    ));
                                }
                            }
                        }
                        Ok(self.dynamic_truncate(&out))
                    }
                } else {
                    // Show entities + related chunks
                    let entities = self.memory.find_kg_entities(entity).await?;
                    let mut out = String::new();
                    if !entities.is_empty() {
                        out.push_str(&format!("Entities matching '{entity}':\n"));
                        for (id, name, etype, desc) in &entities {
                            out.push_str(&format!("\n- {} [{}]", name, etype));
                            if let Some(d) = desc {
                                out.push_str(&format!(": {d}"));
                            }
                            if let Ok(rels) = self.memory.kg_entity_relations(id).await {
                                for (rel, other_name, _other_type, dir, conf) in &rels {
                                    let arrow = if dir == "out" { "→" } else { "←" };
                                    out.push_str(&format!(
                                        "\n    {arrow} {rel} {other_name} ({:.0}%)",
                                        conf * 100.0
                                    ));
                                }
                            }
                        }
                        out.push_str("\n\n");
                    }
                    out.push_str(&format!("Related chunks ({}):\n", results.len()));
                    for (i, r) in results.iter().enumerate().take(10) {
                        let source = r
                            .heading_path
                            .as_deref()
                            .map(|h| format!("{} > {}", r.source_title, h))
                            .unwrap_or_else(|| r.source_title.clone());
                        out.push_str(&format!(
                            "\n[{}] Source: {}\n{}\n",
                            i + 1,
                            source,
                            r.content
                        ));
                    }
                    Ok(self.dynamic_truncate(&out))
                }
            }
            "memory_search" => {
                let query = args["query"]
                    .as_str()
                    .ok_or_else(|| anyhow!("memory_search: missing 'query'"))?;
                let limit = args["limit"].as_u64().unwrap_or(5) as usize;
                let results = self.memory.search_long_term(query, limit).await?;
                if results.is_empty() {
                    Ok("No matching memories found.".to_string())
                } else {
                    Ok(results.join("\n"))
                }
            }
            "memory_store" => {
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow!("memory_store: missing 'content'"))?;
                self.memory
                    .add_long_term_memory(content, "skill", 0.6)
                    .await?;
                Ok("Memory stored.".to_string())
            }
            "bridge_send" => {
                if !perms.network {
                    return Err(anyhow!(
                        "bridge_send: permission denied (requires permissions.network = true)"
                    ));
                }
                let text = args["text"]
                    .as_str()
                    .ok_or_else(|| anyhow!("bridge_send: missing 'text'"))?;
                let platform_filter = args["platform"].as_str();
                self.bridge_send(text, platform_filter).await
            }
            other => Err(anyhow!("unknown tool: '{other}'")),
        }
    }

    async fn bridge_send(&self, text: &str, platform_filter: Option<&str>) -> Result<String> {
        use crate::bridge::types::{OutboundMessage, OutboundType, Platform};

        let tx = self
            .bridge_outbox
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("bridge not running — no outbound channel available"))?;

        let cfg = crate::config::load_with_env().bridge.unwrap_or_default();

        let mut targets: Vec<(Platform, String)> = Vec::new();
        if let Some(tg) = &cfg.telegram {
            targets.push((Platform::Telegram, tg.owner_user_id.clone()));
        }
        if let Some(discord) = &cfg.discord {
            if let Some(channel_id) = discord.allowed_channels.first() {
                targets.push((Platform::Discord, channel_id.clone()));
            }
        }
        if let Some(wx) = &cfg.weixin {
            targets.push((Platform::Weixin, wx.owner_user_id.clone()));
        }
        if let Some(qq) = &cfg.qqbot {
            let chat_id = if qq.owner_user_id.starts_with("c2c:") {
                qq.owner_user_id.clone()
            } else {
                format!("c2c:{}", qq.owner_user_id)
            };
            targets.push((Platform::QQBot, chat_id));
        }

        if let Some(filter) = platform_filter {
            let normalized = match filter {
                "qq" | "qqbot" | "QQ" => "qqbot",
                "wechat" | "wx" | "weixin" => "weixin",
                "tg" | "telegram" => "telegram",
                "discord" | "dc" => "discord",
                other => other,
            };
            targets.retain(|(p, _)| p.as_str() == normalized);
        }

        if targets.is_empty() {
            return Ok("No matching bridge platform configured.".to_string());
        }

        let mut sent = Vec::new();
        for (platform, chat_id) in &targets {
            let _ = tx
                .send(OutboundMessage {
                    platform: platform.clone(),
                    chat_id: chat_id.clone(),
                    text: text.to_string(),
                    reply_to_id: None,
                    msg_type: OutboundType::Result,
                })
                .await;
            sent.push(platform.as_str());
        }

        Ok(format!("Sent to: {}", sent.join(", ")))
    }

    // Tool implementations are in sibling modules: shell.rs, file.rs, network.rs
}
