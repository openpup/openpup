use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tracing::debug;

use crate::llm::client::{AbortFlag, LlmClient};
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::skills::permissions::{Action, PermissionChecker};
use crate::skills::registry::{SkillPermissions, SkillPrompt, SkillRegistry};
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
    /// Non-streaming entry point.
    pub async fn execute_skill(&self, name: &str, input: &str) -> Result<String> {
        let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.execute_skill_stream(
            name,
            input,
            Arc::new(|_tok: String, _is_reasoning: bool| {}),
            Arc::new(|_kind: String, _label: String| {}),
            abort,
        )
        .await
    }

    /// Streaming entry point — runs the skill's `[prompt]` tool-call loop.
    pub async fn execute_skill_stream(
        &self,
        name: &str,
        input: &str,
        on_token: Arc<dyn Fn(String, bool) + Send + Sync>,
        on_activity: Arc<dyn Fn(String, String) + Send + Sync>,
        abort: AbortFlag,
    ) -> Result<String> {
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
                return Ok(format!(
                    "Skill '{}' was not permitted.",
                    manifest.metadata.name
                ));
            }
        }

        let prompt = manifest
            .prompt
            .as_ref()
            .ok_or_else(|| anyhow!("skill '{name}' has no [prompt] section"))?;

        self.run_tool_loop(
            name,
            input,
            prompt,
            &manifest.permissions,
            on_token,
            on_activity,
            abort,
        )
        .await
    }

    async fn run_tool_loop(
        &self,
        skill_name: &str,
        input: &str,
        prompt: &SkillPrompt,
        permissions: &SkillPermissions,
        on_token: Arc<dyn Fn(String, bool) + Send + Sync>,
        on_activity: Arc<dyn Fn(String, String) + Send + Sync>,
        abort: AbortFlag,
    ) -> Result<String> {
        let tool_perms = ToolPermissions {
            shell: permissions.shell,
            filesystem: permissions.filesystem,
            network: permissions.network,
        };

        let mut available_tools = self.tools.available_tools(&tool_perms);

        if permissions.mcp {
            const MAX_MCP: usize = 20;
            let mcp_specs = self.mcp.tools_for_task(input, MAX_MCP).await;
            debug!(
                "[skill/{skill_name}] injecting {} MCP tools",
                mcp_specs.len()
            );
            available_tools.extend(mcp_specs);
        }

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({ "role": "system", "content": prompt.system }),
            serde_json::json!({ "role": "user", "content": input }),
        ];

        const MAX_ITERATIONS: usize = 20;

        for iteration in 0..MAX_ITERATIONS {
            if abort.load(Ordering::Relaxed) {
                debug!("[skill/{skill_name}] aborted at iteration {iteration}");
                return Ok(String::new());
            }

            let response = match self
                .llm
                .chat_with_tools_abortable(messages.clone(), available_tools.clone(), &abort)
                .await?
            {
                Some(r) => r,
                None => {
                    debug!("[skill/{skill_name}] aborted during LLM call");
                    return Ok(String::new());
                }
            };

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                debug!("[skill/{skill_name}] final answer: {} chars", text.len());
                on_token(text.clone(), false);
                return Ok(text);
            }

            messages.push(response.raw_message);

            for tc in &response.tool_calls {
                if abort.load(Ordering::Relaxed) {
                    return Ok(String::new());
                }

                debug!("[skill/{skill_name}] tool_call: {}", tc.name);
                let (act_kind, act_label) =
                    crate::agents::alpha::describe_tool_call(&tc.name, &tc.arguments);
                on_activity(act_kind, act_label);

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

                // After a file write, re-scan skill directories so any newly
                // written skill TOML is live for the scheduler and future calls.
                if tc.name == "file_write" || tc.name == "shell_exec" {
                    self.registry.refresh().await;
                }
            }
        }

        Err(anyhow!(
            "skill '{skill_name}' exceeded maximum tool-call iterations ({MAX_ITERATIONS})"
        ))
    }
}
