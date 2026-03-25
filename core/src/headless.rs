use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::agents::alpha::AlphaPup;
use crate::agents::dev_pup::DevPup;
use crate::agents::life_admin_pup::LifeAdminPup;
use crate::agents::ops_pup::OpsPup;
use crate::agents::plugins::load_dynamic_pups;
use crate::agents::research_pup::ResearchPup;
use crate::agents::writer_pup::WriterPup;
use crate::channel::manager::ChannelManager;
use crate::llm::client::{LlmClient, Provider};
use crate::mcp::orchestrator::{MCPOrchestrator, McpServerEntry};
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::skills::executor::SkillExecutor;
use crate::skills::permissions::{ExecutionMode, PermissionChecker, PermissionUi};
use crate::skills::registry::SkillRegistry;
use crate::tools::primitive::ToolRegistry;

#[derive(Clone)]
pub struct HeadlessRuntime {
    pub workspace_root: PathBuf,
    pub alpha: Arc<AlphaPup>,
    pub memory: Arc<MemorySystem>,
    pub file_layer: Arc<FileLayer>,
    pub permissions: PermissionChecker,
    pub skill_executor: Arc<SkillExecutor>,
    pub mcp_orchestrator: Arc<MCPOrchestrator>,
    pub channel_manager: Arc<ChannelManager>,
}

impl HeadlessRuntime {
    pub async fn new(permission_ui: Option<Arc<dyn PermissionUi>>) -> Result<Self> {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
        let workspace_root = home_dir.join(".openpup");
        let db_path = workspace_root.join("database.db");

        let file_layer = Arc::new(FileLayer::new(&workspace_root));
        file_layer.ensure_workspace_initialized()?;

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
            .await?,
        );

        let mcp_config_path = workspace_root.join("mcp_servers.json");
        let mcp_orchestrator = Arc::new(MCPOrchestrator::load(mcp_config_path));
        if let Ok(base_url) = std::env::var("OPENPUP_MCP_SERVER_URL") {
            let token =
                std::env::var("OPENPUP_MCP_TOKEN").unwrap_or_else(|_| "dev-token".to_string());
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
            .register_builtin(include_str!("../../skills/daily_summary.toml"))
            .await;
        skill_registry
            .register_builtin(include_str!("../../skills/weekly_summary.toml"))
            .await;
        skill_registry
            .register_builtin(include_str!("../../skills/task_manager.toml"))
            .await;
        let cfg = crate::config::load_with_env();
        for search_path in &cfg.skills.search_paths {
            let expanded_path = crate::config::expand_tilde(search_path);
            let _ = skill_registry
                .register_from_dir(&expanded_path, "local")
                .await;
            skill_registry.add_scan_root(expanded_path, "local").await;
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

        let tools = Arc::new(ToolRegistry::new(workspace_root.clone(), memory.clone()));
        tools.set_context_limit(
            crate::agents::alpha::infer_context_limit_for_model(&llm_client.model_name()),
        );
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
        for pup in load_dynamic_pups() {
            alpha.register_pup(pup).await;
        }
        alpha.init_msg_count().await;

        Ok(Self {
            workspace_root,
            alpha,
            memory,
            file_layer,
            permissions,
            skill_executor,
            mcp_orchestrator,
            channel_manager,
        })
    }
}
