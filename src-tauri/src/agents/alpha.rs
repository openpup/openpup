use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::RwLock;
use uuid::Uuid;

use tracing::debug;

use crate::agents::custom_pup::CustomPup;
use crate::agents::specialist::{Message, PupToolPermissions, SpecialistPup, Task, TaskStatus};
use crate::llm::client::{AbortFlag, LlmClient, LlmMessage};
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::memory::file_layer::FileLayer;
use crate::memory::system::{MemorySystem, TaskRecord};
use crate::skills::executor::SkillExecutor;
use crate::tools::primitive::ToolPermissions;

// ─── Pup configuration ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PupConfig {
    pub key: String,
    pub display_name: String,
    pub description: String,
    /// If non-empty, overrides the pup's built-in system prompt.
    #[serde(default)]
    pub system_prompt_override: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// True for user-created pups (can be deleted); false for built-ins.
    #[serde(default)]
    pub is_custom: bool,
}

fn default_true() -> bool {
    true
}

fn default_pup_configs() -> HashMap<String, PupConfig> {
    [
        ("dev", "Dev Pup", "代码、调试、Git、项目脚手架"),
        ("writer", "Writer Pup", "写作、编辑、翻译、内容创作"),
        ("ops", "Ops Pup", "系统命令、自动化、调度、提醒"),
        ("research", "Research Pup", "信息检索、事实核查、报告生成"),
        ("life_admin", "Life Admin Pup", "邮件、账单、购物、个人事务"),
    ]
    .into_iter()
    .map(|(key, name, desc)| {
        (
            key.to_string(),
            PupConfig {
                key: key.to_string(),
                display_name: name.to_string(),
                description: desc.to_string(),
                system_prompt_override: String::new(),
                enabled: true,
                is_custom: false,
            },
        )
    })
    .collect()
}

// ─── Event payloads ───────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
struct StreamDonePayload {
    pup_name: String,
    /// Authoritative final content from the backend (empty when aborted).
    content: String,
}

/// Emitted at key execution steps so the UI can show a live activity trace.
#[derive(serde::Serialize, Clone)]
pub struct ActivityEvent {
    /// "routing" | "skill" | "tool_call" | "tool_done"
    pub kind: String,
    pub label: String,
}

// ─── AlphaPup ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AlphaPup {
    pub memory: Arc<MemorySystem>,
    pub specialist_registry: Arc<RwLock<HashMap<String, Arc<dyn SpecialistPup>>>>,
    pub llm_client: Arc<LlmClient>,
    pub mcp_orchestrator: Arc<MCPOrchestrator>,
    pub file_layer: Arc<FileLayer>,
    pub skill_executor: Arc<SkillExecutor>,
    /// Shared abort flag — set to true to stop an ongoing stream.
    pub abort_flag: AbortFlag,
    /// Per-pup configuration (enabled state, system-prompt overrides, custom pups).
    pup_configs: Arc<RwLock<HashMap<String, PupConfig>>>,
    pup_config_path: Option<PathBuf>,
    /// Cached summarised OWNER.md with TTL.
    owner_summary_cache: Arc<RwLock<Option<(String, std::time::Instant)>>>,
    msg_count: Arc<std::sync::atomic::AtomicU32>,
}

