/// Primitive tool registry — the only LLM-callable operations.
///
/// Skills declare which permission flags they need; only the corresponding
/// tool schemas are included in the API request so the LLM cannot exceed
/// the declared capability surface.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::debug;

use crate::memory::system::MemorySystem;

// ── Permission surface ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ToolPermissions {
    pub shell: bool,
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
    http: reqwest::Client,
    /// Context window limit in tokens — tool results are truncated proportionally.
    context_limit: std::sync::atomic::AtomicU64,
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

impl ToolRegistry {
    pub fn new(workspace_root: PathBuf, memory: Arc<MemorySystem>) -> Self {
        Self {
            workspace_root,
            memory,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            context_limit: std::sync::atomic::AtomicU64::new(128_000),
        }
    }

    /// Update the context limit (called once after model is known).
    pub fn set_context_limit(&self, limit: u64) {
        self.context_limit.store(limit, std::sync::atomic::Ordering::Relaxed);
    }

    /// Dynamic max chars for tool results: 30% of context window × 4 chars/token, clamped.
    fn tool_result_max_chars(&self) -> usize {
        let limit = self.context_limit.load(std::sync::atomic::Ordering::Relaxed);
        let max = ((limit as f64 * 0.30) * 4.0) as usize;
        max.clamp(2_000, 32_768)
    }

    /// Truncate tool results using head+tail strategy: keep the first ~70% and
    /// last ~30% of the budget so that error messages at the end aren't lost.
    fn dynamic_truncate(&self, text: &str) -> String {
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
          "description": "Execute a shell command and return combined stdout + stderr.",
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
          "description": "Perform an HTTP GET request and return the response body (truncated to 8 KB).",
          "parameters": {
            "type": "object",
            "properties": {
              "url": { "type": "string", "description": "URL to fetch" }
            },
            "required": ["url"]
          }
        }
      }));
        }

        // Memory tools are always available — they don't need a permission flag.
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
                self.shell_exec(cmd).await
            }
            "file_read" => {
                if !perms.file_read {
                    return Err(anyhow!("file_read: permission denied (requires permissions.file_read = true)"));
                }
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_read: missing 'path'"))?;
                self.file_read(path).await
            }
            "file_write" => {
                if !perms.file_write {
                    return Err(anyhow!("file_write: permission denied (requires permissions.file_write = true)"));
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

    // ── Tool implementations ───────────────────────────────────────────────────

    async fn shell_exec(&self, command: &str) -> Result<String> {
        debug!("[tool/shell_exec] $ {}", truncate_chars(command, 120));
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.workspace_root)
            .output()
            .await
            .map_err(|e| anyhow!("shell_exec failed to spawn: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let combined = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            format!("[stderr]\n{stderr}")
        } else {
            format!("{stdout}\n[stderr]\n{stderr}")
        };
        let trimmed = combined.trim().to_string();
        Ok(self.dynamic_truncate(&trimmed))
    }

    async fn file_read(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        debug!("[tool/file_read] {}", resolved.display());
        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow!("file_read '{}': {e}", resolved.display()))?;
        Ok(self.dynamic_truncate(&content))
    }

    async fn file_write(&self, path: &str, content: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        debug!(
            "[tool/file_write] {} ({} bytes)",
            resolved.display(),
            content.len()
        );
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow!("file_write mkdir '{}': {e}", parent.display()))?;
        }
        tokio::fs::write(&resolved, content)
            .await
            .map_err(|e| anyhow!("file_write '{}': {e}", resolved.display()))?;
        Ok(format!(
            "Written {} bytes to '{}'",
            content.len(),
            resolved.display()
        ))
    }

    async fn http_get(&self, url: &str) -> Result<String> {
        debug!("[tool/http_get] {}", truncate_chars(url, 120));
        let resp = self
            .http
            .get(url)
            .header("User-Agent", "openpup/0.1")
            .send()
            .await
            .map_err(|e| anyhow!("http_get '{url}': {e}"))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("http_get '{url}': HTTP {status}"));
        }
        Ok(self.dynamic_truncate(&body))
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }
}
