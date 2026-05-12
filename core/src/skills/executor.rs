use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::debug;

use crate::agents::alpha::infer_context_limit_for_model;
use crate::llm::client::LlmClient;
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::policy::{InvocationSource, PolicyActor};
use crate::skills::permissions::PermissionChecker;
use crate::skills::registry::{LoadedSkillPrompt, SkillPermissions, SkillRegistry};
use crate::tool_loop::{
    run_tool_loop, ContextBudget, PreparedToolLoopIteration, ToolLoopControl, ToolLoopDelegate,
};
use crate::tools::primitive::{ToolPermissions, ToolRegistry};

#[derive(Clone)]
pub struct SkillExecutor {
    pub registry: SkillRegistry,
    pub permissions: PermissionChecker,
    pub mcp: Arc<MCPOrchestrator>,
    pub llm: Arc<LlmClient>,
    pub tools: Arc<ToolRegistry>,
}

struct SkillToolLoopState<'a> {
    executor: &'a SkillExecutor,
    name: &'a str,
    messages: Vec<serde_json::Value>,
    available_tools: Vec<serde_json::Value>,
    tool_perms: ToolPermissions,
    policy_actor: PolicyActor,
    budget: ContextBudget,
}

#[async_trait]
impl ToolLoopDelegate for SkillToolLoopState<'_> {
    type Output = String;

    fn loop_label(&self) -> &str {
        self.name
    }

    fn messages(&self) -> &Vec<serde_json::Value> {
        &self.messages
    }

    fn messages_mut(&mut self) -> &mut Vec<serde_json::Value> {
        &mut self.messages
    }

    fn max_tool_rounds(&self) -> usize {
        SkillExecutor::MAX_TOOL_ROUNDS
    }

    fn context_budget(&self) -> ContextBudget {
        self.budget
    }

    async fn prepare_iteration(&mut self, _iteration: usize) -> Result<PreparedToolLoopIteration> {
        Ok(PreparedToolLoopIteration {
            tools: self.available_tools.clone(),
        })
    }

    fn log_context(&self, iteration: usize, estimated_tokens: u64, tools: &[serde_json::Value]) {
        debug!(
            "[skill/{}] context(iter={}): messages={} tools={} est_tokens={} limit={} target={} reserve={}",
            self.name,
            iteration,
            self.messages.len(),
            tools.len(),
            estimated_tokens,
            self.budget.context_limit(),
            self.budget.target_budget(),
            self.budget.response_reserve(),
        );
    }

    async fn handle_tool_call(
        &mut self,
        tc: &crate::llm::client::ToolCall,
        _budget: &ContextBudget,
    ) -> Result<ToolLoopControl<Self::Output>> {
        debug!("[skill/{}] tool_call: {}", self.name, tc.name);

        let result = if tc.name.starts_with("mcp__") {
            match self.executor.mcp.resolve_fn_name(&tc.name).await {
                Some((server, tool)) => {
                    let request = self.executor.permissions.policy_request_for_mcp_call(
                        self.policy_actor.clone(),
                        &server,
                        &tool,
                        &tc.arguments,
                    );
                    match self.executor.permissions.authorize(request.clone()).await {
                        Ok(true) => self
                            .executor
                            .mcp
                            .call_tool(&server, &tool, &tc.arguments)
                            .await
                            .map(|v| v.to_string())
                            .unwrap_or_else(|e| format!("MCP error: {e}")),
                        Ok(false) => {
                            self.executor
                                .permissions
                                .denial_diagnostics("Permission denied for MCP tool call", &request)
                                .await
                        }
                        Err(e) => {
                            let diag = self
                                .executor
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
            let request = self.executor.tools.policy_request_for_tool(
                self.policy_actor.clone(),
                &tc.name,
                &tc.arguments,
            );
            match self.executor.permissions.authorize(request.clone()).await {
                Ok(true) => self
                    .executor
                    .tools
                    .execute(
                        &tc.name,
                        &tc.arguments,
                        &self.tool_perms,
                        &self.executor.permissions,
                        &self.policy_actor,
                    )
                    .await
                    .unwrap_or_else(|e| format!("Error: {e}")),
                Ok(false) => {
                    self.executor
                        .permissions
                        .denial_diagnostics("Permission denied for tool call", &request)
                        .await
                }
                Err(e) => {
                    let diag = self
                        .executor
                        .permissions
                        .denial_diagnostics("Permission check failed for tool call", &request)
                        .await;
                    format!("{diag}; error={e}")
                }
            }
        };

        Ok(ToolLoopControl::AppendToolResult { content: result })
    }

    async fn after_tool_result_appended(&mut self, tool_name: &str) -> Result<()> {
        debug!(
            "[skill/{}] {} → {} chars",
            self.name,
            tool_name,
            self.messages
                .last()
                .and_then(|message| message["content"].as_str())
                .map(str::len)
                .unwrap_or_default()
        );
        if tool_name == "file_write" || tool_name == "shell_exec" {
            self.executor.registry.refresh().await;
        }
        Ok(())
    }

    async fn finalize_text_response(&mut self, text: String) -> Result<Self::Output> {
        debug!("[skill/{}] final answer: {} chars", self.name, text.len());
        Ok(text)
    }

    async fn on_round_limit_exceeded(
        &mut self,
        _llm: Arc<LlmClient>,
        _abort: &crate::llm::client::AbortFlag,
    ) -> Result<Self::Output> {
        Err(anyhow!(
            "skill '{}' exceeded maximum tool-call rounds ({})",
            self.name,
            SkillExecutor::MAX_TOOL_ROUNDS
        ))
    }

    fn aborted_output(&self) -> Self::Output {
        String::new()
    }
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

        let messages: Vec<serde_json::Value> = vec![
            serde_json::json!({ "role": "system", "content": prompt_text }),
            serde_json::json!({ "role": "user", "content": input }),
        ];
        let budget = ContextBudget::new(infer_context_limit_for_model(&self.llm.model_name()));
        let on_token_ref = &on_token as &(dyn Fn(&str) + Send + Sync);
        let mut state = SkillToolLoopState {
            executor: self,
            name,
            messages,
            available_tools,
            tool_perms,
            policy_actor,
            budget,
        };
        run_tool_loop(&mut state, self.llm.clone(), &abort, on_token_ref).await
    }
}