impl AlphaPup {
    pub fn new(
        memory: Arc<MemorySystem>,
        llm_client: Arc<LlmClient>,
        mcp_orchestrator: Arc<MCPOrchestrator>,
        file_layer: Arc<FileLayer>,
        skill_executor: Arc<SkillExecutor>,
        pup_config_path: Option<PathBuf>,
    ) -> Self {
        // Load persisted pup configs, merging with defaults
        let mut configs = default_pup_configs();
        if let Some(ref path) = pup_config_path {
            if path.exists() {
                if let Ok(text) = fs::read_to_string(path) {
                    if let Ok(saved) = serde_json::from_str::<Vec<PupConfig>>(&text) {
                        for cfg in saved {
                            configs.insert(cfg.key.clone(), cfg);
                        }
                    }
                }
            }
        }
        Self {
            memory,
            specialist_registry: Arc::new(RwLock::new(HashMap::new())),
            llm_client,
            mcp_orchestrator,
            file_layer,
            skill_executor,
            abort_flag: Arc::new(AtomicBool::new(false)),
            pup_configs: Arc::new(RwLock::new(configs)),
            pup_config_path,
            owner_summary_cache: Arc::new(RwLock::new(None)),
            msg_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub async fn register_pup(&self, pup: Arc<dyn SpecialistPup>) {
        let mut guard = self.specialist_registry.write().await;
        guard.insert(pup.name().to_string(), pup);
    }

    /// Seed `msg_count` from the actual number of conversation exchanges already
    /// stored in the DB.  Call this once after construction so the extraction
    /// cadence survives app restarts.
    pub async fn init_msg_count(&self) {
        if let Ok(rows) = self.memory.conversation_count().await {
            // Each exchange = 2 rows (user + assistant); use exchange count.
            let exchanges = (rows / 2) as u32;
            self.msg_count.store(exchanges, Ordering::Relaxed);
            debug!("[alpha] init_msg_count: {exchanges} exchanges in DB");
        }
    }

    // ── Streaming entry point ──────────────────────────────────────────────────

    /// Process a user message with streaming output.
    /// If `forced_pup` is Some, routes directly to that pup bypassing intent classification.
    /// Emits `stream_token`, `stream_done`, or `stream_error` Tauri events.
    pub async fn process_user_message_stream(
        &self,
        msg: String,
        forced_pup: Option<String>,
        app_handle: &tauri::AppHandle,
    ) {
        debug!(
            "[alpha] process_user_message_stream: msg_len={} forced_pup={forced_pup:?}",
            msg.len()
        );
        self.abort_flag.store(false, Ordering::Relaxed);

        let result = self.do_stream(&msg, forced_pup, app_handle).await;
        match result {
            Ok((reply, pup_key)) => {
                debug!(
                    "[alpha] do_stream ok: pup={pup_key:?} reply_len={}",
                    reply.len()
                );
                let aborted = self.abort_flag.load(Ordering::Relaxed);
                // Emit stream_done with authoritative content from the backend.
                // Content is empty when the user aborted so the frontend discards partial output.
                debug!(
                    "[alpha] emitting stream_done pup={} aborted={aborted}",
                    pup_display_name(&pup_key)
                );
                let _ = app_handle.emit(
                    "stream_done",
                    StreamDonePayload {
                        pup_name: pup_display_name(&pup_key),
                        content: if aborted {
                            String::new()
                        } else {
                            reply.clone()
                        },
                    },
                );

                // Post-processing (memory writes + task creation) runs in background
                // so it never blocks the UI. Skip if aborted.
                if !aborted && !reply.is_empty() {
                    let self_clone = self.clone();
                    let msg_clone = msg.clone();
                    let pup_key_clone = pup_key.clone();
                    tauri::async_runtime::spawn(async move {
                        // 1. Persist conversation turns tagged with the pup that handled them
                        let _ = self_clone
                            .memory
                            .add_conversation(&pup_key_clone, "user", &msg_clone)
                            .await;
                        let _ = self_clone
                            .memory
                            .add_conversation(&pup_key_clone, "assistant", &reply)
                            .await;

                        // 2. Always write a brief diary entry for the conversation
                        let pup_label = pup_display_name(&pup_key_clone);
                        let snippet: String = msg_clone.chars().take(80).collect();
                        let ellipsis = if msg_clone.chars().count() > 80 {
                            "…"
                        } else {
                            ""
                        };
                        let diary_line = format!("💬 [{pup_label}] {snippet}{ellipsis}");
                        let _ = self_clone.file_layer.append_daily_diary(&[diary_line]);

                        // 3. Extract long-term memories every 3 exchanges; compress context
                        //    every 10 exchanges (per-pup).
                        //    msg_count is seeded from DB on startup so restarts don't reset it.
                        let count = self_clone.msg_count.fetch_add(1, Ordering::Relaxed);
                        if count % 3 == 0 {
                            let _ = self_clone.maybe_extract_memories(&pup_key_clone).await;
                        }
                        if count % 10 == 0 {
                            let _ = self_clone.maybe_compress_context(&pup_key_clone).await;
                        }

                        // 4. Maybe create a task record
                        self_clone
                            .maybe_create_task(&msg_clone, &pup_key_clone)
                            .await;
                    });
                }
            }
            Err(e) => {
                debug!("[alpha] do_stream error: {e}");
                let _ = app_handle.emit("stream_error", e.to_string());
            }
        }
    }

    async fn do_stream(
        &self,
        msg: &str,
        forced_pup: Option<String>,
        app_handle: &tauri::AppHandle,
    ) -> Result<(String, String)> {
        let owner_md = self.file_layer.read_owner_profile().unwrap_or_default();
        let owner_summary = self.get_owner_summary(&owner_md).await;
        let relevant_memories = self
            .memory
            .search_long_term(msg, 5)
            .await
            .unwrap_or_default();
        // Brief global history for intent classification (last 4 turns, all pups)
        let classify_history = self.build_classify_history().await;
        // Load pending tasks for context injection
        let pending_tasks: Vec<TaskRecord> = self
            .memory
            .list_tasks(5)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.status == "pending" || t.status == "in_progress")
            .collect();
        let pup_key = if let Some(forced) = forced_pup {
            forced
        } else if let Some(mention) = Self::extract_at_mention(msg, &self.pup_configs).await {
            mention
        } else {
            self.classify_intent(msg, &owner_summary, &classify_history)
                .await
        };
        debug!("[alpha] do_stream: pup_key={pup_key:?}");

        // Notify the UI which pup/skill is handling this request
        let _ = app_handle.emit(
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: pup_display_name(&pup_key),
            },
        );

        // If the user aborted while we were classifying intent, stop before hitting the LLM.
        if self.abort_flag.load(Ordering::Relaxed) {
            debug!("[alpha] do_stream: aborted before LLM call");
            return Ok((String::new(), "alpha".into()));
        }

        // Multi-pup parallel dispatch
        if let Some(pups_str) = pup_key.strip_prefix("channel:") {
            let required_pups: Vec<String> = pups_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if required_pups.len() >= 2 {
                let output = self
                    .run_parallel_pack(msg, required_pups, app_handle)
                    .await?;
                return Ok((output, pup_key));
            }
        }

        if let Some(skill_name) = pup_key.strip_prefix("skill:") {
            let handle = app_handle.clone();
            let handle2 = app_handle.clone();
            let run_id = Uuid::new_v4().to_string();
            let _ = self
                .memory
                .record_skill_run(&run_id, skill_name, "conversation")
                .await;
            let result = self
                .skill_executor
                .execute_skill_stream(
                    skill_name,
                    msg,
                    Arc::new(move |tok: String, _is_reasoning: bool| {
                        let _ = handle.emit("stream_token", tok);
                    }),
                    Arc::new(move |kind: String, label: String| {
                        let _ = handle2.emit("stream_activity", ActivityEvent { kind, label });
                    }),
                    self.abort_flag.clone(),
                )
                .await;
            let (status, output) = match result {
                Ok(o) => ("completed".to_string(), o),
                Err(e) => ("failed".to_string(), format!("Skill error: {e}")),
            };
            let _ = self
                .memory
                .complete_skill_run(&run_id, &status, &output)
                .await;
            let _ = app_handle.emit(
                "skill_run_completed",
                serde_json::json!({
                    "skill_name": skill_name,
                    "triggered_by": "conversation",
                    "status": status,
                }),
            );
            return Ok((output, pup_key));
        }

        if pup_key == "alpha" {
            let pup_history = self.build_history("alpha").await;
            let reply = self
                .alpha_reply_stream(
                    msg,
                    &owner_summary,
                    &pup_history,
                    &relevant_memories,
                    &pending_tasks,
                    app_handle,
                )
                .await?;
            return Ok((reply, "alpha".to_string()));
        }

        // Route to specialist pup — build task context then run shared tool loop
        let override_prompt = {
            let cfgs = self.pup_configs.read().await;
            cfgs.get(&pup_key).map(|c| c.system_prompt_override.clone())
        };
        let pup = self.specialist_registry.read().await.get(&pup_key).cloned();
        if let Some(pup) = pup {
            let mut enriched_memories = relevant_memories.clone();
            if !pending_tasks.is_empty() {
                let task_lines: String = pending_tasks
                    .iter()
                    .map(|t| format!("id:{} [{}] {}", t.id, t.status, t.description))
                    .collect::<Vec<_>>()
                    .join("；");
                enriched_memories.push(format!(
                    "当前待处理任务（用 task_update 工具更新状态）：{task_lines}"
                ));
            }
            let pup_history = self.build_history(&pup_key).await;
            let task = Task {
                id: Uuid::new_v4().to_string(),
                intent: msg.to_string(),
                context: pup_history
                    .iter()
                    .map(|m| Message {
                        role: m.role.clone(),
                        content: m.content.clone(),
                    })
                    .collect(),
                owner_context: owner_summary.clone(),
                relevant_memories: enriched_memories,
                system_prompt_override: override_prompt.filter(|s| !s.is_empty()),
                assigned_pup: Some(pup_key.clone()),
                status: TaskStatus::Pending,
            };

            let system_prompt = pup.build_system_prompt(&task);
            let tool_perms = pup.tool_permissions();

            // Build message list as JSON values for chat_with_tools
            let mut msgs: Vec<serde_json::Value> =
                vec![serde_json::json!({ "role": "system", "content": system_prompt })];
            for m in &task.context {
                msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
            }
            msgs.push(serde_json::json!({ "role": "user", "content": task.intent }));

            let handle = app_handle.clone();
            let handle2 = app_handle.clone();
            let output = self
                .run_agent_with_tools(
                    &pup_key,
                    msgs,
                    &tool_perms,
                    move |tok| {
                        let _ = handle.emit("stream_token", tok);
                    },
                    move |kind, label| {
                        let _ = handle2.emit("stream_activity", ActivityEvent { kind, label });
                    },
                    &self.abort_flag,
                )
                .await?;
            return Ok((output, pup_key));
        }

        // Fallback to alpha
        let fallback_history = self.build_history("alpha").await;
        let reply = self
            .alpha_reply_stream(
                msg,
                &owner_summary,
                &fallback_history,
                &relevant_memories,
                &pending_tasks,
                app_handle,
            )
            .await?;
        Ok((reply, "alpha".to_string()))
    }

