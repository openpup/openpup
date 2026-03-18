use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::llm::client::{AbortFlag, LlmClient, LlmMessage};
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::memory::system::MemorySystem;
use crate::skills::permissions::{Action, PermissionChecker};
use crate::skills::registry::{PromptChainStep, SkillPermissions, SkillPrompt, SkillRegistry};
use crate::tools::primitive::{ToolPermissions, ToolRegistry};

#[derive(Clone)]
pub struct SkillExecutor {
  pub registry: SkillRegistry,
  pub permissions: PermissionChecker,
  pub mcp: Arc<MCPOrchestrator>,
  pub llm: Arc<LlmClient>,
  pub memory: Arc<MemorySystem>,
  pub tools: Arc<ToolRegistry>,
}

impl SkillExecutor {
  /// Non-streaming entry point — used by the scheduler and legacy callers.
  /// Internally delegates to `execute_skill_stream`, discarding token events.
  pub async fn execute_skill(&self, name: &str, input: &str) -> Result<String> {
    let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
    self
      .execute_skill_stream(
        name,
        input,
        Arc::new(|_tok: String, _is_reasoning: bool| {}),
        Arc::new(|_kind: String, _label: String| {}),
        abort,
      )
      .await
  }

  /// Streaming entry point.
  ///
  /// For skills with a `[prompt]` section the LLM drives execution through
  /// the primitive tool-call loop; the final text response is delivered via
  /// `on_token`.
  ///
  /// For skills with an `[implementation]` section the legacy prompt_chain
  /// runner is used and the final text is emitted as a single token.
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
    let is_dangerous =
      manifest.permissions.dangerous || manifest.permissions.dangerous_operations;
    if is_dangerous {
      let allowed = self
        .permissions
        .check_permission(
          &manifest.metadata.name,
          &Action::Other(format!("execute skill {}", manifest.metadata.name)),
        )
        .await?;
      if !allowed {
        return Ok(format!("Skill '{}' was not permitted.", manifest.metadata.name));
      }
    }

    // ── New-style: primitive tool-call loop ────────────────────────────────
    if let Some(prompt) = &manifest.prompt {
      return self
        .run_tool_loop(name, input, prompt, &manifest.permissions, on_token, on_activity, abort)
        .await;
    }

