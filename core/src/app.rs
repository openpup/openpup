use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use openpup_capabilities::{Capabilities, CapabilityProfile};

use crate::agents::alpha::AlphaPup;
use crate::agents::dev_pup::DevPup;
use crate::agents::life_admin_pup::LifeAdminPup;
use crate::agents::ops_pup::OpsPup;
use crate::agents::research_pup::ResearchPup;
use crate::agents::specialist::SpecialistPup;
use crate::agents::writer_pup::WriterPup;
use crate::channel::manager::ChannelManager;
use crate::channel::types::{
    ChannelMessageRecord, ChannelRecord, ChannelWorkflowState, DelegationPlan,
};
use crate::llm::client::{LlmClient, Provider};
use crate::mcp::orchestrator::McpToolInfo;
use crate::mcp::orchestrator::{MCPOrchestrator, McpServerEntry};
use crate::memory::file_layer::FileLayer;
use crate::memory::system::{MemorySystem, SkillRunRecord, TaskRecord};
use crate::runtime::SharedEventSink;
use crate::skills::executor::SkillExecutor;
use crate::skills::job_registry::{JobRegistry, ScheduledJob};
use crate::skills::permissions::{ExecutionMode, PermissionChecker, PermissionUi};
use crate::skills::registry::{InstalledSkill, SkillRegistry};
use crate::tools::primitive::ToolRegistry;

#[derive(Clone)]
pub struct OpenPupApp {
    pub workspace_root: PathBuf,
    pub capabilities: Arc<Capabilities>,
    pub alpha: Arc<AlphaPup>,
    pub memory: Arc<MemorySystem>,
    pub file_layer: Arc<FileLayer>,
    pub permissions: PermissionChecker,
    pub skill_executor: Arc<SkillExecutor>,
    pub mcp_orchestrator: Arc<MCPOrchestrator>,
    pub channel_manager: Arc<ChannelManager>,
    pub bridge_outbox: crate::bridge::types::BridgeOutbox,
}

impl OpenPupApp {
    pub fn capability_profile(&self) -> CapabilityProfile {
        self.capabilities.env.capability_profile()
    }

    pub fn is_mobile_runtime(&self) -> bool {
        matches!(
            self.capability_profile(),
            CapabilityProfile::AndroidMobile | CapabilityProfile::IosRestricted
        )
    }

    pub fn llm_provider_name(&self) -> String {
        match self.alpha.llm_client.provider() {
            Provider::OpenAI => "openai",
            Provider::Ollama => "ollama",
        }
        .to_string()
    }

    pub fn current_llm_config(
        &self,
    ) -> (
        Provider,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) {
        let (provider, model, mini_model, embed_model, _api_key, api_base) =
            self.alpha.llm_client.current_config();
        (provider, model, mini_model, embed_model, None, api_base)
    }

    pub fn current_llm_config_with_secret(
        &self,
    ) -> (
        Provider,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) {
        self.alpha.llm_client.current_config()
    }

    pub fn reconfigure_llm(
        &self,
        provider: Provider,
        model: String,
        mini_model: Option<String>,
        embed_model: Option<String>,
        api_key: Option<String>,
        api_base: Option<String>,
    ) {
        self.alpha.llm_client.reconfigure(
            provider,
            model,
            mini_model,
            embed_model,
            api_key,
            api_base,
        );
    }

    pub fn token_usage_snapshot(&self) -> crate::llm::client::TokenUsage {
        self.alpha.llm_client.usage.snapshot()
    }

    pub fn reset_token_usage(&self) {
        self.alpha.llm_client.usage.reset();
    }

