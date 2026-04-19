use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use tracing::debug;

use crate::agents::context_builder::{ContextBuilder, PupContext};
use crate::agents::custom_pup::CustomPup;
use crate::agents::router::Router;
use crate::agents::specialist::{Message, PupToolPermissions, SpecialistPup, Task, TaskStatus};
use crate::channel::dag::build_execution_layers;
use crate::channel::manager::{ChannelManager, ReviewDecision};
use crate::channel::types::{DelegationPlan, Subtask};
use crate::llm::client::{AbortFlag, LlmClient, LlmMessage};
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::memory::compaction::CompactionEngine;
use crate::memory::extractor::MemoryExtractor;
use crate::memory::file_layer::FileLayer;
use crate::memory::injector::{MemoryBudget, MemoryInjector};
use crate::memory::retriever::MemoryRetriever;
use crate::memory::system::{MemorySystem, TaskRecord};
use crate::runtime::{emit_event, SharedEventSink};
use crate::skills::executor::SkillExecutor;
use crate::tools::primitive::ToolPermissions;

type BridgeProgressHook = Arc<dyn Fn(String) + Send + Sync>;

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
    /// Optional tool permission overrides. When set, these take priority over
    /// the pup's hardcoded `tool_permissions()` defaults.
    #[serde(default)]
    pub permissions: Option<PupPermissionConfig>,
}

/// Configurable tool permissions for a pup.
/// Each field is optional — `None` means "use the pup's built-in default".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PupPermissionConfig {
    pub shell: Option<bool>,
    pub sandbox_shell: Option<bool>,
    pub file_read: Option<bool>,
    pub file_write: Option<bool>,
    pub network: Option<bool>,
    pub mcp: Option<bool>,
}

impl PupPermissionConfig {
    /// Overlay config permissions on top of a pup's hardcoded defaults.
    /// `None` fields fall through to the default.
    pub fn merge_over(&self, base: PupToolPermissions) -> PupToolPermissions {
        PupToolPermissions {
            shell: self.shell.unwrap_or(base.shell),
            sandbox_shell: self.sandbox_shell.unwrap_or(base.sandbox_shell),
            file_read: self.file_read.unwrap_or(base.file_read),
            file_write: self.file_write.unwrap_or(base.file_write),
            network: self.network.unwrap_or(base.network),
            mcp: self.mcp.unwrap_or(base.mcp),
        }
    }
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
                permissions: None,
            },
        )
    })
    .collect()
}

// ─── Event payloads ───────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
struct StreamDonePayload {
    pup_key: String,
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

#[derive(Clone)]
struct LayerExecutionResult {
    pup_key: String,
    result: String,
    message_id: String,
    review_request: Option<ParsedReviewRequest>,
}

#[derive(Clone)]
struct ParsedReviewRequest {
    requester_pup: String,
    target_pup: Option<String>,
    summary: String,
    blocking: bool,
    suggested_action: Option<String>,
}

#[derive(Clone)]
struct ReviewToolContext {
    allowed_targets: Vec<String>,
}

fn build_downstream_review_contract(allowed_targets: &[String]) -> String {
    let targets = if allowed_targets.is_empty() {
        "none".to_string()
    } else {
        allowed_targets.join(", ")
    };
    format!(
        "## Downstream review contract\n\
You are executing a downstream task that depends on upstream outputs.\n\
- Your allowed review targets are: {targets}\n\
- If any upstream output is missing facts, failed to gather required data, is ambiguous, contradictory, stale, or otherwise unusable, you MUST call `request_review` before giving a final answer.\n\
- Do NOT package blocked inputs into a normal deliverable.\n\
- Do NOT write a polished summary of failure and pretend the task is complete.\n\
- Only continue normally when the upstream context is genuinely sufficient for your task.\n\
- Prefer `blocking=true` when the dependency problem means the workflow should pause for review."
    )
}

enum AgentRunResult {
    FinalText(String),
    ReviewRequest(ToolReviewRequest),
}

struct ToolReviewRequest {
    target_pup: Option<String>,
    summary: String,
    blocking: bool,
    suggested_action: Option<String>,
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
    /// Per-pup real context token counts (prompt_tokens from the last API call for each pup).
    per_pup_context_tokens: Arc<RwLock<HashMap<String, u64>>>,
    pup_config_path: Option<PathBuf>,
    msg_count: Arc<std::sync::atomic::AtomicU32>,
    pub channel_manager: Arc<ChannelManager>,
    layer_hook: Arc<RwLock<Option<Arc<dyn Fn(usize, Vec<String>) + Send + Sync>>>>,
    /// Whether to auto-ingest pup artifacts and conversation summaries to KB.
    pub kb_auto_ingest: Arc<AtomicBool>,
    /// v0.1.12 memory subsystems
    memory_injector: Arc<MemoryInjector>,
    memory_extractor: Arc<MemoryExtractor>,
    /// Multi-layer context compaction engine
    pub compaction_engine: Arc<CompactionEngine>,
    /// Intent classification and @mention routing.
    router: Arc<Router>,
    /// Context assembly: owner summary caching, per-pup history, memory injection.
    context_builder: Arc<ContextBuilder>,
    /// Current pup_to_pup delegation nesting depth (shared across concurrent calls).
    delegation_depth: Arc<std::sync::atomic::AtomicU8>,
}

impl AlphaPup {
    pub fn new(
        memory: Arc<MemorySystem>,
        llm_client: Arc<LlmClient>,
        mcp_orchestrator: Arc<MCPOrchestrator>,
        file_layer: Arc<FileLayer>,
        skill_executor: Arc<SkillExecutor>,
        pup_config_path: Option<PathBuf>,
        channel_manager: Arc<ChannelManager>,
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
        // v0.1.12: build retriever → injector → extractor from shared pool + llm
        let retriever = Arc::new(MemoryRetriever::new(
            memory.pool().clone(),
            llm_client.clone(),
        ));
        let injector = Arc::new(MemoryInjector::new(memory.pool().clone(), retriever));
        let extractor = Arc::new(MemoryExtractor::new(
            memory.pool().clone(),
            llm_client.clone(),
        ));
        let compaction_engine = Arc::new(CompactionEngine::new(
            memory.pool().clone(),
            llm_client.clone(),
            extractor.clone(),
        ));

        let pup_configs = Arc::new(RwLock::new(configs));

        let context_builder = Arc::new(ContextBuilder::new(
            memory.clone(),
            llm_client.clone(),
            injector.clone(),
        ));

        let router = Arc::new(Router {
            llm_client: llm_client.clone(),
            pup_configs: pup_configs.clone(),
            skill_executor: skill_executor.clone(),
            memory: memory.clone(),
        });

        Self {
            memory,
            specialist_registry: Arc::new(RwLock::new(HashMap::new())),
            llm_client,
            mcp_orchestrator,
            file_layer,
            skill_executor,
            abort_flag: Arc::new(AtomicBool::new(false)),
            pup_configs,
            per_pup_context_tokens: Arc::new(RwLock::new(HashMap::new())),
            pup_config_path,
            msg_count: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            channel_manager,
            layer_hook: Arc::new(RwLock::new(None)),
            kb_auto_ingest: Arc::new(AtomicBool::new(true)),
            memory_injector: injector,
            memory_extractor: extractor,
            compaction_engine,
            router,
            context_builder,
            delegation_depth: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        }
    }

    pub async fn set_layer_hook(
        &self,
        hook: Option<Arc<dyn Fn(usize, Vec<String>) + Send + Sync>>,
    ) {
        *self.layer_hook.write().await = hook;
    }

    fn emit_bridge_progress(progress_hook: &Option<BridgeProgressHook>, text: impl Into<String>) {
        if let Some(hook) = progress_hook {
            hook(text.into());
        }
    }

    pub async fn register_pup(&self, pup: Arc<dyn SpecialistPup>) {
        let mut guard = self.specialist_registry.write().await;
        guard.insert(pup.name().to_string(), pup);
    }

    async fn configured_pup(&self, key: &str) -> Option<PupConfig> {
        self.pup_configs.read().await.get(key).cloned()
    }

    async fn resolve_pup(&self, key: &str) -> Result<Arc<dyn SpecialistPup>> {
        if let Some(cfg) = self.configured_pup(key).await {
            if !cfg.enabled {
                return Err(anyhow!("Pup '{key}' is disabled."));
            }
            if cfg.is_custom {
                let pup: Arc<dyn SpecialistPup> = Arc::new(CustomPup {
                    key: cfg.key,
                    display_name: cfg.display_name,
                    system_prompt: cfg.system_prompt_override,
                });
                return Ok(pup);
            }
        }

        let pup = self.specialist_registry.read().await.get(key).cloned();
        if let Some(pup) = pup {
            return Ok(pup);
        }

        if self.configured_pup(key).await.is_some() {
            Err(anyhow!(
                "Pup '{key}' is configured but unavailable at runtime."
            ))
        } else {
            Err(anyhow!("Pup '{key}' not found."))
        }
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
        events: SharedEventSink,
    ) {
        debug!(
            "[alpha] process_user_message_stream: msg_len={} forced_pup={forced_pup:?}",
            msg.len()
        );
        self.abort_flag.store(false, Ordering::Relaxed);

        let result = self.do_stream(&msg, forced_pup, events.clone()).await;
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
                emit_event(
                    events.as_ref(),
                    "stream_done",
                    StreamDonePayload {
                        pup_key: pup_key.clone(),
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
                    let reply_clone = reply.clone();
                    tokio::spawn(async move {
                        let _ = self_clone
                            .post_process_conversation_turn(
                                &pup_key_clone,
                                &msg_clone,
                                &reply_clone,
                            )
                            .await;
                    });
                }
            }
            Err(e) => {
                debug!("[alpha] do_stream error: {e}");
                emit_event(events.as_ref(), "stream_error", e.to_string());
            }
        }
    }

    async fn do_stream(
        &self,
        msg: &str,
        forced_pup: Option<String>,
        events: SharedEventSink,
    ) -> Result<(String, String)> {
        let owner_md = self.file_layer.read_owner_profile().unwrap_or_default();
        let owner_summary = self.context_builder.get_owner_summary(&owner_md).await;
        // v0.1.12: hybrid retrieval with rule force-injection + Weibull decay
        let memory_context = self
            .memory_injector
            .build_memory_context(msg, &MemoryBudget::default())
            .await
            .unwrap_or_default();
        let memories_str = MemoryInjector::format_for_injection(&memory_context);
        let relevant_memories: Vec<String> = if memories_str.is_empty() {
            vec![]
        } else {
            vec![memories_str]
        };
        // Brief global history for intent classification (last 4 turns, all pups)
        let classify_history = self.router.build_classify_history().await;
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
        } else if let Some(mention) = self.router.extract_at_mention(msg).await {
            mention
        } else {
            self.router
                .classify_intent(msg, &owner_summary, &classify_history)
                .await
        };
        debug!("[alpha] do_stream: pup_key={pup_key:?}");

