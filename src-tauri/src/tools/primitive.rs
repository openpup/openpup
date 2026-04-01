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
use serde_json::Value;
use tracing::debug;

use crate::memory::system::MemorySystem;
use crate::skills::registry::SkillRegistry;
use super::risk::{assess_command_risk, format_risk_warning};

// Re-export risk types for external consumers
pub use super::risk::{CommandRiskLevel, assess_command_risk as assess_risk, format_risk_warning as format_risk};

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
    pub(crate) http: reqwest::Client,
    /// Context window limit in tokens — tool results are truncated proportionally.
    pub(crate) context_limit: std::sync::atomic::AtomicU64,
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

impl ToolRegistry {
    pub fn new(
        workspace_root: PathBuf,
        memory: Arc<MemorySystem>,
        skill_registry: SkillRegistry,
    ) -> Self {
        Self {
            workspace_root,
            memory,
            skill_registry,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            context_limit: std::sync::atomic::AtomicU64::new(128_000),
        }
    }

    /// Update the context limit (called once after model is known).
    pub fn set_context_limit(&self, limit: u64) {
        self.context_limit
            .store(limit, std::sync::atomic::Ordering::Relaxed);
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
          "description": "Execute a shell command in the real workspace and return combined stdout + stderr.",
          "parameters": {
            "type": "object",
            "properties": {
              "command": { "type": "string", "description": "Shell command to run (passed to /bin/sh -c)" }
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
          "description": "Execute a shell command in a temporary isolated working directory with a reduced environment. Use this to test commands before using shell_exec in the real workspace.",
          "parameters": {
            "type": "object",
            "properties": {
              "command": { "type": "string", "description": "Shell command to run inside the sandbox (passed to /bin/sh -c)" },
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
                let risk = assess_command_risk(cmd);
                if let Some(warning) = format_risk_warning(cmd, risk) {
                    debug!("[tool/shell_exec] BLOCKED high-risk command: {}", truncate_chars(cmd, 80));
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
            other => Err(anyhow!("unknown tool: '{other}'")),
        }
    }

    // Tool implementations are in sibling modules: shell.rs, file.rs, network.rs
}