    async fn alpha_reply_stream(
        &self,
        msg: &str,
        owner_summary: &str,
        history: &[LlmMessage],
        memories: &[String],
        pending_tasks: &[TaskRecord],
        app_handle: &tauri::AppHandle,
    ) -> Result<String> {
        let mut system_content = if owner_summary.contains("## Boundaries") {
            let summary = if owner_summary.chars().count() > 1000 {
                let truncated: String = owner_summary.chars().take(1000).collect();
                format!("{truncated}…")
            } else {
                owner_summary.to_string()
            };
            format!(
                "You are Alpha Pup, a loyal personal AI assistant. \
         Respond in the user's preferred language. Owner profile:\n\n{summary}"
            )
        } else {
            "You are Alpha Pup, a loyal personal AI assistant. Be concise and helpful.".to_string()
        };

        if !memories.is_empty() {
            let bullets: String = memories
                .iter()
                .map(|m| {
                    let capped: String = m.chars().take(200).collect();
                    format!("- {capped}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            system_content.push_str(&format!("\n\n## Relevant Memories\n{bullets}"));
        }

        if !pending_tasks.is_empty() {
            let tasks_str: String = pending_tasks
                .iter()
                .map(|t| format!("- id:{} [{}] {}", t.id, t.status, t.description))
                .collect::<Vec<_>>()
                .join("\n");
            system_content.push_str(&format!(
                "\n\n## 当前任务\n{tasks_str}\n\n使用 task_update 工具更新任务状态（开始时设为 in_progress，完成时设为 done）。"
            ));
        }

        // Inject installed skills so Alpha knows what capabilities are available
        let skill_list = self
            .skill_executor
            .registry
            .enabled_skills_for_tools()
            .await;
        if !skill_list.is_empty() {
            let lines: String = skill_list
                .iter()
                .map(|(name, desc, _)| format!("- skill__{name}: {desc}"))
                .collect::<Vec<_>>()
                .join("\n");
            system_content.push_str(&format!(
                "\n\n## 已安装技能\n{lines}\n\n可通过工具调用 skill__<name> 直接执行，也可告知用户已具备该能力。"
            ));
        }

        // Build messages as JSON values for the unified tool-call loop
        let mut messages: Vec<serde_json::Value> =
            vec![serde_json::json!({ "role": "system", "content": system_content })];
        for m in history {
            messages.push(serde_json::json!({ "role": m.role, "content": m.content }));
        }
        messages.push(serde_json::json!({ "role": "user", "content": msg }));

        // Alpha gets MCP access; no dangerous primitives in conversational context.
        let tool_perms = PupToolPermissions {
            shell: false,
            filesystem: false,
            network: false,
            mcp: true,
        };

        let handle = app_handle.clone();
        let handle2 = app_handle.clone();
        let handle3 = app_handle.clone();
        self.run_agent_with_tools(
            "alpha",
            messages,
            &tool_perms,
            move |tok| {
                // Emit reasoning tokens separately if the LLM uses them.
                // For now all tokens from the tool loop go to stream_token.
                let _ = handle.emit("stream_token", tok);
            },
            move |kind, label| {
                let _ = handle3.emit("stream_activity", ActivityEvent { kind, label });
            },
            &self.abort_flag,
        )
        .await
        .map_err(|e| {
            let _ = handle2.emit("stream_error", e.to_string());
            e
        })
    }

    // ── Unified agent tool-call loop ──────────────────────────────────────────

    /// Run a tool-call loop on behalf of any pup (or Alpha itself).
    ///
    /// Primitive tools are filtered by `tool_perms`; MCP tools from all enabled
    /// servers are injected when `tool_perms.mcp` is true.
    ///
    /// The loop uses non-streaming `chat_with_tools` for tool rounds.
    /// The final text response is emitted via `on_token` as a single chunk.
    /// Maximum MCP tools to inject per call — prevents context bloat and
    /// hitting provider tool-count limits. When the user has more MCP tools
    /// than this, `tools_for_task` keeps only the most relevant ones.
    const MAX_MCP_TOOLS: usize = 20;

    /// Build tool schemas for all currently enabled installed skills.
    /// Reads from the live in-memory registry — no disk I/O, safe to call each iteration.
    async fn build_skill_tools(&self) -> Vec<serde_json::Value> {
        use crate::mcp::orchestrator::sanitize_tool_name;
        self.skill_executor
            .registry
            .enabled_skills_for_tools()
            .await
            .into_iter()
            .map(|(name, description, triggers)| {
                let desc = if triggers.is_empty() {
                    description
                } else {
                    format!("{description}（触发词: {}）", triggers.join(", "))
                };
                let safe_name = sanitize_tool_name(&name);
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": format!("skill__{safe_name}"),
                        "description": desc,
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "input": {
                                    "type": "string",
                                    "description": "用户的请求或任务描述，原文传入"
                                }
                            },
                            "required": ["input"]
                        }
                    }
                })
            })
            .collect()
    }

    async fn run_agent_with_tools(
        &self,
        agent_name: &str,
        messages: Vec<serde_json::Value>,
        tool_perms: &PupToolPermissions,
        on_token: impl Fn(String) + Send + Sync,
        on_activity: impl Fn(String, String) + Send + Sync,
        abort: &AbortFlag,
    ) -> Result<String> {
        let primitive_perms = ToolPermissions {
            shell: tool_perms.shell,
            filesystem: tool_perms.filesystem,
            network: tool_perms.network,
        };

        let mut available_tools = self.skill_executor.tools.available_tools(&primitive_perms);

        // Always expose task_update so the LLM can mark tasks done/in_progress
        available_tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "task_update",
                "description": "Update the status of a task. Use this when you start working on a task (set in_progress) or complete it (set done). Valid statuses: pending, in_progress, done, failed.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Task ID" },
                        "status": { "type": "string", "enum": ["pending", "in_progress", "done", "failed"], "description": "New status" },
                        "result": { "type": "string", "description": "Optional result summary" }
                    },
                    "required": ["id", "status"]
                }
            }
        }));

        if tool_perms.mcp {
            // Extract the task from the last user message to drive tool selection
            let task_hint = messages
                .iter()
                .rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .unwrap_or("");
            let mcp_specs = self
                .mcp_orchestrator
                .tools_for_task(task_hint, Self::MAX_MCP_TOOLS)
                .await;
            debug!(
                "[{agent_name}] injecting {} MCP tools (filtered)",
                mcp_specs.len()
            );
            available_tools.extend(mcp_specs);
        }

        let mut msgs = messages;
        const MAX_ITER: usize = 12;

        for iter in 0..MAX_ITER {
            if abort.load(Ordering::Relaxed) {
                debug!("[{agent_name}] aborted at iteration {iter}");
                return Ok(String::new());
            }

            // Rebuild skill tools each iteration so newly installed skills are visible immediately.
            let mut iter_tools = available_tools.clone();
            iter_tools.extend(self.build_skill_tools().await);

            let response = match self
                .llm_client
                .chat_with_tools_abortable(msgs.clone(), iter_tools, abort)
                .await?
            {
                Some(r) => r,
                None => {
                    debug!("[{agent_name}] aborted during LLM call");
                    return Ok(String::new());
                }
            };

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                debug!("[{agent_name}] final answer: {} chars", text.len());
                on_token(text.clone());
                return Ok(text);
            }

            // Execute each tool call and feed results back
            msgs.push(response.raw_message);
            for tc in &response.tool_calls {
                debug!("[{agent_name}] tool_call: {}", tc.name);

                // Emit a specific activity kind + human-readable label for each tool type
                let (act_kind, act_label) = describe_tool_call(&tc.name, &tc.arguments);
                on_activity(act_kind, act_label);

                let result = if tc.name == "task_update" {
                    let id = tc.arguments["id"].as_str().unwrap_or_default();
                    let status = tc.arguments["status"].as_str().unwrap_or("done");
                    let result_text = tc.arguments["result"].as_str();
                    match self
                        .memory
                        .update_task_status(id, status, result_text)
                        .await
                    {
                        Ok(_) => format!("Task {id} updated to {status}."),
                        Err(e) => format!("task_update failed: {e}"),
                    }
                } else if let Some(safe_name) = tc.name.strip_prefix("skill__") {
                    // LLM explicitly called an installed skill as a tool.
                    // safe_name is sanitized; resolve to the original skill name from the registry.
                    use crate::mcp::orchestrator::sanitize_tool_name;
                    let skill_name = self
                        .skill_executor
                        .registry
                        .enabled_skills_for_tools()
                        .await
                        .into_iter()
                        .find(|(n, _, _)| sanitize_tool_name(n) == safe_name)
                        .map(|(n, _, _)| n)
                        .unwrap_or_else(|| safe_name.to_string());
                    let input = tc.arguments["input"].as_str().unwrap_or("");
                    let run_id = Uuid::new_v4().to_string();
                    let _ = self
                        .memory
                        .record_skill_run(&run_id, &skill_name, agent_name)
                        .await;
                    let skill_result = self
                        .skill_executor
                        .execute_skill_stream(
                            &skill_name,
                            input,
                            Arc::new(|_, _| {}), // output is returned as tool result, not streamed
                            Arc::new(|_, _| {}),
                            abort.clone(),
                        )
                        .await;
                    let (status, output) = match skill_result {
                        Ok(o) => ("completed".to_string(), o),
                        Err(e) => ("failed".to_string(), format!("Skill error: {e}")),
                    };
                    let _ = self
                        .memory
                        .complete_skill_run(&run_id, &status, &output)
                        .await;
                    output
                } else if tc.name.starts_with("mcp__") {
                    match self.mcp_orchestrator.resolve_fn_name(&tc.name).await {
                        Some((server, tool)) => self
                            .mcp_orchestrator
                            .call_tool(&server, &tool, &tc.arguments)
                            .await
                            .map(|v| v.to_string())
                            .unwrap_or_else(|e| format!("MCP error: {e}")),
                        None => format!("Unknown MCP tool: '{}'", tc.name),
                    }
                } else {
                    self.skill_executor
                        .tools
                        .execute(&tc.name, &tc.arguments, &primitive_perms)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"))
                };

                debug!("[{agent_name}] {} → {} chars", tc.name, result.len());
                msgs.push(serde_json::json!({
                  "role": "tool",
                  "tool_call_id": tc.id,
                  "content": result,
                }));

                // After any file operation, refresh skill dirs so LLM-written skills are live.
                if tc.name == "file_write" || tc.name == "shell_exec" {
                    self.skill_executor.registry.refresh().await;
                }

                // Check abort after each tool so we don't continue a long tool chain
                if abort.load(Ordering::Relaxed) {
                    debug!("[{agent_name}] aborted after tool '{}'", tc.name);
                    return Ok(String::new());
                }
            }
        }

        Err(anyhow!(
            "[{agent_name}] exceeded maximum tool-call iterations ({MAX_ITER})"
        ))
    }

    // ── Parallel pack dispatch ────────────────────────────────────────────────

    /// Run multiple pups in parallel against the same task and aggregate results.
    /// This is the current "parallel fan-out" mode — not true Pack Channel (which
    /// requires an inter-pup message bus and is not yet implemented).
    async fn run_parallel_pack(
        &self,
        msg: &str,
        required_pups: Vec<String>,
        app_handle: &tauri::AppHandle,
    ) -> Result<String> {
        let pup_list = required_pups
            .iter()
            .map(|k| pup_display_name(k))
            .collect::<Vec<_>>()
            .join("、");
        debug!("[alpha] parallel_pack: pups={required_pups:?}");
        let _ = app_handle.emit(
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: format!("pack:{}", required_pups.join(",")),
            },
        );

        let owner_summary = self
            .get_owner_summary(&self.file_layer.read_owner_profile().unwrap_or_default())
            .await;

        let mut join_handles = Vec::new();
        for pup_key in &required_pups {
            let self_clone = self.clone();
            let msg_owned = msg.to_string();
            let pup_key_owned = pup_key.clone();
            let owner_ctx = owner_summary.clone();
            let handle = tauri::async_runtime::spawn(async move {
                let result = self_clone
                    .run_pup_for_channel(&pup_key_owned, &msg_owned, &owner_ctx)
                    .await
                    .unwrap_or_else(|e| format!("Error: {e}"));
                (pup_key_owned, result)
            });
            join_handles.push(handle);
        }

        let joined = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            futures_util::future::join_all(join_handles),
        )
        .await;

        let pup_outputs: Vec<(String, String)> = match joined {
            Ok(results) => results.into_iter().filter_map(|r| r.ok()).collect(),
            Err(_) => {
                debug!("[alpha] parallel_pack: timed out");
                vec![]
            }
        };

        let _ = app_handle.emit(
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: format!("pack:{pup_list} → 汇总"),
            },
        );

        self.aggregate_channel_results(msg, &pup_outputs).await
    }

    /// Run a specialist pup's tool-call loop for a channel task (no streaming to chat).
    async fn run_pup_for_channel(
        &self,
        pup_key: &str,
        msg: &str,
        owner_summary: &str,
    ) -> Result<String> {
        let pup = self.specialist_registry.read().await.get(pup_key).cloned();
        let Some(pup) = pup else {
            return Ok(format!("Pup '{pup_key}' not available in registry."));
        };

        let override_prompt = {
            let cfgs = self.pup_configs.read().await;
            cfgs.get(pup_key).map(|c| c.system_prompt_override.clone())
        };

        let task = Task {
            id: Uuid::new_v4().to_string(),
            intent: msg.to_string(),
            context: vec![],
            owner_context: owner_summary.to_string(),
            relevant_memories: vec![],
            system_prompt_override: override_prompt.filter(|s| !s.is_empty()),
            assigned_pup: Some(pup_key.to_string()),
            status: TaskStatus::Pending,
        };

        let system_prompt = pup.build_system_prompt(&task);
        let tool_perms = pup.tool_permissions();
        let msgs = vec![
            serde_json::json!({ "role": "system", "content": system_prompt }),
            serde_json::json!({ "role": "user", "content": msg }),
        ];

        self.run_agent_with_tools(
            pup_key,
            msgs,
            &tool_perms,
            |_tok| {}, // channel pups don't stream to chat
            |_kind, _label| {},
            &self.abort_flag,
        )
        .await
    }

    /// LLM-based aggregation of multiple pup outputs into a final user-facing reply.
    async fn aggregate_channel_results(
        &self,
        original_msg: &str,
        results: &[(String, String)],
    ) -> Result<String> {
        if results.is_empty() {
            return Ok("Pack Channel 协作超时，未收到 Pup 结果。".to_string());
        }

        let results_text = results
            .iter()
            .map(|(pup, output)| {
                let name = pup_display_name(pup);
                let capped: String = output.chars().take(2000).collect();
                format!("=== {name} 的输出 ===\n{capped}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            "用户的原始请求：{original_msg}\n\n各 Pup 协作完成后的输出如下：\n\n{results_text}\n\n\
       请整合以上内容，生成一个连贯、完整的最终回复给用户。保持原有内容，适当组织结构。"
        );

        self.llm_client
            .chat(vec![
                LlmMessage {
                    role: "system".into(),
                    content: "你是 Alpha Pup，负责整合多 Pup 协作成果并输出清晰的最终回复。".into(),
                },
                LlmMessage {
                    role: "user".into(),
                    content: prompt,
                },
            ])
            .await
    }

    // ── Intent classification ──────────────────────────────────────────────────

    async fn classify_intent(
        &self,
        msg: &str,
        owner_summary: &str,
        history: &[LlmMessage],
    ) -> String {
        let trimmed = msg.trim();
        if trimmed.len() < 8 {
            debug!("[alpha] classify_intent: short msg → alpha");
            return "alpha".to_string();
        }

        let enabled_pups: Vec<String> = {
            let cfgs = self.pup_configs.read().await;
            cfgs.values()
                .filter(|c| c.enabled)
                .map(|c| c.key.clone())
                .collect()
        };
        let skill_entries = self
            .skill_executor
            .registry
            .enabled_skill_names_and_triggers()
            .await;

        let pup_options: String = enabled_pups.iter().map(|k| format!(" | {k}")).collect();
        let skill_options: String = skill_entries
            .iter()
            .map(|(n, _)| format!(" | skill:{n}"))
            .collect();
        let skill_lines: Vec<String> = skill_entries
            .iter()
            .map(|(name, triggers)| {
                if triggers.is_empty() {
                    format!("  - skill:{name}")
                } else {
                    format!("  - skill:{name} → {}", triggers.join(", "))
                }
            })
            .collect();
        let skills_block = if skill_lines.is_empty() {
            String::new()
        } else {
            format!("\nInstalled skills:\n{}", skill_lines.join("\n"))
        };

        let snippet = if owner_summary.len() > 400 {
            &owner_summary[..400]
        } else {
            owner_summary
        };
        let pup_hints: String = {
            let cfgs = self.pup_configs.read().await;
            enabled_pups
                .iter()
                .filter_map(|k| cfgs.get(k))
                .map(|c| format!("  - {} → {}", c.key, c.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let channel_hint = if enabled_pups.len() >= 2 {
            format!(
        "\n- channel:<pup1>,<pup2> → 任务同时需要多个专业 pup 并行协作完成（输出一个 token，例如 channel:research,writer）\
         \n  适用场景举例：\
         \n    · 「调研 XX 并写成报告」→ channel:research,writer\
         \n    · 「分析财报数据并写摘要」→ channel:finance,writer\
         \n    · 「写一个爬虫脚本并附使用文档」→ channel:dev,writer\
         \n  pup 列表（只能用已有 key）：{}\
         \n  注意：channel 本身是一个 token，不含空格。",
        enabled_pups.join(", ")
      )
        } else {
            String::new()
        };

        let system_prompt = format!(
            "Owner profile (excerpt):\n{snippet}\n\n\
       你是任务路由器。根据用户消息，输出以下选项之一（单个 token，无多余内容）：\
       \n- alpha → 闲聊、问答、或其他\
       \n{pup_hints}\n\
       {skills_block}{channel_hint}\n\
       直接输出 token，不要解释。"
        );

        let mut classifier_msgs = vec![LlmMessage {
            role: "system".into(),
            content: system_prompt,
        }];
        if let Some(last) = history.last() {
            classifier_msgs.push(last.clone());
        }
        classifier_msgs.push(LlmMessage {
            role: "user".into(),
            content: format!("Message to classify: \"{msg}\""),
        });

        let raw = match self.llm_client.chat_mini(classifier_msgs).await {
            Ok(r) => r,
            Err(e) => {
                debug!("[alpha] classify_intent chat_mini error: {e}");
                return "alpha".to_string();
            }
        };
        let key = raw.trim().to_lowercase();
        let key = key.split_whitespace().next().unwrap_or("alpha");
        debug!("[alpha] classify_intent: raw={raw:?} → key={key:?}");

        if key == "alpha" || enabled_pups.iter().any(|p| p == key) {
            return key.to_string();
        }
        if key.starts_with("skill:") {
            let skill_name = &key["skill:".len()..];
            if skill_entries.iter().any(|(n, _)| n == skill_name) {
                return key.to_string();
            }
        }
        if key.starts_with("channel:") {
            let pups_str = &key["channel:".len()..];
            let valid_pups: Vec<String> = pups_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|p| enabled_pups.contains(p))
                .collect();
            if valid_pups.len() >= 2 {
                let canonical = format!("channel:{}", valid_pups.join(","));
                debug!("[alpha] classify_intent: multi-pup channel → {canonical}");
                return canonical;
            }
        }
        debug!("[alpha] classify_intent: unrecognised key {key:?} → alpha");
        "alpha".to_string()
    }

    // ── Owner summary cache ────────────────────────────────────────────────────

    async fn get_owner_summary(&self, owner_md: &str) -> String {
        {
            let guard = self.owner_summary_cache.read().await;
            if let Some((ref cached, ref instant)) = *guard {
                if instant.elapsed().as_secs() < 300 {
                    return cached.clone();
                }
            }
        }
        let summary = if owner_md.len() <= 800 {
            owner_md.to_string()
        } else {
            let prompt = format!(
        "Summarize this OWNER.md in under 150 words, keeping: name, key boundaries/forbidden actions, \
         language preference, work schedule, top pain points.\n\n{owner_md}"
      );
            match self
                .llm_client
                .chat_mini(vec![LlmMessage {
                    role: "user".into(),
                    content: prompt,
                }])
                .await
            {
                Ok(s) => s,
                Err(_) => format!("{}…", &owner_md[..800]),
            }
        };
        {
            let mut guard = self.owner_summary_cache.write().await;
            *guard = Some((summary.clone(), std::time::Instant::now()));
        }
        summary
    }

    // ── History & memory extraction ────────────────────────────────────────────

    /// How many recent turns to keep verbatim (each turn = 2 rows: user + assistant).
    const VERBATIM_ROWS: i64 = 20;
    /// Compress when uncovered rows exceed this threshold.
    const COMPRESS_THRESHOLD: i64 = 40;

    /// Per-pup history: rolling summary + last VERBATIM_ROWS turns for this pup only.
    async fn build_history(&self, pup: &str) -> Vec<LlmMessage> {
        let mut msgs: Vec<LlmMessage> = Vec::new();

        // Prepend this pup's rolling compressed summary
        if let Ok(Some((summary, _))) = self.memory.get_context_summary(pup).await {
            msgs.push(LlmMessage {
                role: "system".into(),
                content: format!("## Earlier conversation summary\n{summary}"),
            });
        }

        // Then append the most recent verbatim turns for this pup
        let recent = self
            .memory
            .recent_conversations(pup, Self::VERBATIM_ROWS)
            .await
            .unwrap_or_default();
        msgs.extend(
            recent
                .into_iter()
                .rev()
                .map(|(role, content)| LlmMessage { role, content }),
        );
        msgs
    }

    /// Brief global history (last 4 turns across all pups) — used only for intent classification.
    async fn build_classify_history(&self) -> Vec<LlmMessage> {
        let recent = self
            .memory
            .recent_conversations_global(4)
            .await
            .unwrap_or_default();
        recent
            .into_iter()
            .rev()
            .map(|(role, content)| LlmMessage { role, content })
            .collect()
    }

    /// Compress older conversation history into a rolling summary.
    ///
    /// Called in the background every 10 exchanges.  Reads all rows NOT yet
    /// covered by the existing summary (excluding the latest VERBATIM_ROWS so
    /// those stay verbatim), summarises them with the LLM, and persists the
    /// result.  No-ops if there aren't enough new rows to warrant compression.
    async fn maybe_compress_context(&self, pup: &str) -> Result<()> {
        let max_row = self.memory.max_conversation_row(pup).await?;
        let (existing_summary, covers_through) = self
            .memory
            .get_context_summary(pup)
            .await?
            .unwrap_or_default();

        // Rows available for compression (everything except the last VERBATIM_ROWS)
        let compressible_ceiling = max_row - Self::VERBATIM_ROWS;
        let uncovered_rows = compressible_ceiling - covers_through;

        if uncovered_rows < Self::COMPRESS_THRESHOLD {
            return Ok(()); // Not enough new content to compress
        }

        debug!(
      "[{pup}] compress_context: {uncovered_rows} uncovered rows (covers_through={covers_through} max={max_row})"
    );

        // Load the uncovered rows (up to 200 to cap LLM input)
        let rows = self
            .memory
            .conversations_after_row(pup, covers_through, 200)
            .await?
            .into_iter()
            .filter(|(id, _, _)| *id <= compressible_ceiling)
            .collect::<Vec<_>>();

        if rows.is_empty() {
            return Ok(());
        }

        let last_row_id = rows.last().map(|(id, _, _)| *id).unwrap_or(covers_through);

        let transcript = rows
            .iter()
            .map(|(_, role, content)| {
                let snippet: String = content.chars().take(300).collect();
                format!("{role}: {snippet}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prior_context = if existing_summary.is_empty() {
            String::new()
        } else {
            format!("Prior summary:\n{existing_summary}\n\nNew exchanges to merge:\n")
        };

        let prompt = format!(
            "{prior_context}{transcript}\n\n\
       Produce a concise summary (≤300 words) of the above conversation exchanges. \
       Preserve: key facts, decisions, user preferences, ongoing tasks, and any commitments made. \
       Write in third-person neutral style."
        );

        let new_summary = self
            .llm_client
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await?;

        self.memory
            .save_context_summary(pup, &new_summary, last_row_id)
            .await?;
        debug!("[{pup}] compress_context: saved summary covering through row {last_row_id}");
        Ok(())
    }

    async fn maybe_extract_memories(&self, pup: &str) -> Result<()> {
        let recent = self.memory.recent_conversations(pup, 10).await?;
        if recent.is_empty() {
            return Ok(());
        }
        let transcript = recent
            .into_iter()
            .rev()
            .map(|(role, content)| format!("{role}: {content}"))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "从下面的对话中，提取 0-3 条对用户长期有用的事实/偏好/规则，\
       用 JSON 数组返回，每条形如 \
       {{\"type\": \"fact|preference|rule\", \"text\": \"...\", \"importance\": 0.0-1.0}}。\
       没有值得提取的内容时返回空数组 []。只返回 JSON：\n\n{transcript}"
        );

        let answer = self
            .llm_client
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await?;

        let json_str = answer
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let mut diary_entries: Vec<String> = Vec::new();
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(json_str) {
            for item in items {
                if let (Some(text), Some(mem_type)) = (
                    item.get("text").and_then(|v| v.as_str()),
                    item.get("type").and_then(|v| v.as_str()),
                ) {
                    let importance = item
                        .get("importance")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.7) as f32;
                    if self.memory.has_similar_memory(text, 0.88).await {
                        continue;
                    }
                    let _ = self
                        .memory
                        .add_long_term_memory(text, mem_type, importance)
                        .await;
                    diary_entries.push(format!("[{mem_type}] {text}"));
                }
            }
        }
        let _ = self.file_layer.append_daily_diary(&diary_entries);
        Ok(())
    }

    // ── Auto task creation ────────────────────────────────────────────────────────

    async fn maybe_create_task(&self, msg: &str, pup_key: &str) {
        // Only fire for specialist pups or clearly task-like messages
        let is_specialist = pup_key != "alpha" && !pup_key.starts_with("skill:");
        let looks_like_task = [
            "帮我", "帮助", "创建", "安排", "提醒", "记得", "todo", "task", "remind", "create",
            "schedule", "help me",
        ]
        .iter()
        .any(|kw| msg.to_lowercase().contains(kw));
        if !is_specialist && !looks_like_task {
            return;
        }

        let prompt = format!(
            "判断这条用户消息是否应该作为任务追踪。\
       如果是明确的可执行任务（如'帮我做X'、'创建Y'、'安排Z'），\
       返回 JSON: {{\"create\":true,\"description\":\"<简洁任务描述>\",\"pup\":\"{pup_key}\"}}\n\
       否则返回: {{\"create\":false}}\n\
       只返回 JSON，不要其他内容。\n\nUser: {msg}"
        );

        let raw = match self
            .llm_client
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await
        {
            Ok(r) => r,
            Err(_) => return,
        };

        let json_str = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
            if v.get("create").and_then(|x| x.as_bool()).unwrap_or(false) {
                let desc = v.get("description").and_then(|x| x.as_str()).unwrap_or(msg);
                let pup = v.get("pup").and_then(|x| x.as_str()).unwrap_or(pup_key);
                let _ = self.memory.create_task(desc, Some(pup)).await;
            }
        }
    }

    // ── @mention routing helper ───────────────────────────────────────────────

    async fn extract_at_mention(
        msg: &str,
        pup_configs: &Arc<RwLock<HashMap<String, PupConfig>>>,
    ) -> Option<String> {
        let trimmed = msg.trim_start();
        if !trimmed.starts_with('@') {
            return None;
        }
        let rest = &trimmed[1..];
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let candidate = rest[..end].to_lowercase();
        let cfgs = pup_configs.read().await;
        if cfgs.get(&candidate).map(|c| c.enabled).unwrap_or(false) {
            Some(candidate)
        } else {
            None
        }
    }

    /// Manually trigger context compression for a pup (exposed for the Pack UI).
    /// No-ops if there is nothing compressible yet.
    pub async fn compress_pup_context_now(&self, pup: &str) -> Result<()> {
        self.maybe_compress_context(pup).await
    }

    // ── Pup management ────────────────────────────────────────────────────────

    pub async fn list_pup_configs(&self) -> Vec<PupConfig> {
        let guard = self.pup_configs.read().await;
        let mut v: Vec<PupConfig> = guard.values().cloned().collect();
        v.sort_by(|a, b| a.key.cmp(&b.key));
        v
    }

    pub async fn update_pup_config(
        &self,
        key: &str,
        system_prompt_override: String,
        enabled: bool,
    ) -> Result<()> {
        {
            let mut guard = self.pup_configs.write().await;
            if let Some(cfg) = guard.get_mut(key) {
                cfg.system_prompt_override = system_prompt_override;
                cfg.enabled = enabled;
            }
        }
        self.persist_pup_configs().await
    }

    pub async fn add_custom_pup(
        &self,
        key: String,
        display_name: String,
        description: String,
        system_prompt: String,
    ) -> Result<()> {
        let cfg = PupConfig {
            key: key.clone(),
            display_name: display_name.clone(),
            description,
            system_prompt_override: system_prompt.clone(),
            enabled: true,
            is_custom: true,
        };
        {
            let mut guard = self.pup_configs.write().await;
            guard.insert(key.clone(), cfg);
        }
        // Register a CustomPup instance in the specialist registry
        let pup: Arc<dyn SpecialistPup> = Arc::new(CustomPup {
            key,
            display_name,
            system_prompt,
        });
        self.register_pup(pup).await;
        self.persist_pup_configs().await
    }

    pub async fn remove_custom_pup(&self, key: &str) -> Result<()> {
        {
            let mut guard = self.pup_configs.write().await;
            if let Some(cfg) = guard.get(key) {
                if !cfg.is_custom {
                    return Err(anyhow::anyhow!("Cannot remove built-in pup '{key}'"));
                }
            }
            guard.remove(key);
        }
        self.specialist_registry.write().await.remove(key);
        self.persist_pup_configs().await
    }

    async fn persist_pup_configs(&self) -> Result<()> {
        let Some(ref path) = self.pup_config_path else {
            return Ok(());
        };
        let guard = self.pup_configs.read().await;
        let list: Vec<&PupConfig> = guard.values().collect();
        let text = serde_json::to_string_pretty(&list)?;
        drop(guard);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        Ok(())
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a (kind, label) pair for stream_activity based on the tool being called.
/// `pub` so executor.rs can reuse it.
/// kind maps to a specific icon in the frontend; label is a human-readable summary.
pub fn describe_tool_call(name: &str, args: &serde_json::Value) -> (String, String) {
    fn trunc(s: &str, max: usize) -> String {
        if s.chars().count() > max {
            format!("{}…", s.chars().take(max).collect::<String>())
        } else {
            s.to_string()
        }
    }

    match name {
        "shell_exec" => {
            let cmd = args["command"].as_str().unwrap_or("");
            ("shell".into(), format!("$ {}", trunc(cmd, 60)))
        }
        "file_read" => {
            let path = args["path"].as_str().unwrap_or("");
            ("file_read".into(), trunc(path, 60))
        }
        "file_write" => {
            let path = args["path"].as_str().unwrap_or("");
            ("file_write".into(), trunc(path, 60))
        }
        "http_get" => {
            let url = args["url"].as_str().unwrap_or("");
            // Show just host + path, not query params
            let short = url.split('?').next().unwrap_or(url);
            ("http".into(), trunc(short, 60))
        }
        "memory_search" => {
            let q = args["query"].as_str().unwrap_or("");
            ("memory".into(), trunc(q, 50))
        }
        "memory_store" => ("memory".into(), "保存记忆".into()),
        "task_update" => {
            let status = args["status"].as_str().unwrap_or("");
            ("task".into(), format!("→ {status}"))
        }
        _ if name.starts_with("skill__") => {
            let skill = name.strip_prefix("skill__").unwrap_or(name);
            let input = args["input"].as_str().unwrap_or("");
            ("skill".into(), format!("{skill}: {}", trunc(input, 40)))
        }
        _ if name.starts_with("mcp__") => {
            // mcp__server__tool → "[server] tool"
            let rest = name.strip_prefix("mcp__").unwrap_or(name);
            let label = if let Some((server, tool)) = rest.split_once("__") {
                format!("[{server}] {tool}")
            } else {
                rest.to_string()
            };
            ("mcp".into(), trunc(&label, 60))
        }
        other => ("tool_call".into(), other.to_string()),
    }
}

pub fn pup_display_name(key: &str) -> String {
    if let Some(skill_name) = key.strip_prefix("skill:") {
        return format!("⚡ {skill_name}");
    }
    match key {
        "alpha" => "Alpha".to_string(),
        "dev" => "Dev Pup".to_string(),
        "writer" => "Writer Pup".to_string(),
        "ops" => "Ops Pup".to_string(),
        "research" => "Research Pup".to_string(),
        "life_admin" => "Life Admin Pup".to_string(),
        other => {
            // Capitalise first letter for custom pups
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str() + " Pup",
            }
        }
    }
}
