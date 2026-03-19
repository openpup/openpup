/// Primitive tool registry — the only LLM-callable operations.
///
/// Skills declare which permission flags they need; only the corresponding
/// tool schemas are included in the API request so the LLM cannot exceed
/// the declared capability surface.
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::memory::system::MemorySystem;

// ── Permission surface ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ToolPermissions {
    pub shell: bool,
    pub filesystem: bool,
    pub network: bool,
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    pub workspace_root: PathBuf,
    pub memory: Arc<MemorySystem>,
    http: reqwest::Client,
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
        }
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

        if perms.filesystem {
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
                if !perms.filesystem {
                    return Err(anyhow!("file_read: permission denied (skill requires permissions.filesystem = true)"));
                }
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("file_read: missing 'path'"))?;
                self.file_read(path).await
            }
            "file_write" => {
                if !perms.filesystem {
                    return Err(anyhow!("file_write: permission denied (skill requires permissions.filesystem = true)"));
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
        eprintln!("[tool/shell_exec] $ {}", &command[..command.len().min(120)]);
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
        // Cap output to avoid overflowing the context window
        if trimmed.len() > 16_384 {
            Ok(format!(
                "{}\n… [truncated, {} chars total]",
                &trimmed[..16_384],
                trimmed.len()
            ))
        } else {
            Ok(trimmed)
        }
    }

    async fn file_read(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        eprintln!("[tool/file_read] {}", resolved.display());
        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow!("file_read '{}': {e}", resolved.display()))?;
        if content.len() > 32_768 {
            Ok(format!(
                "{}\n… [truncated, {} chars total]",
                &content[..32_768],
                content.len()
            ))
        } else {
            Ok(content)
        }
    }

    async fn file_write(&self, path: &str, content: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        eprintln!(
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
        eprintln!("[tool/http_get] {}", &url[..url.len().min(120)]);
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
        if body.len() > 8_192 {
            Ok(format!(
                "{}\n… [truncated, {} chars total]",
                &body[..8_192],
                body.len()
            ))
        } else {
            Ok(body)
        }
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