    pub fn abort_current_message(&self) {
        self.alpha
            .abort_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn process_user_message_stream(
        &self,
        input: String,
        forced_pup: Option<String>,
        event_sink: SharedEventSink,
    ) {
        self.alpha
            .process_user_message_stream(input, forced_pup, event_sink)
            .await;
    }

    pub fn onboarding_completed(&self) -> bool {
        self.file_layer.is_onboarding_completed()
    }

    pub fn owner_profile(&self) -> Result<String> {
        self.file_layer.read_owner_profile()
    }

    pub fn write_owner_profile(&self, content: &str) -> Result<()> {
        self.file_layer.write_owner_profile(content)
    }

    pub fn list_diary_dates(&self) -> Vec<String> {
        self.file_layer.list_diary_dates()
    }

    pub fn read_diary(&self, date: &str) -> Result<String> {
        self.file_layer.read_diary(date)
    }

    pub async fn add_memory(&self, content: &str, memory_type: &str, importance: f32) {
        let _ = self
            .memory
            .add_long_term_memory(content, memory_type, importance)
            .await;
    }

    pub async fn list_long_term_memories(
        &self,
        offset: i64,
        limit: i64,
        query: Option<&str>,
    ) -> Result<Vec<(String, String, String, f32, i64, Option<String>)>> {
        self.memory
            .list_long_term_memories(offset, limit, query)
            .await
    }

    pub async fn update_long_term_memory(
        &self,
        id: &str,
        content: &str,
        memory_type: &str,
        importance: f32,
    ) -> Result<()> {
        self.memory
            .update_long_term_memory(id, content, memory_type, importance)
            .await
    }

    pub async fn delete_long_term_memory(&self, id: &str) -> Result<()> {
        self.memory.delete_long_term_memory(id).await
    }

    pub async fn top_memories(&self, limit: i64) -> Result<Vec<(String, String, f32)>> {
        self.memory.get_top_memories(limit).await
    }

    pub async fn list_skill_runs(&self, limit: i64) -> Result<Vec<SkillRunRecord>> {
        self.memory.list_skill_runs(limit).await
    }

    pub async fn create_task(
        &self,
        description: &str,
        assigned_pup: Option<&str>,
    ) -> Result<String> {
        self.memory.create_task(description, assigned_pup).await
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<TaskRecord>> {
        self.memory.list_tasks(limit).await
    }

    pub async fn update_task_status(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
    ) -> Result<()> {
        self.memory.update_task_status(id, status, result).await
    }

    pub async fn delete_task(&self, id: &str) -> Result<()> {
        self.memory.delete_task(id).await
    }

    pub async fn search_conversations(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<(String, String, i64)>> {
        self.memory.search_conversations(query, limit).await
    }

    pub async fn timeline_events(&self, limit: i64) -> Result<Vec<(String, String, i64)>> {
        self.memory.list_conversations_for_timeline(limit).await
    }

    pub async fn list_knowledge_sources(
        &self,
    ) -> Result<Vec<crate::knowledge::types::KnowledgeSource>> {
        self.memory.list_knowledge_sources().await
    }

    pub async fn ingest_knowledge_file<F>(
        &self,
        path: String,
        title: Option<String>,
        tags: Vec<String>,
        progress: F,
    ) -> Result<String>
    where
        F: Fn(crate::knowledge::types::IngestProgressEvent) + Send + Sync + 'static,
    {
        let ingestor = crate::knowledge::ingestor::Ingestor::with_llm(
            self.memory.clone(),
            self.alpha.llm_client.clone(),
        );
        let req = crate::knowledge::types::IngestRequest { path, title, tags };
        ingestor.ingest(&req, progress).await
    }

    pub async fn delete_knowledge_source(&self, source_id: &str) -> Result<()> {
        self.memory.delete_knowledge_source(source_id).await
    }

    pub fn kb_auto_ingest(&self) -> bool {
        self.alpha
            .kb_auto_ingest
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_kb_auto_ingest(&self, enabled: bool) {
        self.alpha
            .kb_auto_ingest
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn list_kg_entities(
        &self,
        entity_type: Option<&str>,
    ) -> Result<Vec<(String, String, String, Option<String>)>> {
        self.memory.list_kg_entities(entity_type).await
    }

    pub async fn kg_entity_relations(
        &self,
        id: &str,
    ) -> Result<Vec<(String, String, String, String, f32)>> {
        self.memory.kg_entity_relations(id).await
    }

    pub async fn list_pups(&self) -> Vec<crate::agents::alpha::PupConfig> {
        self.alpha.list_pup_configs().await
    }

    pub async fn run_skill(&self, name: &str, input: &str) -> Result<String> {
        self.skill_executor.execute_skill(name, input).await
    }

    pub async fn list_skills(&self) -> Vec<InstalledSkill> {
        self.skill_executor.registry.list_installed().await
    }

    pub async fn set_skill_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.skill_executor
            .registry
            .set_enabled(name, enabled)
            .await
    }

    pub fn scheduled_jobs_registry(&self) -> JobRegistry {
        JobRegistry::new(self.workspace_root.join("scheduled_jobs.json"))
    }

    pub fn list_scheduled_jobs(&self) -> Vec<ScheduledJob> {
        self.scheduled_jobs_registry().load()
    }

    pub fn delete_scheduled_job(&self, id: &str) -> Result<()> {
        self.scheduled_jobs_registry().delete(id)
    }

    pub async fn list_mcp_servers(&self) -> Vec<crate::mcp::orchestrator::McpServerEntry> {
        self.mcp_orchestrator.list_servers().await
    }

    pub async fn add_mcp_server(&self, entry: McpServerEntry) -> Result<()> {
        self.mcp_orchestrator.add_server(entry).await
    }

    pub async fn remove_mcp_server(&self, name: &str) -> Result<()> {
        self.mcp_orchestrator.remove_server(name).await
    }

    pub async fn toggle_mcp_server(&self, name: &str, enabled: bool) -> Result<()> {
        self.mcp_orchestrator.toggle_server(name, enabled).await
    }

    pub async fn list_mcp_tools(&self) -> Vec<McpToolInfo> {
        self.mcp_orchestrator.list_all_tools().await
    }

    pub async fn refresh_mcp_tools(&self) -> Vec<McpToolInfo> {
        self.mcp_orchestrator.refresh_all_tools().await;
        self.mcp_orchestrator.list_all_tools().await
    }

    pub async fn pup_conversation_display(
        &self,
        pup_key: &str,
        limit: i64,
        before_timestamp: Option<i64>,
    ) -> Result<Vec<(String, String, i64)>> {
        self.memory
            .get_pup_conversation_display(pup_key, limit, before_timestamp)
            .await
    }

    pub async fn pup_message_count(&self, pup_key: &str) -> Result<i64> {
        self.memory.get_pup_message_count(pup_key).await
    }

    pub async fn clear_pup_history(&self, pup_key: &str) -> Result<()> {
        self.memory.clear_pup_conversation(pup_key).await
    }

    pub async fn compress_pup_context(&self, pup_key: &str) -> Result<()> {
        self.alpha.compress_pup_context_now(pup_key).await
    }

    pub async fn context_tokens(&self, pup_key: &str) -> u64 {
        self.alpha.get_context_tokens(pup_key).await.unwrap_or(0)
    }

    pub fn context_limit(&self) -> u64 {
        self.alpha.get_context_limit()
    }

    pub async fn compression_status(&self, _pup_key: &str) -> Result<(bool, i64)> {
        self.memory.get_compression_status().await
    }

    pub async fn compact_pup_context(
        &self,
        _pup_key: &str,
        simulated_tokens: u64,
    ) -> Result<Vec<crate::memory::compaction::CompactionResult>> {
        self.alpha
            .compaction_engine
            .compact(simulated_tokens, self.alpha.get_context_limit())
            .await
    }

    pub async fn bridge_status(&self) -> Result<Vec<crate::bridge::types::BridgeConnectionStatus>> {
        crate::bridge::control::get_bridge_status(&self.memory).await
    }

    pub async fn list_channels(&self, limit: i64) -> Result<Vec<ChannelRecord>> {
        self.memory.list_channels(limit).await
    }

    pub async fn active_channel_count(&self) -> Result<i64> {
        self.channel_manager.active_count().await
    }

    pub async fn channel_messages(&self, channel_id: &str) -> Result<Vec<ChannelMessageRecord>> {
        self.memory.get_channel_messages(channel_id).await
    }

    pub async fn channel_plan(&self, channel_id: &str) -> Result<Option<DelegationPlan>> {
        self.memory.get_channel_plan(channel_id).await
    }

    pub async fn channel_workflow_state(
        &self,
        channel_id: &str,
    ) -> Result<Option<ChannelWorkflowState>> {
        self.channel_manager.workflow_state(channel_id).await
    }

    pub async fn submit_channel_review_comment(
        &self,
        channel_id: &str,
        sender: &str,
        comment: &str,
        reply_to: Option<&str>,
    ) -> Result<()> {
        self.channel_manager
            .submit_review_comment(channel_id, sender, comment, reply_to)
            .await
            .map(|_| ())
    }

    pub async fn request_channel_changes(
        &self,
        channel_id: &str,
        sender: &str,
        comment: &str,
        reply_to: Option<&str>,
    ) -> Result<()> {
        self.channel_manager
            .request_changes(channel_id, sender, comment, reply_to, None)
            .await
    }

    pub async fn abort_channel(&self, channel_id: &str, sender: &str, comment: &str) -> Result<()> {
        self.channel_manager
            .abort_channel(channel_id, sender, comment)
            .await
    }

    pub async fn continue_channel(
        &self,
        channel_id: &str,
        sender: &str,
        comment: &str,
    ) -> Result<()> {
        self.channel_manager
            .continue_channel(channel_id, sender, comment)
            .await
    }

    pub async fn clear_completed_channels(&self) -> Result<i64> {
        self.memory.clear_completed_channels().await
    }

    pub async fn clear_stale_channels(&self, max_age_seconds: i64) -> Result<i64> {
        self.memory
            .clear_stale_active_channels(max_age_seconds)
            .await
    }

    pub async fn kb_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<crate::knowledge::types::KbSearchResult>> {
        let retriever = crate::knowledge::retriever::KbRetriever::new(self.memory.clone());
        retriever.search(query, limit, None).await
    }
}

pub async fn build_app(
    workspace_root: PathBuf,
    capabilities: Arc<Capabilities>,
    permission_ui: Option<Arc<dyn PermissionUi>>,
    dynamic_pups: Vec<Arc<dyn SpecialistPup>>,
) -> Result<OpenPupApp> {
    let db_path = workspace_root.join("database.db");

    let file_layer = Arc::new(FileLayer::new(&workspace_root));
    file_layer
        .ensure_workspace_initialized()
        .with_context(|| format!("initialize workspace at {}", workspace_root.display()))?;

    let llm_client = Arc::new(LlmClient::new_from_env());
    {
        let cfg = crate::config::load_with_env();
        let provider = if cfg.llm.provider == "ollama" {
            Provider::Ollama
        } else {
            Provider::OpenAI
        };
        let api_key = if cfg.llm.api_key.is_empty() {
            None
        } else {
            Some(cfg.llm.api_key)
        };
        let api_base = if cfg.llm.api_base.is_empty() {
            None
        } else {
            Some(cfg.llm.api_base)
        };
        let mini = if cfg.llm.mini_model.is_empty() {
            None
        } else {
            Some(cfg.llm.mini_model)
        };
        let embed = if cfg.llm.embed_model.is_empty() {
            None
        } else {
            Some(cfg.llm.embed_model)
        };
        llm_client.reconfigure(provider, cfg.llm.model, mini, embed, api_key, api_base);
    }

    let memory = Arc::new(
        MemorySystem::new(
            db_path
                .to_str()
                .ok_or_else(|| anyhow!("invalid db path: {}", db_path.display()))?,
            llm_client.clone(),
        )
        .await
        .with_context(|| format!("initialize database at {}", db_path.display()))?,
    );

    let mcp_config_path = workspace_root.join("mcp_servers.json");
    let mcp_orchestrator = Arc::new(MCPOrchestrator::load(mcp_config_path));
    if let Ok(base_url) = std::env::var("OPENPUP_MCP_SERVER_URL") {
        let token = std::env::var("OPENPUP_MCP_TOKEN").unwrap_or_else(|_| "dev-token".to_string());
        let _ = mcp_orchestrator
            .add_server(McpServerEntry {
                name: "env".to_string(),
                base_url,
                token,
                description: "Server from OPENPUP_MCP_SERVER_URL".to_string(),
                enabled: true,
            })
            .await;
    }

    let skills_state_dir = workspace_root.join("skills_state");
    let skills_state_path = skills_state_dir.join("installed_skills.json");
    let skill_registry = SkillRegistry::new(skills_state_path);
    skill_registry
        .register_builtin(include_str!("../../skills/daily_summary.skill.toml"))
        .await;
    skill_registry
        .register_builtin(include_str!("../../skills/weekly_summary.skill.toml"))
        .await;
    skill_registry
        .register_builtin(include_str!("../../skills/task_manager.skill.toml"))
        .await;
    skill_registry
        .register_builtin(include_str!("../../skills/notify.skill.toml"))
        .await;

    let cfg = crate::config::load_with_env();
    let mut skill_scan_roots = Vec::new();
    skill_scan_roots.push(
        crate::config::app_root()
            .unwrap_or_else(|_| workspace_root.clone())
            .join("skills"),
    );
    for search_path in &cfg.skills.search_paths {
        let expanded_path = crate::config::expand_tilde(search_path);
        if !skill_scan_roots.iter().any(|p| p == &expanded_path) {
            skill_scan_roots.push(expanded_path);
        }
    }

    for scan_root in skill_scan_roots {
        let _ = skill_registry
            .register_from_dir(&scan_root, "local")
            .await;
        skill_registry.add_scan_root(scan_root, "local").await;
    }

    let permissions = PermissionChecker::new();
    permissions.set_persist_path(
        workspace_root
            .join("skills_state")
            .join("trusted_skills.json"),
    );
    permissions
        .set_mode(match cfg.app.execution_mode.to_lowercase().as_str() {
            "freerun" | "free_run" | "free-run" => ExecutionMode::FreeRun,
            _ => ExecutionMode::Leashed,
        })
        .await;
    if let Some(ui) = permission_ui {
        permissions.set_permission_ui(ui);
    }

    let bridge_outbox = crate::bridge::types::BridgeOutbox::default();
    let tools = Arc::new(ToolRegistry::new(
        workspace_root.clone(),
        memory.clone(),
        skill_registry.clone(),
        capabilities.clone(),
        bridge_outbox.clone(),
    ));
    tools.set_context_limit(crate::agents::alpha::infer_context_limit_for_model(
        &llm_client.model_name(),
    ));

    let skill_executor = Arc::new(SkillExecutor {
        registry: skill_registry,
        permissions: permissions.clone(),
        mcp: mcp_orchestrator.clone(),
        llm: llm_client.clone(),
        tools,
    });

    let channel_manager = Arc::new(ChannelManager::new(memory.clone()));
    let pup_config_path = workspace_root.join("pups_config.json");
    let alpha = Arc::new(AlphaPup::new(
        memory.clone(),
        llm_client,
        mcp_orchestrator.clone(),
        file_layer.clone(),
        skill_executor.clone(),
        Some(pup_config_path),
        channel_manager.clone(),
    ));

    alpha.register_pup(Arc::new(DevPup::new())).await;
    alpha.register_pup(Arc::new(WriterPup::new())).await;
    alpha.register_pup(Arc::new(OpsPup::new())).await;
    alpha.register_pup(Arc::new(LifeAdminPup::new())).await;
    alpha.register_pup(Arc::new(ResearchPup::new())).await;
    for pup in dynamic_pups {
        alpha.register_pup(pup).await;
    }
    alpha.init_msg_count().await;

    Ok(OpenPupApp {
        workspace_root,
        capabilities,
        alpha,
        memory,
        file_layer,
        permissions,
        skill_executor,
        mcp_orchestrator,
        channel_manager,
        bridge_outbox,
    })
}
