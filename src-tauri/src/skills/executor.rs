use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tracing::debug;

use crate::llm::client::LlmClient;
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::skills::permissions::{Action, PermissionChecker};
use crate::skills::registry::{SkillPermissions, SkillRegistry};
use crate::tools::primitive::{ToolPermissions, ToolRegistry};

#[derive(Clone)]
pub struct SkillExecutor {
    pub registry: SkillRegistry,
    pub permissions: PermissionChecker,
    pub mcp: Arc<MCPOrchestrator>,
    pub llm: Arc<LlmClient>,
    pub tools: Arc<ToolRegistry>,
}

impl SkillExecutor {
    /// Load a skill's prompt and permissions for context injection.
    /// Confirms dangerous skills with the user before returning.
    pub async fn load_skill_prompt(&self, name: &str) -> Result<(String, SkillPermissions)> {
        let manifest = self.registry.ensure_skill(name).await?;

        // Confirm dangerous skills with the user
        if manifest.permissions.dangerous || manifest.permissions.dangerous_operations {
            let allowed = self
                .permissions
                .check_permission(
                    &manifest.metadata.name,
                    &Action::Other(format!("execute skill {}", manifest.metadata.name)),
                )
                .await?;
            if !allowed {
                return Err(anyhow!("Skill '{}' was not permitted.", manifest.metadata.name));
            }
        }

        let permissions = manifest.permissions.clone();
        let prompt = manifest
            .prompt
            .ok_or_else(|| anyhow!("skill '{name}' has no [prompt] section"))?;

        Ok((prompt.system, permissions))
    }

    /// Standalone execution for scheduler and commands — runs the skill in its
    /// own tool-call loop (no conversation context).  The main conversation
    /// paths use `load_skill_prompt` + context injection instead.
    pub async fn execute_skill(&self, name: &str, input: &str) -> Result<String> {
        let (prompt_text, permissions) = self.load_skill_prompt(name).await?;
        let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let tool_perms = ToolPermissions {
            shell: permissions.shell,
            file_read: permissions.file_read,
            file_write: permissions.file_write,
            network: permissions.network,
        };

        let mut available_tools = self.tools.available_tools(&tool_perms);

        if permissions.mcp {
            const MAX_MCP: usize = 20;
            let mcp_specs = self.mcp.tools_for_task(input, MAX_MCP).await;
            debug!(
                "[skill/{name}] injecting {} MCP tools",
                mcp_specs.len()
            );
            available_tools.extend(mcp_specs);
        }

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({ "role": "system", "content": prompt_text }),
            serde_json::json!({ "role": "user", "content": input }),
        ];

        const MAX_ITERATIONS: usize = 20;

        for iteration in 0..MAX_ITERATIONS {
            if abort.load(Ordering::Relaxed) {
                debug!("[skill/{name}] aborted at iteration {iteration}");
                return Ok(String::new());
            }

            let response = match self
                .llm
                .chat_with_tools_abortable(messages.clone(), available_tools.clone(), &abort)
                .await?
            {
                Some(r) => r,
                None => {
                    debug!("[skill/{name}] aborted during LLM call");
                    return Ok(String::new());
                }
            };

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                debug!("[skill/{name}] final answer: {} chars", text.len());
                return Ok(text);
            }

            messages.push(response.raw_message);

            for tc in &response.tool_calls {
                debug!("[skill/{name}] tool_call: {}", tc.name);

                let result = if tc.name.starts_with("mcp__") {
                    match self.mcp.resolve_fn_name(&tc.name).await {
                        Some((server, tool)) => self
                            .mcp
                            .call_tool(&server, &tool, &tc.arguments)
                            .await
                            .map(|v| v.to_string())
                            .unwrap_or_else(|e| format!("MCP error: {e}")),
                        None => format!("Unknown MCP tool: '{}'", tc.name),
                    }
                } else {
                    self.tools
                        .execute(&tc.name, &tc.arguments, &tool_perms)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"))
                };

                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": result,
                }));

                if tc.name == "file_write" || tc.name == "shell_exec" {
                    self.registry.refresh().await;
                }
            }
        }

        Err(anyhow!(
            "skill '{name}' exceeded maximum tool-call iterations ({MAX_ITERATIONS})"
        ))
    }
}