    // ── Legacy: prompt_chain / builtin ─────────────────────────────────────
    if let Some(impl_) = &manifest.implementation {
      let result = match impl_.r#type.as_str() {
        "prompt_chain" => self.run_prompt_chain(name, input, &impl_.steps).await?,
        "builtin" => self.run_builtin(name, input).await?,
        other => return Err(anyhow!("unknown skill type '{other}'")),
      };
      on_token(result.clone(), false);
      return Ok(result);
    }

    Err(anyhow!("skill '{name}' has neither [prompt] nor [implementation]"))
  }

  // ── Primitive tool-call loop ──────────────────────────────────────────────

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

    // Primitive tools filtered by permission flags
    let mut available_tools = self.tools.available_tools(&tool_perms);

    // If the skill declares mcp = true, inject relevant MCP tools (keyword-filtered)
    if permissions.mcp {
      const MAX_MCP: usize = 20;
      let mcp_specs = self.mcp.tools_for_task(input, MAX_MCP).await;
      eprintln!("[skill/{skill_name}] injecting {} MCP tools (filtered)", mcp_specs.len());
      available_tools.extend(mcp_specs);
    }

    // Seed the conversation
    let mut messages: Vec<serde_json::Value> = vec![
      serde_json::json!({ "role": "system", "content": prompt.system }),
      serde_json::json!({ "role": "user", "content": input }),
    ];

    const MAX_ITERATIONS: usize = 12;

    for iteration in 0..MAX_ITERATIONS {
      if abort.load(Ordering::Relaxed) {
        eprintln!("[skill/{skill_name}] aborted at iteration {iteration}");
        return Ok(String::new());
      }

      let response = self.llm.chat_with_tools(messages.clone(), available_tools.clone()).await?;

      if response.tool_calls.is_empty() {
        // Model returned a text answer — stream it and finish
        let text = response.content.unwrap_or_default();
        eprintln!("[skill/{skill_name}] final answer: {} chars", text.len());
        on_token(text.clone(), false);
        return Ok(text);
      }

      // Model requested tool calls — execute each and feed results back
      messages.push(response.raw_message);

      for tc in &response.tool_calls {
        eprintln!("[skill/{skill_name}] tool_call: {}", tc.name);
        on_activity("tool_call".into(), tc.name.clone());

        // Route mcp__<server>__<tool> calls to the MCP orchestrator
        let result = if let Some(rest) = tc.name.strip_prefix("mcp__") {
          let mut parts = rest.splitn(2, "__");
          match (parts.next(), parts.next()) {
            (Some(server), Some(tool)) => self
              .mcp
              .call_tool(server, tool, &tc.arguments)
              .await
              .map(|v| v.to_string())
              .unwrap_or_else(|e| format!("MCP error: {e}")),
            _ => format!("Invalid MCP tool name: '{}'", tc.name),
          }
        } else {
          self
            .tools
            .execute(&tc.name, &tc.arguments, &tool_perms)
            .await
            .unwrap_or_else(|e| format!("Error: {e}"))
        };

        eprintln!("[skill/{skill_name}] {} -> {} chars", tc.name, result.len());
        messages.push(serde_json::json!({
          "role": "tool",
          "tool_call_id": tc.id,
          "content": result,
        }));
      }
    }

    Err(anyhow!("skill '{skill_name}' exceeded maximum tool-call iterations ({MAX_ITERATIONS})"))
  }

  // ── Legacy prompt_chain ───────────────────────────────────────────────────

  async fn run_prompt_chain(
    &self,
    _skill_name: &str,
    input: &str,
    steps: &[PromptChainStep],
  ) -> Result<String> {
    let mut context = input.to_string();
    for step in steps {
      context = self.run_step(&context, step).await?;
    }
    Ok(context)
  }

  async fn run_step(&self, context: &str, step: &PromptChainStep) -> Result<String> {
    match step.action.as_str() {
      "search_memories" => {
        let query =
          step.params.get("query").and_then(|v| v.as_str()).unwrap_or(context);
        let limit =
          step.params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let results = self.memory.search_long_term(query, limit).await?;
        if results.is_empty() {
          Ok(context.to_string())
        } else {
          Ok(format!("{context}\n\n[Memories]\n{}", results.join("\n")))
        }
      }

      "list_available_skills" => {
        let installed = self.registry.list_installed().await;
        let list = installed
          .iter()
          .map(|s| format!("- **{}** ({}): {}", s.name, s.category, s.description))
          .collect::<Vec<_>>()
          .join("\n");
        Ok(format!(
          "{context}\n\n[Installed Skills]\n{}",
          if list.is_empty() { "（无已安装技能）".to_string() } else { list }
        ))
      }

      "summarize_with_llm" | "generate_with_llm" => {
        let system_prompt = step
          .params
          .get("system_prompt")
          .and_then(|v| v.as_str())
          .unwrap_or("You are a helpful assistant.");
        self
          .llm
          .chat(vec![
            LlmMessage {
              role: "system".to_string(),
              content: system_prompt.to_string(),
            },
            LlmMessage { role: "user".to_string(), content: context.to_string() },
          ])
          .await
      }

      "call_mcp" => {
        let server =
          step.params.get("server").and_then(|v| v.as_str()).unwrap_or("local");
        let tool = step
          .params
          .get("tool")
          .and_then(|v| v.as_str())
          .ok_or_else(|| anyhow!("call_mcp step missing 'tool'"))?;
        let mut params = step.params.clone();
        if params.get("input").is_none() {
          params["input"] = serde_json::Value::String(context.to_string());
        }
        let result = self.mcp.call_tool(server, tool, &params).await?;
        Ok(result.to_string())
      }

      "write_file" => {
        let path = step
          .params
          .get("path")
          .and_then(|v| v.as_str())
          .ok_or_else(|| anyhow!("write_file step missing 'path'"))?;
        let result = self
          .mcp
          .call_tool(
            "local",
            "write_file",
            &serde_json::json!({ "path": path, "content": context }),
          )
          .await?;
        Ok(result.to_string())
      }

      "open_browser" => {
        let url = step.params.get("url").and_then(|v| v.as_str()).unwrap_or(context);
        let result = self
          .mcp
          .call_tool("local", "open_browser", &serde_json::json!({ "url": url }))
          .await?;
        Ok(result.to_string())
      }

      other => Err(anyhow!("unknown step action '{other}'")),
    }
  }

  // ── Builtin skills ────────────────────────────────────────────────────────

  async fn run_builtin(&self, skill_name: &str, _input: &str) -> Result<String> {
    Err(anyhow!("unknown builtin skill '{skill_name}'"))
  }
}