        // Notify the UI which pup is handling this request (now that we know).
        emit_event(
            events.as_ref(),
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
                let output = self.run_dag(msg, required_pups, events.clone()).await?;
                return Ok((output, pup_key));
            }
        }

        if let Some(skill_name) = pup_key.strip_prefix("skill:") {
            // Prompt injection: load the skill's prompt and inject it into
            // the system message, then run the normal tool-call loop with
            // alpha's tools so the LLM can follow the skill instructions.
            let (skill_prompt, skill_perms) =
                match self.skill_executor.load_skill_prompt(skill_name).await {
                    Ok(pair) => pair,
                    Err(e) => return Ok((format!("Skill error: {e}"), pup_key)),
                };

            let run_id = Uuid::new_v4().to_string();
            let _ = self
                .memory
                .record_skill_run(&run_id, skill_name, "conversation", None)
                .await;

            let system_prompt = format!(
                "{owner_summary}\n\n\
                 ## Active Skill: {skill_name}\n\n\
                 Follow the Claude-style skill bundle below to complete the user's request.\n\n\
                 {skill_prompt}"
            );

            let pup_history = self.context_builder.build_history().await;
            let mut msgs: Vec<serde_json::Value> =
                vec![serde_json::json!({ "role": "system", "content": system_prompt })];
            for m in &pup_history {
                msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
            }
            msgs.push(serde_json::json!({ "role": "user", "content": msg }));

            // Alpha baseline (no shell/fs/net, MCP only) unioned with skill
            // permissions — the skill can elevate but never restrict.
            let alpha_base = PupToolPermissions {
                shell: false,
                sandbox_shell: false,
                file_read: false,
                file_write: false,
                network: false,
                mcp: true,
            };
            let tool_perms = PupToolPermissions {
                shell: alpha_base.shell || skill_perms.shell,
                sandbox_shell: alpha_base.sandbox_shell || skill_perms.sandbox_shell,
                file_read: alpha_base.file_read || skill_perms.file_read,
                file_write: alpha_base.file_write || skill_perms.file_write,
                network: alpha_base.network || skill_perms.network,
                mcp: alpha_base.mcp || skill_perms.mcp,
            };

            let handle = events.clone();
            let handle2 = events.clone();
            let result = self
                .run_agent_with_tools(
                    &format!("skill:{skill_name}"),
                    msgs,
                    &tool_perms,
                    None,
                    move |tok| {
                        emit_event(handle.as_ref(), "stream_token", tok);
                    },
                    move |kind, label| {
                        emit_event(
                            handle2.as_ref(),
                            "stream_activity",
                            ActivityEvent { kind, label },
                        );
                    },
                    &self.abort_flag,
                )
                .await?;
            let output = match result {
                AgentRunResult::FinalText(text) => text,
                AgentRunResult::ReviewRequest(_) => {
                    "Error: review requests are not supported in skill mode.".to_string()
                }
            };

            let status = if output.is_empty() {
                "aborted"
            } else {
                "completed"
            };
            let _ = self
                .memory
                .complete_skill_run(&run_id, status, &output)
                .await;
            emit_event(
                events.as_ref(),
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
            let pup_history = self.context_builder.build_history().await;
            let reply = self
                .alpha_reply_stream(
                    msg,
                    &owner_summary,
                    &pup_history,
                    &relevant_memories,
                    &pending_tasks,
                    events.clone(),
                )
                .await?;
            self.record_pup_context_tokens_async("alpha").await;
            return Ok((reply, "alpha".to_string()));
        }

        // Route to specialist pup — build task context then run shared tool loop
        let override_prompt = self
            .configured_pup(&pup_key)
            .await
            .map(|c| c.system_prompt_override);
        if let Ok(pup) = self.resolve_pup(&pup_key).await {
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
            let pup_history = self.context_builder.build_history().await;
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
            let base_perms = pup.tool_permissions();
            let tool_perms = if let Some(cfg) = self.configured_pup(&pup_key).await {
                cfg.permissions
                    .map(|p| p.merge_over(base_perms.clone()))
                    .unwrap_or(base_perms)
            } else {
                base_perms
            };

            // Build message list as JSON values for chat_with_tools
            let mut msgs: Vec<serde_json::Value> =
                vec![serde_json::json!({ "role": "system", "content": system_prompt })];
            for m in &task.context {
                msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
            }
            msgs.push(serde_json::json!({ "role": "user", "content": task.intent }));

            let handle = events.clone();
            let handle2 = events.clone();
            let output = self
                .run_agent_with_tools(
                    &pup_key,
                    msgs,
                    &tool_perms,
                    None,
                    move |tok| {
                        emit_event(handle.as_ref(), "stream_token", tok);
                    },
                    move |kind, label| {
                        emit_event(
                            handle2.as_ref(),
                            "stream_activity",
                            ActivityEvent { kind, label },
                        );
                    },
                    &self.abort_flag,
                )
                .await?;
            let output = match output {
                AgentRunResult::FinalText(text) => text,
                AgentRunResult::ReviewRequest(_) => {
                    "Error: review requests are not supported in direct chat.".to_string()
                }
            };
            self.record_pup_context_tokens_async(&pup_key).await;
            return Ok((output, pup_key));
        }

        // Fallback to alpha
        let fallback_history = self.context_builder.build_history().await;
        let reply = self
            .alpha_reply_stream(
                msg,
                &owner_summary,
                &fallback_history,
                &relevant_memories,
                &pending_tasks,
                events.clone(),
            )
            .await?;
        self.record_pup_context_tokens_async("alpha").await;
        Ok((reply, "alpha".to_string()))
    }

    async fn alpha_reply_stream(
        &self,
        msg: &str,
        owner_summary: &str,
        history: &[LlmMessage],
        memories: &[String],
        pending_tasks: &[TaskRecord],
        events: SharedEventSink,
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
            // v0.1.12: memories may be pre-formatted blocks from MemoryInjector
            for m in memories {
                if m.contains("## ") {
                    // Pre-formatted injection block (rules + semantic)
                    system_content.push_str(&format!("\n\n{m}"));
                } else {
                    let capped: String = m.chars().take(200).collect();
                    system_content.push_str(&format!("\n- {capped}"));
                }
            }
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
            sandbox_shell: false,
            file_read: false,
            file_write: false,
            network: false,
            mcp: true,
        };

        let handle = events.clone();
        let handle3 = events.clone();
        self.run_agent_with_tools(
            "alpha",
            messages,
            &tool_perms,
            None,
            move |tok| {
                // Emit reasoning tokens separately if the LLM uses them.
                // For now all tokens from the tool loop go to stream_token.
                emit_event(handle.as_ref(), "stream_token", tok);
            },
            move |kind, label| {
                emit_event(
                    handle3.as_ref(),
                    "stream_activity",
                    ActivityEvent { kind, label },
                );
            },
            &self.abort_flag,
        )
        .await
        .map(|result| match result {
            AgentRunResult::FinalText(text) => text,
            AgentRunResult::ReviewRequest(_) => {
                "Error: review requests are not supported for Alpha chat.".to_string()
            }
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
    const MAX_MCP_TOOLS: usize = 30;

    /// Build the skill catalog: a single `activate_skill` tool whose description
    /// lists all enabled skills.  When the LLM calls `activate_skill(name)`,
    /// the skill's full prompt is loaded from disk and returned as the tool
    /// result — injecting it into the conversation context so the LLM can
    /// follow the instructions using the pup's existing tools.
    ///
    /// Returns (tool_list, registry_generation).
    async fn build_skill_catalog(&self) -> (Vec<serde_json::Value>, u64) {
        let gen = self.skill_executor.registry.generation();
        let enabled = self
            .skill_executor
            .registry
            .enabled_skills_for_tools()
            .await;

        if enabled.is_empty() {
            return (Vec::new(), gen);
        }

        let catalog_lines: Vec<String> = enabled
            .iter()
            .map(|(name, desc, triggers)| {
                if triggers.is_empty() {
                    format!("- {name}: {desc}")
                } else {
                    format!("- {name}: {desc}（触发词: {}）", triggers.join(", "))
                }
            })
            .collect();
        let catalog = catalog_lines.join("\n");
        let tool_desc = format!(
            "Activate a skill by name. The skill's instructions will be returned — \
             follow them using your available tools to complete the task.\n\n\
             Available skills ({} total):\n{catalog}",
            enabled.len()
        );

        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "activate_skill",
                "description": tool_desc,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Skill name from the catalog above"
                        }
                    },
                    "required": ["name"]
                }
            }
        })];
        (tools, gen)
    }

    /// Append the current local date/time to the first system message.
    fn inject_current_time(msgs: &mut [serde_json::Value]) {
        use chrono::Local;
        let now = Local::now();
        let weekday = match now.format("%u").to_string().as_str() {
            "1" => "周一",
            "2" => "周二",
            "3" => "周三",
            "4" => "周四",
            "5" => "周五",
            "6" => "周六",
            "7" => "周日",
            _ => "",
        };
        let tz = now.format("%Z").to_string();
        let time_line = format!(
            "\n\nCurrent time: {} ({weekday}) {tz}",
            now.format("%Y-%m-%d %H:%M"),
        );
        if let Some(sys) = msgs.first_mut() {
            if sys["role"] == "system" {
                if let Some(content) = sys["content"].as_str() {
                    sys["content"] = serde_json::Value::String(format!("{content}{time_line}"));
                }
            }
        }
    }

    /// Estimate the token count of the current context (messages + tools).
    /// Uses ~4 chars per token as a conservative heuristic.
    fn estimate_context_tokens(msgs: &[serde_json::Value], tools: &[serde_json::Value]) -> u64 {
        let msg_chars: usize = msgs
            .iter()
            .map(|m| serde_json::to_string(m).map(|s| s.len()).unwrap_or(0))
            .sum();
        let tool_chars: usize = tools
            .iter()
            .map(|t| serde_json::to_string(t).map(|s| s.len()).unwrap_or(0))
            .sum();
        ((msg_chars + tool_chars) / 4) as u64
    }

    /// Trim oldest non-system messages from the context to fit within the token budget.
    fn trim_context_to_budget(
        msgs: &mut Vec<serde_json::Value>,
        tools: &[serde_json::Value],
        limit: u64,
    ) {
        // Target 85% of limit to leave headroom for the response
        let budget = (limit as f64 * 0.85) as u64;
        while Self::estimate_context_tokens(msgs, tools) > budget && msgs.len() > 2 {
            // Find the first non-system message to remove (preserve system + last user)
            if let Some(idx) = msgs.iter().position(|m| m["role"] != "system") {
                if idx < msgs.len() - 1 {
                    msgs.remove(idx);
                    continue;
                }
            }
            break;
        }
    }

    /// Format a tool error as structured JSON so the LLM can reason about
    /// recoverability and decide whether to retry, use an alternative, or report.
    fn format_structured_error(tool: &str, message: &str, recoverable: bool) -> String {
        serde_json::json!({
            "error": true,
            "tool": tool,
            "message": message,
            "recoverable": recoverable,
            "hint": if recoverable {
                "You may retry with different arguments, try an alternative approach, or report the error to the user."
            } else {
                "This error is not recoverable. Inform the user and suggest an alternative."
            }
        }).to_string()
    }

    /// Compute the dynamic truncation limit for tool results, proportional to context window.
    /// Returns max chars = 30% of context window × 4 chars/token, clamped to [2_000, 32_768].
    fn tool_result_max_chars(&self) -> usize {
        let limit = self.get_context_limit();
        let max = ((limit as f64 * 0.30) * 4.0) as usize;
        max.clamp(2_000, 32_768)
    }

    /// Truncate a tool result using head+tail strategy so that error messages
    /// at the end (e.g. stderr) are preserved. Keeps ~70% head + ~30% tail.
    fn truncate_tool_result(&self, text: &str) -> String {
        let max = self.tool_result_max_chars();
        let count = text.chars().count();
        if count <= max {
            return text.to_string();
        }
        let tail_budget = max * 3 / 10; // 30% for tail
        let head_budget = max.saturating_sub(tail_budget).saturating_sub(80); // room for marker
        let head: String = text.chars().take(head_budget).collect();
        let tail: String = text.chars().skip(count - tail_budget).collect();
        let omitted = count - head_budget - tail_budget;
        format!("{head}\n\n… [truncated {omitted} chars of {count} total] …\n\n{tail}")
    }

    /// Maximum nesting depth for pup_to_pup delegation.
    const MAX_DELEGATION_DEPTH: u8 = 2;

    async fn run_agent_with_tools(
        &self,
        agent_name: &str,
        messages: Vec<serde_json::Value>,
        tool_perms: &PupToolPermissions,
        review_tool: Option<&ReviewToolContext>,
        on_token: impl Fn(String) + Send + Sync,
        on_activity: impl Fn(String, String) + Send + Sync,
        abort: &AbortFlag,
    ) -> Result<AgentRunResult> {
        let mut primitive_perms = ToolPermissions {
            shell: tool_perms.shell,
            sandbox_shell: tool_perms.sandbox_shell,
            file_read: tool_perms.file_read,
            file_write: tool_perms.file_write,
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

        if let Some(review_tool) = review_tool {
            let mut target_options = review_tool.allowed_targets.clone();
            target_options.sort();
            target_options.dedup();
            available_tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_review",
                    "description": "Required for downstream blocking cases. If an upstream dependency is missing, failed, ambiguous, contradictory, or unusable, call this instead of writing a normal final answer.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "target_pup": {
                                "type": ["string", "null"],
                                "description": format!("The upstream pup to review. Allowed targets: {}. Choose the specific dependency when possible. Use null only if the issue is with the overall layer.", if target_options.is_empty() { "none".to_string() } else { target_options.join(", ") }),
                            },
                            "summary": {
                                "type": "string",
                                "description": "A concise objection summary, max 120 Chinese chars or similar length. State exactly why the upstream output cannot be used."
                            },
                            "blocking": {
                                "type": "boolean",
                                "description": "Whether execution should pause for human review. Use true when you cannot complete your task reliably with the current upstream context."
                            },
                            "suggested_action": {
                                "type": ["string", "null"],
                                "description": "Optional short suggestion such as rerun_upstream, clarify_requirements, or continue_with_risk."
                            }
                        },
                        "required": ["summary", "blocking"]
                    }
                }
            }));
        }

        // pup_to_pup: allow this pup to delegate a sub-task to another pup (if depth allows)
        if self.delegation_depth.load(Ordering::Relaxed) < Self::MAX_DELEGATION_DEPTH {
            let other_pups: Vec<String> = {
                let registry = self.specialist_registry.read().await;
                let configs = self.pup_configs.read().await;
                // Collect registered pups that are not disabled in config
                let mut pups: std::collections::HashSet<String> = registry
                    .keys()
                    .filter(|k| k.as_str() != agent_name)
                    .filter(|k| configs.get(*k).map(|c| c.enabled).unwrap_or(true))
                    .cloned()
                    .collect();
                // Also include enabled custom pups from config (not in registry)
                for (key, cfg) in configs.iter() {
                    if cfg.enabled && cfg.is_custom && key != agent_name {
                        pups.insert(key.clone());
                    }
                }
                let mut pups: Vec<String> = pups.into_iter().collect();
                pups.sort();
                pups
            };
            if !other_pups.is_empty() {
                available_tools.push(serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": "pup_to_pup",
                        "description": format!(
                            "Delegate a sub-task to another pup and get the result back. \
                             Available pups: {}. The target pup can see the full shared \
                             conversation history. The task parameter is a focus hint.",
                            other_pups.join(", ")
                        ),
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "target_pup": {
                                    "type": "string",
                                    "description": format!("The pup to delegate to. One of: {}", other_pups.join(", ")),
                                    "enum": other_pups,
                                },
                                "task": {
                                    "type": "string",
                                    "description": "Focus hint for the target pup. The target pup has access to the full shared conversation history, so this only needs to specify what to focus on."
                                }
                            },
                            "required": ["target_pup", "task"]
                        }
                    }
                }));
            }
        }

        // Track whether we're using deferred MCP loading (single fetch_mcp_tool instead of all schemas)
        let mut mcp_deferred = false;
        if tool_perms.mcp {
            // Extract the task from the last user message to drive tool selection
            let task_hint = messages
                .iter()
                .rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .unwrap_or("");
            let all_mcp = self.mcp_orchestrator.tools_as_openai_specs().await;
            if all_mcp.len() > Self::MAX_MCP_TOOLS {
                // Deferred pattern: inject a lightweight catalog tool instead of all schemas.
                // Saves ~100-200 tokens per tool × (total - MAX_MCP_TOOLS) tools.
                let catalog = self
                    .mcp_orchestrator
                    .deferred_tool_catalog(task_hint, Self::MAX_MCP_TOOLS * 2)
                    .await;
                debug!(
                    "[{agent_name}] deferred MCP: {} total tools → catalog with {} entries",
                    all_mcp.len(),
                    Self::MAX_MCP_TOOLS * 2,
                );
                available_tools.extend(catalog);
                mcp_deferred = true;
            } else {
                let mcp_specs = self
                    .mcp_orchestrator
                    .tools_for_task(task_hint, Self::MAX_MCP_TOOLS)
                    .await;
                debug!(
                    "[{agent_name}] injecting {} MCP tools (all fit)",
                    mcp_specs.len()
                );
                available_tools.extend(mcp_specs);
            }
        }

        let mut msgs = messages;
        const MAX_ITER: usize = 20;
        let context_limit = self.get_context_limit();

        // ── Inject current time into system message ──
        Self::inject_current_time(&mut msgs);

        // ── Skill catalog: single `activate_skill` tool listing all enabled skills ──
        let (mut cached_skill_tools, mut cached_skill_gen) = self.build_skill_catalog().await;

        for iter in 0..MAX_ITER {
            if abort.load(Ordering::Relaxed) {
                debug!("[{agent_name}] aborted at iteration {iter}");
                return Ok(AgentRunResult::FinalText(String::new()));
            }

            // Rebuild skill catalog only when the registry has changed.
            let current_gen = self.skill_executor.registry.generation();
            if current_gen != cached_skill_gen {
                let (new_tools, new_gen) = self.build_skill_catalog().await;
                cached_skill_tools = new_tools;
                cached_skill_gen = new_gen;
                debug!(
                    "[{agent_name}] skill catalog rebuilt (gen {cached_skill_gen} → {current_gen})"
                );
            }

            let mut iter_tools = available_tools.clone();
            let skill_tool_count = cached_skill_tools.len();
            iter_tools.extend(cached_skill_tools.clone());

            // ── Priority 1: Context window guard ──
            // Estimate tokens and trim if approaching the limit.
            let estimated_tokens = Self::estimate_context_tokens(&msgs, &iter_tools);
            if estimated_tokens > (context_limit as f64 * 0.85) as u64 {
                debug!(
                    "[{agent_name}] context guard: {estimated_tokens} tokens exceeds 85% of {context_limit}, trimming"
                );
                Self::trim_context_to_budget(&mut msgs, &iter_tools, context_limit);
            }

            let msg_chars = msgs
                .iter()
                .map(|m| serde_json::to_string(m).map(|s| s.len()).unwrap_or(0))
                .sum::<usize>();
            let tool_chars = iter_tools
                .iter()
                .map(|t| serde_json::to_string(t).map(|s| s.len()).unwrap_or(0))
                .sum::<usize>();
            let tool_count = iter_tools.len();
            let non_skill_tool_count = tool_count.saturating_sub(skill_tool_count);
            debug!(
                    "[{agent_name}] context(iter={iter}): messages={} chars={} tools={} tool_chars={} (base={} skill={}) est_tokens={} limit={}",
                    msgs.len(),
                    msg_chars,
                    tool_count,
                    tool_chars,
                    non_skill_tool_count,
                    skill_tool_count,
                    estimated_tokens,
                    context_limit,
                );

            let response = match self
                .llm_client
                .chat_with_tools_stream(
                    msgs.clone(),
                    iter_tools,
                    |tok| on_token(tok.to_string()),
                    abort,
                )
                .await?
            {
                Some(r) => r,
                None => {
                    debug!("[{agent_name}] aborted during LLM call");
                    return Ok(AgentRunResult::FinalText(String::new()));
                }
            };

            if response.tool_calls.is_empty() {
                let text = response.content.unwrap_or_default();
                debug!("[{agent_name}] final answer: {} chars", text.len());
                // Tokens were already streamed via on_token during generation
                return Ok(AgentRunResult::FinalText(text));
            }

            // Execute each tool call and feed results back
            msgs.push(response.raw_message);
            for tc in &response.tool_calls {
                debug!("[{agent_name}] tool_call: {}", tc.name);

                // Emit a specific activity kind + human-readable label for each tool type
                let (act_kind, act_label) = describe_tool_call(&tc.name, &tc.arguments);
                on_activity(act_kind, act_label);

                let result = if tc.name == "request_review" {
                    let target_pup = tc
                        .arguments
                        .get("target_pup")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| value.trim().to_string());
                    if let Some(review_tool) = review_tool {
                        if let Some(target) = target_pup.as_ref() {
                            if !review_tool
                                .allowed_targets
                                .iter()
                                .any(|allowed| allowed == target)
                            {
                                return Ok(AgentRunResult::FinalText(format!(
                                    "Error: invalid review target '{}'",
                                    target
                                )));
                            }
                        }
                    }
                    let summary = tc
                        .arguments
                        .get("summary")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(120)
                        .collect::<String>();
                    let blocking = tc
                        .arguments
                        .get("blocking")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true);
                    let suggested_action = tc
                        .arguments
                        .get("suggested_action")
                        .and_then(|value| value.as_str())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    on_activity(
                        "review".into(),
                        format!(
                            "request_review → {}",
                            target_pup.clone().unwrap_or_else(|| "layer".to_string())
                        ),
                    );
                    return Ok(AgentRunResult::ReviewRequest(ToolReviewRequest {
                        target_pup,
                        summary,
                        blocking,
                        suggested_action,
                    }));
                } else if tc.name == "task_update" {
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
                } else if tc.name == "pup_to_pup" {
                    let target = tc.arguments["target_pup"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let sub_task = tc.arguments["task"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if target.is_empty() || sub_task.is_empty() {
                        "Error: both target_pup and task are required".to_string()
                    } else {
                        let current_depth = self.delegation_depth.load(Ordering::Relaxed);
                        if current_depth >= Self::MAX_DELEGATION_DEPTH {
                            format!(
                                "Error: delegation depth limit ({}) reached — cannot delegate further",
                                Self::MAX_DELEGATION_DEPTH
                            )
                        } else {
                            on_activity("delegation".into(), format!("{agent_name} → {target}"));
                            let owner_md = self.file_layer.read_owner_profile().unwrap_or_default();
                            let owner_ctx = self.context_builder.get_owner_summary(&owner_md).await;
                            self.delegation_depth.fetch_add(1, Ordering::Relaxed);
                            let result = self
                                .run_pup_for_channel(
                                    &target,
                                    &sub_task,
                                    &owner_ctx,
                                    &on_activity,
                                    None,
                                )
                                .await;
                            self.delegation_depth.fetch_sub(1, Ordering::Relaxed);
                            match result {
                                Ok(text) => format!("[{target} 回复]\n{text}"),
                                Err(e) => format!("[{target}] Error: {e}"),
                            }
                        }
                    }
                } else if tc.name == "activate_skill" {
                    // Prompt injection: load the skill's full prompt and return
                    // it as the tool result.  The LLM reads these instructions
                    // and follows them using the pup's existing tools.
                    let skill_name = tc.arguments["name"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if skill_name.is_empty() {
                        "Error: missing skill name".to_string()
                    } else {
                        let run_id = Uuid::new_v4().to_string();
                        let _ = self
                            .memory
                            .record_skill_run(&run_id, &skill_name, agent_name, None)
                            .await;
                        match self.skill_executor.load_skill_prompt(&skill_name).await {
                            Ok((prompt, skill_perms)) => {
                                let _ = self
                                    .memory
                                    .complete_skill_run(&run_id, "activated", "")
                                    .await;
                                // Union pup + skill permissions so the skill
                                // gets the tools it needs.  Update primitive_perms
                                // so subsequent tool *execution* also uses the
                                // elevated permissions, not just the tool list.
                                primitive_perms = primitive_perms.union_with_skill(&skill_perms);
                                let full_tools =
                                    self.skill_executor.tools.available_tools(&primitive_perms);
                                for t in full_tools {
                                    let t_name = t["function"]["name"].as_str().unwrap_or("");
                                    if !available_tools.iter().any(|existing| {
                                        existing["function"]["name"].as_str() == Some(t_name)
                                    }) {
                                        available_tools.push(t);
                                    }
                                }
                                format!(
                                    "## Skill '{skill_name}' activated\n\n\
                                     Follow the Claude-style skill bundle below to complete the task.\n\n\
                                     {prompt}"
                                )
                            }
                            Err(e) => {
                                let _ = self
                                    .memory
                                    .complete_skill_run(&run_id, "failed", &e.to_string())
                                    .await;
                                format!("Error loading skill '{skill_name}': {e}")
                            }
                        }
                    }
                } else if tc.name == "fetch_mcp_tool" && mcp_deferred {
                    // Deferred tool pattern: LLM requested the full schema for an MCP tool.
                    // Return the schema and also inject it into available_tools for the next iteration.
                    let requested = tc.arguments["tool_name"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    match self.mcp_orchestrator.deferred_tool_schema(&requested).await {
                        Ok(schema) => {
                            // Inject the full tool so the LLM can call it in subsequent iterations
                            if !available_tools
                                .iter()
                                .any(|t| t["function"]["name"].as_str() == Some(&requested))
                            {
                                available_tools.push(schema.clone());
                            }
                            format!(
                                "Tool schema loaded. You can now call `{requested}` directly with the following parameters:\n{}",
                                serde_json::to_string_pretty(&schema["function"]["parameters"]).unwrap_or_default()
                            )
                        }
                        Err(e) => format!("Error: MCP tool '{}' not found: {e}", requested),
                    }
                } else if tc.name.starts_with("mcp__") {
                    match self.mcp_orchestrator.resolve_fn_name(&tc.name).await {
                        Some((server, tool)) => self
                            .mcp_orchestrator
                            .call_tool(&server, &tool, &tc.arguments)
                            .await
                            .map(|v| v.to_string())
                            .unwrap_or_else(|e| {
                                Self::format_structured_error(&tc.name, &e.to_string(), true)
                            }),
                        None => Self::format_structured_error(
                            &tc.name,
                            &format!("Unknown MCP tool: '{}'", tc.name),
                            false,
                        ),
                    }
                } else {
                    self.skill_executor
                        .tools
                        .execute(&tc.name, &tc.arguments, &primitive_perms)
                        .await
                        .unwrap_or_else(|e| {
                            Self::format_structured_error(&tc.name, &e.to_string(), true)
                        })
                };

                // Priority 3: Dynamic tool result truncation proportional to context window.
                // Skip truncation for activate_skill — its result is a prompt that
                // must be preserved in full for the LLM to follow the instructions.
                let result = if tc.name == "activate_skill" {
                    result
                } else {
                    self.truncate_tool_result(&result)
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
                    return Ok(AgentRunResult::FinalText(String::new()));
                }
            }
        }

        // Soft limit: inject a system message asking the LLM to wrap up,
        // rather than hard-failing. This lets the model produce a partial answer.
        debug!("[{agent_name}] reached MAX_ITER ({MAX_ITER}), requesting wrap-up");
        msgs.push(serde_json::json!({
            "role": "system",
            "content": format!(
                "You have reached the maximum number of tool-call iterations ({MAX_ITER}). \
                 You MUST now produce a final text response. Summarise what you have accomplished \
                 so far and note any remaining steps the user should complete manually."
            )
        }));
        // One final LLM call without tools to get a wrap-up response
        match self
            .llm_client
            .chat_with_tools_stream(msgs, vec![], |tok| on_token(tok.to_string()), abort)
            .await?
        {
            Some(r) => Ok(AgentRunResult::FinalText(r.content.unwrap_or_default())),
            None => Ok(AgentRunResult::FinalText(String::new())),
        }
    }

    // ── Parallel pack dispatch ────────────────────────────────────────────────

    /// Run multiple pups in parallel against the same task and aggregate results.
    /// This is the current "parallel fan-out" mode — not true Pack Channel (which
    /// requires an inter-pup message bus and is not yet implemented).
    async fn run_parallel_pack(
        &self,
        msg: &str,
        required_pups: Vec<String>,
        events: SharedEventSink,
    ) -> Result<String> {
        let pup_list = required_pups
            .iter()
            .map(|k| pup_display_name(k))
            .collect::<Vec<_>>()
            .join("、");
        debug!("[alpha] parallel_pack: pups={required_pups:?}");
        emit_event(
            events.as_ref(),
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: format!("pack:{}", required_pups.join(",")),
            },
        );

        let owner_summary = self
            .context_builder
            .get_owner_summary(&self.file_layer.read_owner_profile().unwrap_or_default())
            .await;

        // Pre-build shared PupContext to avoid redundant queries across pups
        let pup_ctx = PupContext {
            history: self.context_builder.build_history().await,
            active_rules: self
                .context_builder
                .memory_injector
                .fetch_active_rules(&MemoryBudget::default())
                .await
                .unwrap_or_default(),
        };

        let mut join_handles = Vec::new();
        for pup_key in &required_pups {
            let self_clone = self.clone();
            let msg_owned = msg.to_string();
            let pup_key_owned = pup_key.clone();
            let owner_ctx = owner_summary.clone();
            let pup_ctx_clone = pup_ctx.clone();
            let handle = tokio::spawn(async move {
                let result = self_clone
                    .run_pup_for_channel(
                        &pup_key_owned,
                        &msg_owned,
                        &owner_ctx,
                        &|_, _| {},
                        Some(pup_ctx_clone),
                    )
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

        emit_event(
            events.as_ref(),
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: format!("pack:{pup_list} → 汇总"),
            },
        );

        let aggregated = self
            .aggregate_channel_results(msg, &pup_outputs, events)
            .await;

        // Auto-ingest aggregated result as artifact to KB
        if let Ok(ref text) = aggregated {
            if text.len() > 100 {
                if self.kb_auto_ingest.load(Ordering::Relaxed) {
                    self.auto_ingest_artifact(msg, text).await;
                }
            }
        }

        aggregated
    }

    // ── DAG-based pack dispatch ───────────────────────────────────────────────

    /// Ask the LLM (mini model) to decompose the user message into per-pup subtasks
    /// with dependency information. Falls back to a flat parallel plan on any error.
    async fn decompose(&self, msg: &str, pup_keys: &[String]) -> DelegationPlan {
        let pup_list = pup_keys
            .iter()
            .map(|k| format!("  - {}: {}", k, pup_display_name(k)))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            "You are a task planner. Given a user request and a list of specialist pups, \
             create a delegation plan. Each pup should have a clear description of what it needs to do.\n\
             You MUST output valid JSON only, no markdown, no commentary.\n\
             Format:\n\
             {{\"channel_title\": \"short-title\", \"subtasks\": [\
             {{\"pup\": \"pup_key\", \"description\": \"what this pup should do\", \"depends_on\": []}}\
             ]}}\n\
             depends_on lists pup keys that must complete before this pup starts.\n\
             Available pups:\n{pup_list}"
        );

        let user_prompt = format!("User request: {msg}");

        let fallback = || DelegationPlan {
            channel_id: String::new(),
            channel_title: msg.chars().take(40).collect(),
            subtasks: pup_keys
                .iter()
                .map(|k| Subtask {
                    pup: k.clone(),
                    description: msg.to_string(),
                    depends_on: vec![],
                })
                .collect(),
        };

        let raw = match self
            .llm_client
            .chat_mini(vec![
                LlmMessage {
                    role: "system".into(),
                    content: system_prompt,
                },
                LlmMessage {
                    role: "user".into(),
                    content: user_prompt,
                },
            ])
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!("[alpha] decompose: LLM error: {e}");
                return fallback();
            }
        };

        // Parse JSON — be lenient about markdown fences
        let json_str = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        #[derive(serde::Deserialize)]
        struct LlmPlan {
            channel_title: String,
            subtasks: Vec<Subtask>,
        }

        match serde_json::from_str::<LlmPlan>(json_str) {
            Ok(parsed) => {
                // Validate: all pup keys must be known
                let valid: Vec<Subtask> = parsed
                    .subtasks
                    .into_iter()
                    .filter(|st| pup_keys.contains(&st.pup))
                    .collect();
                if valid.is_empty() {
                    debug!("[alpha] decompose: no valid subtasks parsed, using fallback");
                    return fallback();
                }
                DelegationPlan {
                    channel_id: String::new(),
                    channel_title: parsed.channel_title,
                    subtasks: valid,
                }
            }
            Err(e) => {
                debug!("[alpha] decompose: JSON parse error: {e}");
                fallback()
            }
        }
    }

    async fn execute_channel_layer(
        &self,
        channel_id: &str,
        layer: &[Subtask],
        dep_context: &HashMap<String, String>,
        owner_summary: &str,
        events: SharedEventSink,
        review_feedback: &HashMap<String, Vec<String>>,
        result_message_ids: &HashMap<String, String>,
        pup_ctx: Option<PupContext>,
    ) -> Vec<LayerExecutionResult> {
        let mut layer_handles = Vec::new();

        for subtask in layer {
            let pup_key = subtask.pup.clone();
            let pup_description = subtask.description.clone();
            let deps: Vec<String> = subtask.depends_on.clone();

            let injected: String = if deps.is_empty() {
                String::new()
            } else {
                deps.iter()
                    .filter_map(|dep| dep_context.get(dep))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };
            let feedback = review_feedback.get(&pup_key).cloned().unwrap_or_default();

            let self_clone = self.clone();
            let owner_ctx = owner_summary.to_string();
            let ch_id = channel_id.to_string();
            let app_clone = events.clone();
            let app_activity = events.clone();
            let result_message_ids = result_message_ids.clone();
            let pup_ctx_clone = pup_ctx.clone();

            let _ = self
                .channel_manager
                .post_status(channel_id, &pup_key, "started")
                .await;

            let cm_clone = self.channel_manager.clone();
            let handle = tokio::spawn(async move {
                cm_clone.monitor.register(&ch_id, &pup_key).await;

                let hb_ch_id = ch_id.clone();
                let hb_pup = pup_key.clone();
                let hb_cm = cm_clone.clone();
                let (hb_stop_tx, mut hb_stop_rx) = tokio::sync::oneshot::channel::<()>();
                let _hb_handle = tokio::spawn(async move {
                    let mut interval =
                        tokio::time::interval(crate::channel::heartbeat::HEARTBEAT_INTERVAL);
                    interval.tick().await;
                    loop {
                        tokio::select! {
                            _ = interval.tick() => {
                                hb_cm.post_heartbeat(&hb_ch_id, &hb_pup).await;
                            }
                            _ = &mut hb_stop_rx => {
                                break;
                            }
                        }
                    }
                });

                let mut full_msg = if injected.is_empty() {
                    pup_description.clone()
                } else {
                    format!(
                        "{}\n\n{}\n\n## Context from previous steps\n{}",
                        pup_description,
                        build_downstream_review_contract(&deps),
                        injected
                    )
                };
                if !feedback.is_empty() {
                    full_msg.push_str("\n\n## Review feedback\n");
                    full_msg.push_str(&feedback.join("\n"));
                }

                emit_event(
                    app_clone.as_ref(),
                    "stream_activity",
                    ActivityEvent {
                        kind: "routing".into(),
                        label: format!("pack:{}", pup_display_name(&pup_key)),
                    },
                );

                let result = self_clone
                    .run_pup_for_channel_with_activity(
                        &pup_key,
                        &full_msg,
                        &owner_ctx,
                        {
                            let review_tool = if deps.is_empty() {
                                None
                            } else {
                                Some(ReviewToolContext {
                                    allowed_targets: deps.clone(),
                                })
                            };
                            review_tool
                        },
                        {
                            let cm_activity = cm_clone.clone();
                            let ch_activity = ch_id.clone();
                            let pup_activity = pup_key.clone();
                            move |kind, label| {
                                let entry = format_activity_entry(&kind, &label);
                                let cm_emit = cm_activity.clone();
                                let ch_emit = ch_activity.clone();
                                let pup_emit = pup_activity.clone();
                                tokio::spawn(async move {
                                    let _ =
                                        cm_emit.post_activity(&ch_emit, &pup_emit, &entry).await;
                                });
                                emit_event(
                                    app_activity.as_ref(),
                                    "stream_activity",
                                    ActivityEvent { kind, label },
                                );
                            }
                        },
                        pup_ctx_clone,
                    )
                    .await
                    .unwrap_or_else(|e| AgentRunResult::FinalText(format!("Error: {e}")));

                let _ = hb_stop_tx.send(());
                cm_clone.monitor.unregister(&ch_id, &pup_key).await;

                if let AgentRunResult::ReviewRequest(review_request) = result {
                    let reply_to = review_request
                        .target_pup
                        .as_ref()
                        .and_then(|target| result_message_ids.get(target))
                        .cloned();
                    let target_pup = review_request.target_pup.clone();
                    let suggested_action = review_request.suggested_action.clone();
                    let _ = cm_clone
                        .post_message(
                            &ch_id,
                            &pup_key,
                            &review_request.summary,
                            "review_request",
                            None,
                            None,
                            &[],
                            reply_to.as_deref(),
                            Some(serde_json::json!({
                                "target_pup": target_pup,
                                "blocking": review_request.blocking,
                                "suggested_action": suggested_action,
                            })),
                        )
                        .await;
                    let _ = cm_clone.post_status(&ch_id, &pup_key, "blocked").await;
                    return LayerExecutionResult {
                        pup_key: pup_key.clone(),
                        result: String::new(),
                        message_id: String::new(),
                        review_request: Some(ParsedReviewRequest {
                            requester_pup: pup_key,
                            target_pup: review_request.target_pup,
                            summary: review_request.summary,
                            blocking: review_request.blocking,
                            suggested_action: review_request.suggested_action,
                        }),
                    };
                }

                let result_text = match result {
                    AgentRunResult::FinalText(text) => text,
                    AgentRunResult::ReviewRequest(_) => String::new(),
                };
                let message_id = cm_clone
                    .post_text(&ch_id, &pup_key, &result_text, &[])
                    .await
                    .ok();
                let status = if result_text.starts_with("Error:") {
                    "failed"
                } else {
                    "done"
                };
                let _ = cm_clone.post_status(&ch_id, &pup_key, status).await;

                LayerExecutionResult {
                    pup_key,
                    result: result_text,
                    message_id: message_id.unwrap_or_default(),
                    review_request: None,
                }
            });
            layer_handles.push(handle);
        }

        let joined = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            futures_util::future::join_all(layer_handles),
        )
        .await;

        match joined {
            Ok(results) => results.into_iter().filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    fn build_review_request_text(
        &self,
        layer_idx: usize,
        review_round: i64,
        requests: &[ParsedReviewRequest],
    ) -> String {
        let items = requests
            .iter()
            .map(|request| {
                let target = request
                    .target_pup
                    .as_ref()
                    .map(|pup| pup_display_name(pup))
                    .unwrap_or_else(|| "当前协作结果".to_string());
                format!(
                    "- {} 对 {} 有异议：{}",
                    pup_display_name(&request.requester_pup),
                    target,
                    request.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "第 {} 层触发第 {} 轮评审。\n\n异议摘要：\n{}\n\n默认不会在每层停下。只有出现明确异议时才进入评审。请决定继续，或要求修改。",
            layer_idx + 1,
            review_round,
            if items.is_empty() {
                "- 无".to_string()
            } else {
                items
            }
        )
    }

    /// DAG-based multi-pup dispatch. Decomposes the request, builds execution layers,
    /// runs layers sequentially (pups within a layer run in parallel), and aggregates.
    async fn run_dag(
        &self,
        msg: &str,
        required_pups: Vec<String>,
        events: SharedEventSink,
    ) -> Result<String> {
        debug!("[alpha] run_dag: pups={required_pups:?}");

        // 1. Decompose into subtasks
        let mut plan = self.decompose(msg, &required_pups).await;

        // 2. Build execution layers — fall back to run_parallel_pack on cycle
        let layers = match build_execution_layers(&plan.subtasks) {
            Ok(l) => l,
            Err(e) => {
                debug!("[alpha] run_dag: DAG build error ({e}), falling back to parallel pack");
                return self
                    .run_parallel_pack(msg, required_pups, events.clone())
                    .await;
            }
        };

        // 3. Create channel
        let task_id = Uuid::new_v4().to_string();
        let all_members: Vec<&str> = required_pups.iter().map(|s| s.as_str()).collect();
        let channel_id = self
            .channel_manager
            .create_channel(&task_id, &plan.channel_title, &all_members)
            .await?;
        plan.channel_id = channel_id.clone();
        self.memory.save_channel_plan(&plan).await?;

        // 4. Emit delegation_plan event
        emit_event(events.as_ref(), "delegation_plan", &plan);

        // 5. Post Alpha briefing
        let briefing = format!(
            "Pack Channel 已建立。任务：{}\n共 {} 个执行层，涉及 Pup：{}",
            msg,
            layers.len(),
            required_pups
                .iter()
                .map(|k| pup_display_name(k))
                .collect::<Vec<_>>()
                .join("、")
        );
        let _ = self
            .channel_manager
            .post_text(&channel_id, "alpha", &briefing, &[])
            .await;

        // 6. Spawn timeout monitor loop
        let monitor_channel_id = channel_id.clone();
        let monitor_cm = self.channel_manager.clone();
        let monitor_app = events.clone();
        let timeout_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if let Ok(Some(state)) = monitor_cm.workflow_state(&monitor_channel_id).await {
                    if state.status == "awaiting_review" {
                        continue;
                    }
                }
                let timed_out = monitor_cm.monitor.check_timeouts(&monitor_channel_id).await;
                for (pup_id, _kind) in timed_out {
                    debug!(
                        "[alpha] run_dag: pup '{pup_id}' timed out in channel {monitor_channel_id}"
                    );
                    let _ = monitor_cm
                        .post_status(&monitor_channel_id, &pup_id, "failed")
                        .await;
                    emit_event(
                        monitor_app.as_ref(),
                        "stream_activity",
                        ActivityEvent {
                            kind: "routing".into(),
                            label: format!("{} timed out", pup_display_name(&pup_id)),
                        },
                    );
                }
            }
        });

        // 7. Execute layers sequentially, pups within a layer in parallel
        let owner_summary = self
            .context_builder
            .get_owner_summary(&self.file_layer.read_owner_profile().unwrap_or_default())
            .await;

        // Pre-build shared PupContext to avoid redundant queries across layers/pups
        let pup_ctx = PupContext {
            history: self.context_builder.build_history().await,
            active_rules: self
                .context_builder
                .memory_injector
                .fetch_active_rules(&MemoryBudget::default())
                .await
                .unwrap_or_default(),
        };

        let mut all_results: HashMap<String, String> = HashMap::new();
        let mut result_message_ids: HashMap<String, String> = HashMap::new();
        // Accumulated context from prior layers to inject as deps
        let mut dep_context: HashMap<String, String> = HashMap::new();
        let subtask_by_pup: HashMap<String, (usize, Subtask)> = layers
            .iter()
            .enumerate()
            .flat_map(|(layer_idx, layer)| {
                layer
                    .iter()
                    .cloned()
                    .map(move |subtask| (subtask.pup.clone(), (layer_idx, subtask)))
            })
            .collect();

        for (layer_idx, layer) in layers.iter().enumerate() {
            debug!(
                "[alpha] run_dag: executing layer {} with {} pups",
                layer_idx,
                layer.len()
            );

            let mut review_round = 0_i64;
            let mut rerun_layer = layer.clone();
            let mut review_feedback: HashMap<String, Vec<String>> = HashMap::new();

            loop {
                let _ = self
                    .channel_manager
                    .update_workflow_state(
                        &channel_id,
                        "active",
                        Some(layer_idx as i64),
                        review_round,
                        false,
                        None,
                    )
                    .await;

                let layer_results = self
                    .execute_channel_layer(
                        &channel_id,
                        &rerun_layer,
                        &dep_context,
                        &owner_summary,
                        events.clone(),
                        &review_feedback,
                        &result_message_ids,
                        Some(pup_ctx.clone()),
                    )
                    .await;

                if layer_results.is_empty() {
                    debug!("[alpha] run_dag: layer {} timed out", layer_idx);
                }

                let objections: Vec<ParsedReviewRequest> = layer_results
                    .iter()
                    .filter_map(|result| result.review_request.clone())
                    .collect();

                for result in layer_results
                    .iter()
                    .filter(|result| result.review_request.is_none())
                {
                    dep_context.insert(result.pup_key.clone(), result.result.clone());
                    all_results.insert(result.pup_key.clone(), result.result.clone());
                    if !result.message_id.is_empty() {
                        result_message_ids
                            .insert(result.pup_key.clone(), result.message_id.clone());
                    }
                }

                if let Some(hook) = self.layer_hook.read().await.clone() {
                    let done_pups = rerun_layer
                        .iter()
                        .map(|subtask| pup_display_name(&subtask.pup))
                        .collect::<Vec<_>>();
                    hook(layer_idx, done_pups);
                }

                if objections.is_empty() {
                    let _ = self
                        .channel_manager
                        .update_workflow_state(
                            &channel_id,
                            "active",
                            Some((layer_idx + 1) as i64),
                            review_round,
                            false,
                            None,
                        )
                        .await;
                    break;
                }

                review_round += 1;
                let review_rx = self
                    .channel_manager
                    .begin_review(
                        &channel_id,
                        layer_idx,
                        review_round,
                        &self.build_review_request_text(layer_idx, review_round, &objections),
                    )
                    .await?;

                match review_rx.await {
                    Ok(ReviewDecision::Continue) => {
                        review_feedback = objections
                            .iter()
                            .map(|request| {
                                (
                                    request.requester_pup.clone(),
                                    vec!["Owner reviewed the objection and asked you to continue with the current context. Keep going unless there is a blocking issue.".to_string()],
                                )
                            })
                            .collect();
                        let mut rerun_pups = HashSet::new();
                        rerun_layer = objections
                            .iter()
                            .filter_map(|request| {
                                if !rerun_pups.insert(request.requester_pup.clone()) {
                                    return None;
                                }
                                subtask_by_pup
                                    .get(&request.requester_pup)
                                    .map(|(_, subtask)| subtask.clone())
                            })
                            .collect();
                        if rerun_layer.is_empty() {
                            rerun_layer = layer.clone();
                        }
                        let _ = self
                            .channel_manager
                            .update_workflow_state(
                                &channel_id,
                                "active",
                                Some(layer_idx as i64),
                                review_round,
                                false,
                                None,
                            )
                            .await;
                    }
                    Ok(ReviewDecision::RequestChanges {
                        sender,
                        comment,
                        reply_to,
                    }) => {
                        let feedback_line = format!("{}: {}", sender, comment.trim());
                        let target_pup = reply_to
                            .as_ref()
                            .and_then(|message_id| {
                                result_message_ids.iter().find(|(_, id)| *id == message_id)
                            })
                            .map(|(pup, _)| pup.clone())
                            .or_else(|| {
                                objections
                                    .iter()
                                    .find_map(|request| request.target_pup.clone())
                            });

                        review_feedback = HashMap::new();
                        if let Some(target_pup) = target_pup.clone() {
                            if let Some((_target_layer_idx, target_subtask)) =
                                subtask_by_pup.get(&target_pup)
                            {
                                review_feedback
                                    .insert(target_pup.clone(), vec![feedback_line.clone()]);
                                let refreshed = self
                                    .execute_channel_layer(
                                        &channel_id,
                                        &[target_subtask.clone()],
                                        &dep_context,
                                        &owner_summary,
                                        events.clone(),
                                        &review_feedback,
                                        &result_message_ids,
                                        Some(pup_ctx.clone()),
                                    )
                                    .await;
                                for result in refreshed
                                    .iter()
                                    .filter(|result| result.review_request.is_none())
                                {
                                    dep_context
                                        .insert(result.pup_key.clone(), result.result.clone());
                                    all_results
                                        .insert(result.pup_key.clone(), result.result.clone());
                                    if !result.message_id.is_empty() {
                                        result_message_ids.insert(
                                            result.pup_key.clone(),
                                            result.message_id.clone(),
                                        );
                                    }
                                }
                            }
                        }
                        review_feedback = objections
                            .iter()
                            .map(|request| {
                                (
                                    request.requester_pup.clone(),
                                    vec![format!(
                                        "Owner requested upstream changes. Re-evaluate with the refreshed context. Note: {}",
                                        feedback_line
                                    )],
                                )
                            })
                            .collect();
                        rerun_layer = layer.clone();

                        let _ = self
                            .channel_manager
                            .update_workflow_state(
                                &channel_id,
                                "active",
                                Some(layer_idx as i64),
                                review_round,
                                false,
                                Some("changes_requested"),
                            )
                            .await;
                    }
                    Ok(ReviewDecision::Abort { comment }) => {
                        timeout_handle.abort();
                        let _ = self
                            .channel_manager
                            .update_workflow_state(
                                &channel_id,
                                "completed",
                                Some(layer_idx as i64),
                                review_round,
                                false,
                                Some("aborted"),
                            )
                            .await;
                        let _ = self.channel_manager.complete(&channel_id).await;
                        let abort_msg = if comment.is_empty() {
                            "Channel aborted by owner.".to_string()
                        } else {
                            format!("Channel aborted by owner: {comment}")
                        };
                        return Ok(abort_msg);
                    }
                    Err(_) => {
                        timeout_handle.abort();
                        let _ = self.channel_manager.clear_review_session(&channel_id).await;
                        let _ = self
                            .channel_manager
                            .update_workflow_state(
                                &channel_id,
                                "completed",
                                Some(layer_idx as i64),
                                review_round,
                                false,
                                Some("interrupted"),
                            )
                            .await;
                        let _ = self.channel_manager.complete(&channel_id).await;
                        return Err(anyhow!("review session interrupted"));
                    }
                }
            }
        }

        // 8. Cancel timeout monitor
        timeout_handle.abort();

        // 9. Aggregate results
        emit_event(
            events.as_ref(),
            "stream_activity",
            ActivityEvent {
                kind: "routing".into(),
                label: "pack → 汇总".into(),
            },
        );

        let aggregated = self
            .aggregate_channel_results(
                msg,
                &all_results.into_iter().collect::<Vec<(String, String)>>(),
                events,
            )
            .await;

        // 10. Complete channel
        let _ = self.channel_manager.complete(&channel_id).await;

        // Auto-ingest aggregated result as artifact to KB
        if let Ok(ref text) = aggregated {
            if text.len() > 100 {
                if self.kb_auto_ingest.load(Ordering::Relaxed) {
                    self.auto_ingest_artifact(msg, text).await;
                }
            }
        }

        aggregated
    }

    async fn run_dag_bridge(
        &self,
        msg: &str,
        required_pups: Vec<String>,
        progress_hook: Option<BridgeProgressHook>,
    ) -> Result<String> {
        debug!("[alpha] run_dag_bridge: pups={required_pups:?}");

        let mut plan = self.decompose(msg, &required_pups).await;
        let layers = match build_execution_layers(&plan.subtasks) {
            Ok(layers) => layers,
            Err(e) => {
                debug!(
                    "[alpha] run_dag_bridge: DAG build error ({e}), falling back to single layer"
                );
                vec![plan.subtasks.clone()]
            }
        };

        let task_id = Uuid::new_v4().to_string();
        let all_members: Vec<&str> = required_pups.iter().map(|s| s.as_str()).collect();
        let channel_id = self
            .channel_manager
            .create_channel(&task_id, &plan.channel_title, &all_members)
            .await?;
        plan.channel_id = channel_id.clone();
        self.memory.save_channel_plan(&plan).await?;

        let briefing = format!(
            "Bridge 协作已建立。任务：{}\n共 {} 个执行层，涉及 Pup：{}",
            msg,
            layers.len(),
            required_pups
                .iter()
                .map(|key| pup_display_name(key))
                .collect::<Vec<_>>()
                .join("、")
        );
        let _ = self
            .channel_manager
            .post_text(&channel_id, "alpha", &briefing, &[])
            .await;
        Self::emit_bridge_progress(
            &progress_hook,
            format!("协作频道已创建：{}", plan.channel_title),
        );

        let monitor_channel_id = channel_id.clone();
        let monitor_cm = self.channel_manager.clone();
        let timeout_progress = progress_hook.clone();
        let timeout_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let timed_out = monitor_cm.monitor.check_timeouts(&monitor_channel_id).await;
                for (pup_id, _kind) in timed_out {
                    let _ = monitor_cm
                        .post_status(&monitor_channel_id, &pup_id, "failed")
                        .await;
                    if let Some(hook) = &timeout_progress {
                        hook(format!("{} 超时，已标记失败", pup_display_name(&pup_id)));
                    }
                }
            }
        });

        let owner_summary = self
            .context_builder
            .get_owner_summary(&self.file_layer.read_owner_profile().unwrap_or_default())
            .await;

        let mut all_results: Vec<(String, String)> = Vec::new();
        let mut dep_context: HashMap<String, String> = HashMap::new();

        for (layer_idx, layer) in layers.iter().enumerate() {
            Self::emit_bridge_progress(
                &progress_hook,
                format!(
                    "开始第 {} 层：{}",
                    layer_idx + 1,
                    layer
                        .iter()
                        .map(|subtask| pup_display_name(&subtask.pup))
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            );

            let mut layer_handles = Vec::new();

            for subtask in layer {
                let pup_key = subtask.pup.clone();
                let pup_description = subtask.description.clone();
                let deps: Vec<String> = subtask.depends_on.clone();

                let injected: String = if deps.is_empty() {
                    String::new()
                } else {
                    deps.iter()
                        .filter_map(|dep| dep_context.get(dep))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };

                let self_clone = self.clone();
                let owner_ctx = owner_summary.clone();
                let ch_id = channel_id.clone();
                let cm_clone = self.channel_manager.clone();
                let progress_for_task = progress_hook.clone();

                let _ = self
                    .channel_manager
                    .post_status(&channel_id, &pup_key, "started")
                    .await;
                Self::emit_bridge_progress(
                    &progress_hook,
                    format!("{} 开始执行", pup_display_name(&pup_key)),
                );

                let handle = tokio::spawn(async move {
                    cm_clone.monitor.register(&ch_id, &pup_key).await;

                    let hb_ch_id = ch_id.clone();
                    let hb_pup = pup_key.clone();
                    let hb_cm = cm_clone.clone();
                    let (hb_stop_tx, mut hb_stop_rx) = tokio::sync::oneshot::channel::<()>();
                    let _hb_handle = tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(crate::channel::heartbeat::HEARTBEAT_INTERVAL);
                        interval.tick().await;
                        loop {
                            tokio::select! {
                                _ = interval.tick() => {
                                    hb_cm.post_heartbeat(&hb_ch_id, &hb_pup).await;
                                }
                                _ = &mut hb_stop_rx => {
                                    break;
                                }
                            }
                        }
                    });

                    let full_msg = if injected.is_empty() {
                        pup_description.clone()
                    } else {
                        format!(
                            "{}\n\n## Context from previous steps\n{}",
                            pup_description, injected
                        )
                    };

                    let result = self_clone
                        .run_pup_for_channel(&pup_key, &full_msg, &owner_ctx, &|_, _| {}, None)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"));

                    let _ = hb_stop_tx.send(());
                    cm_clone.monitor.unregister(&ch_id, &pup_key).await;

                    let _ = cm_clone.post_text(&ch_id, &pup_key, &result, &[]).await;
                    let status = if result.starts_with("Error:") {
                        "failed"
                    } else {
                        "done"
                    };
                    let _ = cm_clone.post_status(&ch_id, &pup_key, status).await;

                    if let Some(hook) = &progress_for_task {
                        let summary = if status == "failed" {
                            format!("{} 执行失败", pup_display_name(&pup_key))
                        } else {
                            format!("{} 已完成", pup_display_name(&pup_key))
                        };
                        hook(summary);
                    }

                    (pup_key, result)
                });
                layer_handles.push(handle);
            }

            let joined = tokio::time::timeout(
                std::time::Duration::from_secs(300),
                futures_util::future::join_all(layer_handles),
            )
            .await;

            let layer_results: Vec<(String, String)> = match joined {
                Ok(results) => results.into_iter().filter_map(|r| r.ok()).collect(),
                Err(_) => {
                    Self::emit_bridge_progress(
                        &progress_hook,
                        format!("第 {} 层等待超时", layer_idx + 1),
                    );
                    vec![]
                }
            };

            for (pup_key, result) in &layer_results {
                dep_context.insert(pup_key.clone(), result.clone());
            }

            all_results.extend(layer_results);

            if let Some(hook) = self.layer_hook.read().await.clone() {
                let done_pups = layer
                    .iter()
                    .map(|subtask| pup_display_name(&subtask.pup))
                    .collect::<Vec<_>>();
                hook(layer_idx, done_pups);
            }
        }

        timeout_handle.abort();
        Self::emit_bridge_progress(&progress_hook, "正在汇总最终结果…");

        let null_events: SharedEventSink = Arc::new(crate::runtime::NullEventSink);
        let aggregated = self
            .aggregate_channel_results(msg, &all_results, null_events)
            .await?;
        let _ = self
            .channel_manager
            .post_text(&channel_id, "alpha", &aggregated, &[])
            .await;
        let _ = self.channel_manager.complete(&channel_id).await;

        // Auto-ingest aggregated result as artifact to KB
        if aggregated.len() > 100 && self.kb_auto_ingest.load(Ordering::Relaxed) {
            self.auto_ingest_artifact(msg, &aggregated).await;
        }

        Ok(aggregated)
    }

    /// Entry point for external bridge messages (Telegram, Discord, Slack).
    /// Uses the same routing as process_user_message_stream but returns a Result
    /// instead of emitting Tauri events.
    #[allow(dead_code)]
    pub async fn process_bridge_message(
        &self,
        msg: String,
        progress_hook: Option<BridgeProgressHook>,
    ) -> anyhow::Result<String> {
        self.abort_flag.store(false, Ordering::Relaxed);
        let owner_md = self.file_layer.read_owner_profile().unwrap_or_default();
        let owner_summary = self.context_builder.get_owner_summary(&owner_md).await;
        let classify_history = self.router.build_classify_history().await;
        let pup_key = self
            .router
            .classify_intent(&msg, &owner_summary, &classify_history)
            .await;

        if let Some(pups_str) = pup_key.strip_prefix("channel:") {
            let pups: Vec<String> = pups_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if pups.len() >= 2 {
                Self::emit_bridge_progress(
                    &progress_hook,
                    format!(
                        "已触发协作：{}",
                        pups.iter()
                            .map(|pup| pup_display_name(pup))
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                );
                let reply = self
                    .run_dag_bridge(&msg, pups, progress_hook.clone())
                    .await?;
                if !reply.is_empty() && !self.abort_flag.load(Ordering::Relaxed) {
                    self.post_process_conversation_turn("alpha", &msg, &reply)
                        .await?;
                }
                return Ok(reply);
            }
        }

        // Single pup path
        if self.resolve_pup(&pup_key).await.is_ok() {
            let reply = self
                .run_pup_for_channel(&pup_key, &msg, &owner_summary, &|_, _| {}, None)
                .await?;
            self.record_pup_context_tokens_async(&pup_key).await;
            if !reply.is_empty() && !self.abort_flag.load(Ordering::Relaxed) {
                self.post_process_conversation_turn(&pup_key, &msg, &reply)
                    .await?;
            }
            return Ok(reply);
        }

        // Alpha fallback — use the same tool-call loop as the UI path
        // so bridge users get skills, MCP tools, and task management.
        let pup_history = self.context_builder.build_history().await;
        let memory_ctx = self
            .memory_injector
            .build_memory_context(&msg, &MemoryBudget::default())
            .await
            .unwrap_or_default();
        let mem_str = MemoryInjector::format_for_injection(&memory_ctx);
        let relevant_memories: Vec<String> = if mem_str.is_empty() {
            vec![]
        } else {
            vec![mem_str]
        };
        let pending_tasks = self.memory.list_tasks(5).await.unwrap_or_default();
        let null_events: SharedEventSink = Arc::new(crate::runtime::NullEventSink);
        let reply = self
            .alpha_reply_stream(
                &msg,
                &owner_summary,
                &pup_history,
                &relevant_memories,
                &pending_tasks,
                null_events,
            )
            .await?;
        self.record_pup_context_tokens_async("alpha").await;
        if !reply.is_empty() && !self.abort_flag.load(Ordering::Relaxed) {
            self.post_process_conversation_turn("alpha", &msg, &reply)
                .await?;
        }
        Ok(reply)
    }

    pub async fn process_group_message(
        &self,
        conversation_id: &str,
        group_title: &str,
        msg: &str,
    ) -> anyhow::Result<String> {
        self.abort_flag.store(false, Ordering::Relaxed);
        let group_messages = self
            .memory
            .list_conversation_messages(conversation_id, 40)
            .await
            .unwrap_or_default();

        let mut messages: Vec<serde_json::Value> = vec![serde_json::json!({
            "role": "system",
            "content": format!(
                "You are Alpha, an AI member of the OpenPup group \"{group_title}\".\n\
                 This is a group-scoped conversation. Use only the messages from this group and the current user message.\n\
                 Do not use personal chat history, private owner memory, or context from other groups.\n\
                 Reply in the language used by the group."
            ),
        })];

        let last_human_idx = group_messages
            .iter()
            .rposition(|message| message.sender_kind == "human" && message.content == msg);
        for (idx, message) in group_messages.into_iter().enumerate() {
            if Some(idx) == last_human_idx {
                continue;
            }
            if message.sender_kind == "system" {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": message.content,
                }));
                continue;
            }

            let role = if message.sender_kind == "agent" {
                "assistant"
            } else {
                "user"
            };
            let route = message
                .route_label
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!(" via {value}"))
                .unwrap_or_default();
            messages.push(serde_json::json!({
                "role": role,
                "content": format!("[{}{}] {}", message.sender_name, route, message.content),
            }));
        }

        messages.push(serde_json::json!({ "role": "user", "content": msg }));

        let tool_perms = PupToolPermissions {
            shell: false,
            sandbox_shell: false,
            file_read: false,
            file_write: false,
            network: false,
            mcp: true,
        };
        let null_flag = &self.abort_flag;
        self.run_agent_with_tools(
            "alpha",
            messages,
            &tool_perms,
            None,
            |_| {},
            |_, _| {},
            null_flag,
        )
        .await
        .map(|result| match result {
            AgentRunResult::FinalText(text) => text,
            AgentRunResult::ReviewRequest(_) => {
                "Error: review requests are not supported for group chat.".to_string()
            }
        })
    }

    async fn post_process_conversation_turn(
        &self,
        pup_key: &str,
        msg: &str,
        reply: &str,
    ) -> Result<()> {
        self.memory.add_conversation(pup_key, "user", msg).await?;
        self.memory
            .add_conversation(pup_key, "assistant", reply)
            .await?;

        let pup_label = pup_display_name(pup_key);
        let snippet: String = msg.chars().take(80).collect();
        let ellipsis = if msg.chars().count() > 80 { "…" } else { "" };
        let diary_line = format!("💬 [{pup_label}] {snippet}{ellipsis}");
        let _ = self.file_layer.append_daily_diary(&[diary_line]);

        let count = self.msg_count.fetch_add(1, Ordering::Relaxed);
        if count % 3 == 0 {
            let _ = self.maybe_extract_memories(pup_key).await;
        }

        // Auto-ingest conversation summary to KB every 6 messages
        if count % 6 == 0 && count > 0 && self.kb_auto_ingest.load(Ordering::Relaxed) {
            let _ = self.maybe_ingest_conversation_summary(pup_key).await;
        }

        // Priority 5: Multi-layer context compaction.
        // Uses pressure-based strategy selection: micro-compact at 40%, full compact
        // at 65%, emergency persist+compact at 85%. Falls back to message-count
        // trigger every 10 turns when real token counts aren't available.
        // v2: compaction is global (shared history), not per-pup.
        let context_limit = self.get_context_limit();
        let should_compact = if let Some(tokens) = self.get_context_tokens(pup_key).await {
            tokens > context_limit * 2 / 5 // 40% threshold for any compaction
        } else {
            count % 10 == 0
        };
        if should_compact {
            let current_tokens = self
                .get_context_tokens(pup_key)
                .await
                .unwrap_or(context_limit / 2); // conservative estimate if unknown
            let engine = self.compaction_engine.clone();
            tokio::spawn(async move {
                match engine.compact(current_tokens, context_limit).await {
                    Ok(results) => {
                        for r in &results {
                            if r.estimated_tokens_saved > 0 {
                                debug!(
                                    "[alpha] compaction(shared) {}: saved ~{} tokens",
                                    r.strategy, r.estimated_tokens_saved
                                );
                            }
                        }
                    }
                    Err(e) => debug!("[alpha] compaction error: {e}"),
                }
            });
        }

        self.maybe_create_task(msg, pup_key).await;
        Ok(())
    }

    /// Run multiple pups and collect results without channel overhead (bridge path).
    async fn run_pups_for_results(&self, msg: &str, pup_keys: &[String]) -> Vec<(String, String)> {
        let owner_md = self.file_layer.read_owner_profile().unwrap_or_default();
        let owner_summary = self.context_builder.get_owner_summary(&owner_md).await;
        let handles: Vec<_> = pup_keys
            .iter()
            .map(|key| {
                let s = self.clone();
                let k = key.clone();
                let m = msg.to_string();
                let o = owner_summary.clone();
                tokio::spawn(async move {
                    let result = s
                        .run_pup_for_channel(&k, &m, &o, &|_, _| {}, None)
                        .await
                        .unwrap_or_else(|e| format!("Error: {e}"));
                    (k, result)
                })
            })
            .collect();
        futures_util::future::join_all(handles)
            .await
            .into_iter()
            .filter_map(|r| r.ok())
            .collect()
    }

    /// Run a specialist pup's tool-call loop for a channel task (no streaming to chat).
    ///
    /// Returns a boxed future to allow recursive pup_to_pup delegation without
    /// creating infinitely-sized futures.
    fn run_pup_for_channel<'a>(
        &'a self,
        pup_key: &'a str,
        msg: &'a str,
        owner_summary: &'a str,
        on_activity: &'a (dyn Fn(String, String) + Send + Sync),
        pup_ctx: Option<PupContext>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            match self
                .run_pup_for_channel_with_activity(
                    pup_key,
                    msg,
                    owner_summary,
                    None,
                    |kind, label| on_activity(kind, label),
                    pup_ctx,
                )
                .await?
            {
                AgentRunResult::FinalText(text) => Ok(text),
                AgentRunResult::ReviewRequest(_) => Ok(
                    "Error: review requests are not supported for this execution path.".to_string(),
                ),
            }
        })
    }

    async fn run_pup_for_channel_with_activity(
        &self,
        pup_key: &str,
        msg: &str,
        owner_summary: &str,
        review_tool: Option<ReviewToolContext>,
        on_activity: impl Fn(String, String) + Send + Sync,
        pup_ctx: Option<PupContext>,
    ) -> Result<AgentRunResult> {
        let pup = match self.resolve_pup(pup_key).await {
            Ok(pup) => pup,
            Err(err) => return Ok(AgentRunResult::FinalText(err.to_string())),
        };

        let override_prompt = self
            .configured_pup(pup_key)
            .await
            .map(|c| c.system_prompt_override);

        // Inject shared memories (rules + semantic) into channel pup context
        let (_, memories_str) = if let Some(ref ctx) = pup_ctx {
            let memory_context = self
                .context_builder
                .memory_injector
                .build_memory_context_with_preloaded_rules(
                    msg,
                    &MemoryBudget::default(),
                    None,
                    Some(&ctx.active_rules),
                )
                .await
                .unwrap_or_default();
            let formatted = MemoryInjector::format_for_injection(&memory_context);
            (memory_context, formatted)
        } else {
            self.context_builder.build_memory_context(msg).await
        };
        let relevant_memories = ContextBuilder::format_memories_for_prompt(&memories_str);

        // v2: inject shared conversation history so delegated pups have full context
        let shared_history = if let Some(ref ctx) = pup_ctx {
            ctx.history.clone()
        } else {
            self.context_builder.build_history().await
        };

        let task = Task {
            id: Uuid::new_v4().to_string(),
            intent: msg.to_string(),
            context: shared_history
                .iter()
                .map(|m| Message {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
            owner_context: owner_summary.to_string(),
            relevant_memories,
            system_prompt_override: override_prompt.filter(|s| !s.is_empty()),
            assigned_pup: Some(pup_key.to_string()),
            status: TaskStatus::Pending,
        };

        let system_prompt = pup.build_system_prompt(&task);
        let base_perms = pup.tool_permissions();
        let tool_perms = if let Some(cfg) = self.configured_pup(pup_key).await {
            cfg.permissions
                .map(|p| p.merge_over(base_perms.clone()))
                .unwrap_or(base_perms)
        } else {
            base_perms
        };
        let mut msgs: Vec<serde_json::Value> =
            vec![serde_json::json!({ "role": "system", "content": system_prompt })];
        for m in &task.context {
            msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
        }
        msgs.push(serde_json::json!({ "role": "user", "content": task.intent }));

        self.run_agent_with_tools(
            pup_key,
            msgs,
            &tool_perms,
            review_tool.as_ref(),
            |_tok| {}, // channel pups don't stream to chat
            on_activity,
            &self.abort_flag,
        )
        .await
    }

    /// LLM-based aggregation of multiple pup outputs into a final user-facing reply.
    async fn aggregate_channel_results(
        &self,
        original_msg: &str,
        results: &[(String, String)],
        events: SharedEventSink,
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
            .chat_stream(
                vec![
                    LlmMessage {
                        role: "system".into(),
                        content: "你是 Alpha Pup，负责整合多 Pup 协作成果并输出清晰的最终回复。"
                            .into(),
                    },
                    LlmMessage {
                        role: "user".into(),
                        content: prompt,
                    },
                ],
                |tok, _is_reasoning| {
                    emit_event(events.as_ref(), "stream_token", tok);
                },
                &self.abort_flag,
            )
            .await
    }

    // ── Per-pup context token tracking ──────────────────────────────────────────

    /// Record prompt_tokens from the last API call for a pup.
    async fn record_pup_context_tokens_async(&self, pup: &str) {
        if let Some(usage) = self.llm_client.take_last_call_usage() {
            if usage.prompt_tokens > 0 {
                self.per_pup_context_tokens
                    .write()
                    .await
                    .insert(pup.to_string(), usage.prompt_tokens);
            }
        }
    }

    /// Get the real context token count for a pup (from its last API call).
    pub async fn get_context_tokens(&self, pup: &str) -> Option<u64> {
        self.per_pup_context_tokens.read().await.get(pup).copied()
    }

    /// Get the model's context window limit.
    pub fn get_context_limit(&self) -> u64 {
        let model = self.llm_client.model_name();
        infer_context_limit_for_model(&model)
    }

    async fn maybe_extract_memories(&self, _pup: &str) -> Result<()> {
        let recent = self.memory.recent_conversations(10).await?;
        if recent.is_empty() {
            return Ok(());
        }
        let transcript = recent
            .into_iter()
            .rev()
            .map(|(role, content, _speaker)| format!("{role}: {content}"))
            .collect::<Vec<_>>()
            .join("\n");

        // v0.1.12: use MemoryExtractor with LLM conflict resolution
        let diary_entries = self
            .memory_extractor
            .extract_and_resolve_with_diary(&transcript, None)
            .await?;

        let _ = self.file_layer.append_daily_diary(&diary_entries);
        Ok(())
    }

    // ── Artifact auto-ingestion → KB ─────────────────────────────────────────────

    async fn auto_ingest_artifact(&self, original_msg: &str, artifact: &str) {
        let title_snippet: String = original_msg.chars().take(60).collect();
        let title = format!(
            "协作产出: {} ({})",
            title_snippet,
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );

        let ingestor = crate::knowledge::ingestor::Ingestor::with_llm(
            self.memory.clone(),
            self.llm_client.clone(),
        );
        let req = crate::knowledge::types::IngestTextRequest {
            title,
            content: artifact.to_string(),
            source_type: "artifact".to_string(),
            tags: vec!["auto-artifact".to_string()],
        };

        match ingestor.ingest_text(&req).await {
            Ok(id) => debug!("[alpha] artifact auto-ingested to KB: {id}"),
            Err(e) => debug!("[alpha] artifact auto-ingest failed: {e}"),
        }
    }

    // ── Conversation auto-summary → KB ────────────────────────────────────────────

    async fn maybe_ingest_conversation_summary(&self, pup: &str) -> Result<()> {
        let recent = self.memory.recent_conversations(12).await?;
        if recent.len() < 4 {
            return Ok(()); // not enough content to summarize
        }

        let transcript = recent
            .into_iter()
            .rev()
            .map(|(role, content, _speaker)| format!("{role}: {content}"))
            .collect::<Vec<_>>()
            .join("\n");

        // Ask the LLM to produce a concise summary worth storing
        let prompt = format!(
            "请将下面的对话浓缩为一段 200-400 字的摘要，保留关键决策、结论和行动项。\
       如果对话没有实质性内容（纯闲聊/问候），返回空字符串 \"\"。\n\n{transcript}"
        );

        let summary = self
            .llm_client
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await?;

        let summary = summary.trim().trim_matches('"');
        if summary.is_empty() || summary.len() < 30 {
            return Ok(());
        }

        let ingestor = crate::knowledge::ingestor::Ingestor::with_llm(
            self.memory.clone(),
            self.llm_client.clone(),
        );
        let req = crate::knowledge::types::IngestTextRequest {
            title: format!(
                "对话摘要 ({} · {})",
                pup,
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content: summary.to_string(),
            source_type: "conversation".to_string(),
            tags: vec!["auto-summary".to_string(), pup.to_string()],
        };

        match ingestor.ingest_text(&req).await {
            Ok(id) => debug!("[alpha] conversation summary ingested: {id}"),
            Err(e) => debug!("[alpha] conversation summary ingest failed: {e}"),
        }

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

    /// Manually trigger context compression for a pup (exposed for the Pack UI).
    /// Uses the multi-layer compaction engine with High pressure to force compaction.
    pub async fn compress_pup_context_now(&self, pup: &str) -> Result<()> {
        let context_limit = self.get_context_limit();
        // Use High pressure (70%) to force both micro and full compaction
        let simulated_tokens = (context_limit as f64 * 0.70) as u64;
        let results = self
            .compaction_engine
            .compact(simulated_tokens, context_limit)
            .await?;
        for r in &results {
            if r.estimated_tokens_saved > 0 {
                debug!(
                    "[alpha] manual compaction({}) {}: saved ~{} tokens",
                    pup, r.strategy, r.estimated_tokens_saved
                );
            }
        }
        Ok(())
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
        permissions: Option<PupPermissionConfig>,
    ) -> Result<()> {
        {
            let mut guard = self.pup_configs.write().await;
            if let Some(cfg) = guard.get_mut(key) {
                cfg.system_prompt_override = system_prompt_override;
                cfg.enabled = enabled;
                cfg.permissions = permissions;
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
            permissions: None,
        };
        {
            let mut guard = self.pup_configs.write().await;
            guard.insert(key.clone(), cfg);
        }
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
        // Evict any stale cached runtime instance from older app sessions/logic.
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
        "activate_skill" => {
            let skill = args["name"].as_str().unwrap_or("");
            ("skill".into(), format!("activate: {skill}"))
        }
        "fetch_mcp_tool" => {
            let tool = args["tool_name"].as_str().unwrap_or("");
            ("mcp".into(), format!("fetch: {tool}"))
        }
        "pup_to_pup" => {
            let target = args["target_pup"].as_str().unwrap_or("?");
            let task = args["task"].as_str().unwrap_or("");
            (
                "delegation".into(),
                format!("→ {target}: {}", trunc(task, 50)),
            )
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

fn format_activity_entry(kind: &str, label: &str) -> String {
    let prefix = match kind {
        "shell" => "Shell",
        "file_read" => "Read",
        "file_write" => "Write",
        "http" => "HTTP",
        "memory" => "Memory",
        "task" => "Task",
        "skill" => "Skill",
        "mcp" => "MCP",
        "review" => "Review",
        "tool_call" => "Tool",
        _ => "Activity",
    };
    format!("{prefix}: {label}")
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

/// Infer the context window limit (in tokens) from a model name string.
/// Public so other modules (main, primitive tools) can use it.
pub fn infer_context_limit_for_model(model: &str) -> u64 {
    let m = model.to_lowercase();
    if m.contains("gpt-4o") || m.contains("gpt-4-turbo") {
        128_000
    } else if m.contains("gpt-4") {
        8_192
    } else if m.contains("gpt-3.5") {
        16_385
    } else if m.contains("deepseek") {
        65_536
    } else if m.contains("claude-3") || m.contains("claude-4") {
        200_000
    } else if m.contains("qwen") {
        131_072
    } else if m.contains("llama-3") || m.contains("llama3") {
        131_072
    } else if m.contains("gemma") {
        8_192
    } else if m.contains("mistral") || m.contains("mixtral") {
        32_768
    } else {
        // Conservative default
        128_000
    }
}
