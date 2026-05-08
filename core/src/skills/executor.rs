use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tracing::debug;

use crate::llm::client::LlmClient;
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::policy::{InvocationSource, PolicyActor};
use crate::skills::permissions::PermissionChecker;
use crate::skills::registry::{LoadedSkillPrompt, SkillPermissions, SkillRegistry};
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
    /// Maximum MCP tool schemas injected into a standalone skill run.
    /// Skills usually operate on a narrower task than Alpha, so we keep the
    /// pool smaller to reduce prompt bloat and improve tool selection quality.
    const MAX_MCP_TOOLS: usize = 24;

    /// Maximum assistant -> tool -> assistant rounds in a standalone skill run.
    /// This is a hard stop because scheduled/command-driven skill execution has
    /// no broader conversation loop to gracefully wrap up inside.
    const MAX_TOOL_ROUNDS: usize = 24;

    /// Load a skill's prompt and permissions for context injection.
    /// Confirms dangerous skills with the user before returning.
    pub async fn load_skill_prompt(&self, name: &str) -> Result<(String, SkillPermissions)> {
        self.load_skill_prompt_from_source(name, InvocationSource::Ui)
            .await
    }

    pub async fn load_skill_prompt_from_source(
        &self,
        name: &str,
        source: InvocationSource,
    ) -> Result<(String, SkillPermissions)> {
        let loaded = self.load_skill_bundle_from_source(name, source).await?;
        Ok((loaded.render_with_preamble(), loaded.permissions.clone()))
    }

    pub async fn load_skill_bundle(&self, name: &str) -> Result<LoadedSkillPrompt> {
        self.load_skill_bundle_from_source(name, InvocationSource::Ui)
            .await
    }

    pub async fn load_skill_bundle_from_source(
        &self,
        name: &str,
        source: InvocationSource,
    ) -> Result<LoadedSkillPrompt> {
        let loaded = self.registry.load_skill_prompt(name).await?;

        if loaded.permissions.dangerous || loaded.permissions.dangerous_operations {
            let allowed = self
                .permissions
                .authorize_skill_activation(
                    PolicyActor::from_agent_name(
                        &format!("skill:{}", loaded.metadata.name),
                        source,
                    ),
                    &loaded.metadata.name,
                    true,
                )
                .await?;
            if !allowed {
                return Err(anyhow!(
                    "Skill '{}' was not permitted.",
                    loaded.metadata.name
                ));
            }
        }

        Ok(loaded)
    }

    /// Standalone execution for scheduler and commands — runs the skill in its
    /// own tool-call loop (no conversation context).  The main conversation
    /// paths use `load_skill_prompt` + context injection instead.
    pub async fn execute_skill(&self, name: &str, input: &str) -> Result<String> {
        self.execute_skill_stream(name, input, |_| {}).await
    }

    pub async fn execute_skill_from_source(
        &self,
        name: &str,
        input: &str,
        source: InvocationSource,
    ) -> Result<String> {
        self.execute_skill_stream_from_source(name, input, source, |_| {})
            .await
    }

    /// Like `execute_skill` but accepts an `on_token` callback for streaming
    /// intermediate LLM output to the caller.
    pub async fn execute_skill_stream(
        &self,
        name: &str,
        input: &str,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<String> {
        self.execute_skill_stream_from_source(name, input, InvocationSource::Ui, on_token)
            .await
    }

    pub async fn execute_skill_stream_from_source(
        &self,
        name: &str,
        input: &str,
        source: InvocationSource,
        on_token: impl Fn(&str) + Send + Sync,
    ) -> Result<String> {
        let (prompt_text, permissions) = self.load_skill_prompt_from_source(name, source).await?;
        let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let policy_actor = PolicyActor::from_agent_name(&format!("skill:{name}"), source);

        let tool_perms = ToolPermissions {
            shell: permissions.shell,
            sandbox_shell: permissions.sandbox_shell,
            file_read: permissions.file_read,
            file_write: permissions.file_write,
            network: permissions.network,
        };

        let mut available_tools = self.tools.available_tools(&tool_perms);

        if permissions.mcp {
            let mcp_specs = self.mcp.tools_for_task(input, Self::MAX_MCP_TOOLS).await;
            debug!("[skill/{name}] injecting {} MCP tools", mcp_specs.len());
            available_tools.extend(mcp_specs);
        }

        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({ "role": "system", "content": prompt_text }),
            serde_json::json!({ "role": "user", "content": input }),
        ];

        for iteration in 0..Self::MAX_TOOL_ROUNDS {
            if abort.load(Ordering::Relaxed) {
                debug!("[skill/{name}] aborted at iteration {iteration}");
                return Ok(String::new());
            }

            let response = match self
                .llm
                .chat_with_tools_stream(
                    messages.clone(),
                    available_tools.clone(),
                    &on_token,
                    &abort,
                )
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
                        Some((server, tool)) => {
                            let request = self.permissions.policy_request_for_mcp_call(
                                policy_actor.clone(),
                                &server,
                                &tool,
                                &tc.arguments,
                            );
                            match self.permissions.authorize(request.clone()).await {
                                Ok(true) => self
                                    .mcp
                                    .call_tool(&server, &tool, &tc.arguments)
                                    .await
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|e| format!("MCP error: {e}")),
                                Ok(false) => {
                                    self.permissions
                                        .denial_diagnostics(
                                            "Permission denied for MCP tool call",
                                            &request,
                                        )
                                        .await
                                }
                                Err(e) => {
                                    let diag = self
                                        .permissions
                                        .denial_diagnostics(
                                            "Permission check failed for MCP tool call",
                                            &request,
                                        )
                                        .await;
                                    format!("{diag}; error={e}")
                                }
                            }
                        }
                        None => format!("Unknown MCP tool: '{}'", tc.name),
                    }
                } else {
                    let request = self.tools.policy_request_for_tool(
                        policy_actor.clone(),
                        &tc.name,
                        &tc.arguments,
                    );
                    match self.permissions.authorize(request.clone()).await {
                        Ok(true) => self
                            .tools
                            .execute(
                                &tc.name,
                                &tc.arguments,
                                &tool_perms,
                                &self.permissions,
                                &policy_actor,
                            )
                            .await
                            .unwrap_or_else(|e| format!("Error: {e}")),
                        Ok(false) => {
                            self.permissions
                                .denial_diagnostics("Permission denied for tool call", &request)
                                .await
                        }
                        Err(e) => {
                            let diag = self
                                .permissions
                                .denial_diagnostics(
                                    "Permission check failed for tool call",
                                    &request,
                                )
                                .await;
                            format!("{diag}; error={e}")
                        }
                    }
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
            "skill '{name}' exceeded maximum tool-call rounds ({})",
            Self::MAX_TOOL_ROUNDS
        ))
    }
}
