#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod bridge;
mod channel;
mod commands;
mod config;
mod crypto;
mod llm;
mod mcp;
mod memory;
mod runtime;
mod runtime_tauri;
mod skills;
mod tools;
mod workspace;

use std::sync::Arc;

use tracing::{info, warn};

use agents::alpha::AlphaPup;
use agents::dev_pup::DevPup;
use agents::life_admin_pup::LifeAdminPup;
use agents::ops_pup::OpsPup;
use agents::plugins::load_dynamic_pups;
use agents::research_pup::ResearchPup;
use agents::writer_pup::WriterPup;
use channel::manager::ChannelManager;
use commands::AppState;
use llm::client::LlmClient;
use mcp::orchestrator::{MCPOrchestrator, McpServerEntry};
use memory::file_layer::FileLayer;
use memory::system::MemorySystem;
use skills::executor::SkillExecutor;
use skills::permissions::PermissionChecker;
use skills::registry::SkillRegistry;
use skills::scheduler::SkillScheduler;
use tokio::runtime::Runtime;
use tools::primitive::ToolRegistry;

fn init_logging(workspace_root: &std::path::Path) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let log_dir = workspace_root.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    // Non-blocking file appender — rotates daily, keeps ~7 files
    let file_appender = tracing_appender::rolling::daily(&log_dir, "openpup.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // In debug builds also write to stderr; in release only to file.
    #[cfg(debug_assertions)]
    let writer = non_blocking.and(std::io::stderr);
    #[cfg(not(debug_assertions))]
    let writer = non_blocking;

    let filter =
        std::env::var("OPENPUP_LOG").unwrap_or_else(|_| "openpup_tauri=debug,warn".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(writer)
        .with_ansi(false) // file logs: no ANSI escape codes
        .with_target(true)
        .with_thread_ids(false)
        .init();

    // Keep the guard alive for the lifetime of the process
    std::mem::forget(guard);
}

fn main() {
    let _ = dotenvy::dotenv();

    // Initialise logging before anything else so startup messages are captured.
    let home_dir = dirs::home_dir().expect("cannot determine home directory");
    init_logging(&home_dir.join(".openpup"));

    let rt = Runtime::new().expect("failed to create tokio runtime");

    let (alpha, file_layer, permission_checker, scheduler, channel_manager, alpha_for_bridge) = rt
        .block_on(async {
            let home_dir = dirs::home_dir().expect("cannot determine home directory");
            let workspace_root = home_dir.join(".openpup");
            let db_path = workspace_root.join("database.db");

            let file_layer = Arc::new(FileLayer::new(&workspace_root));
            file_layer
                .ensure_workspace_initialized()
                .expect("ensure workspace");

            if let Some(parent) = db_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    panic!("failed to create db directory {:?}: {e}", parent);
                }
            }

            let db_path_str = db_path
                .to_str()
                .unwrap_or_else(|| panic!("invalid db path: {:?}", db_path));

            info!("db_path: {db_path_str}");
            let llm_client = Arc::new(LlmClient::new_from_env());

            // Load unified config.toml
            {
                let cfg = crate::config::load_with_env();
                let provider = if cfg.llm.provider == "ollama" {
                    crate::llm::client::Provider::Ollama
                } else {
                    crate::llm::client::Provider::OpenAI
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
                MemorySystem::new(db_path_str, llm_client.clone())
                    .await
                    .expect("init memory system"),
            );

            // MCP — load persisted server config
            let mcp_config_path = workspace_root.join("mcp_servers.json");
            let mcp_orchestrator = Arc::new(MCPOrchestrator::load(mcp_config_path));

            // Optional env-var bootstrap for first-run
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

            // Skills registry — load persisted + built-ins
            let skills_state_dir = workspace_root.join("skills_state");
            let skills_state_path = skills_state_dir.join("installed_skills.json");
            let skill_registry = SkillRegistry::new(skills_state_path);

            // Register built-in skills (always available, user can disable but not uninstall)
            skill_registry
                .register_builtin(include_str!("../../skills/daily_summary.toml"))
                .await;
            skill_registry
                .register_builtin(include_str!("../../skills/weekly_summary.toml"))
                .await;
            skill_registry
                .register_builtin(include_str!("../../skills/task_manager.toml"))
                .await;

            // Load user-configured skill search paths from config.toml
            let cfg = crate::config::load_with_env();
            for search_path in &cfg.skills.search_paths {
                let expanded_path = crate::config::expand_tilde(search_path);
                if let Err(e) = skill_registry
                    .register_from_dir(&expanded_path, "local")
                    .await
                {
                    warn!(
                        "failed to load skills from {}: {e}",
                        expanded_path.display()
                    );
                }
                skill_registry.add_scan_root(expanded_path, "local").await;
            }

            // Single shared PermissionChecker — all clones share the same Arc-wrapped state
            let permission_checker = PermissionChecker::new();
            permission_checker.set_persist_path(
                workspace_root
                    .join("skills_state")
                    .join("trusted_skills.json"),
            );

            // ToolRegistry — primitive tool execution surface for skills
            let tool_registry = Arc::new(ToolRegistry::new(workspace_root.clone(), memory.clone()));
            // Propagate model context limit so tool results are truncated proportionally
            tool_registry.set_context_limit(
                crate::agents::alpha::infer_context_limit_for_model(&llm_client.model_name()),
            );

            // SkillExecutor — shares the same PermissionChecker clone
            let skill_executor = Arc::new(SkillExecutor {
                registry: skill_registry,
                permissions: permission_checker.clone(),
                mcp: mcp_orchestrator.clone(),
                llm: llm_client.clone(),
                tools: tool_registry,
            });

            let pup_config_path = workspace_root.join("pups_config.json");
            let channel_manager = Arc::new(ChannelManager::new(memory.clone()));
            let alpha = Arc::new(AlphaPup::new(
                memory.clone(),
                llm_client.clone(),
                mcp_orchestrator.clone(),
                file_layer.clone(),
                skill_executor.clone(),
                Some(pup_config_path),
                channel_manager.clone(),
            ));

            // Register built-in specialist pups
            alpha.register_pup(Arc::new(DevPup::new())).await;
            alpha.register_pup(Arc::new(WriterPup::new())).await;
            alpha.register_pup(Arc::new(OpsPup::new())).await;
            alpha.register_pup(Arc::new(LifeAdminPup::new())).await;
            alpha.register_pup(Arc::new(ResearchPup::new())).await;

            // Load dynamic plugins from ~/.openpup/plugins
            for pup in load_dynamic_pups() {
                alpha.register_pup(pup).await;
            }

            // Seed the message counter from DB so memory extraction survives restarts
            alpha.init_msg_count().await;

            // Scheduler — owns clones of executor/memory/permissions/file_layer
            let jobs_path = workspace_root.join("scheduled_jobs.json");
            let scheduler = SkillScheduler {
                executor: skill_executor,
                memory: memory.clone(),
                permissions: permission_checker.clone(),
                file_layer: file_layer.clone(),
                jobs_path,
            };

            let alpha_for_bridge = alpha.clone();
            (
                alpha,
                file_layer,
                permission_checker,
                scheduler,
                channel_manager,
                alpha_for_bridge,
            )
        });

    let checker_for_setup = permission_checker.clone();
    // Scheduler is moved into setup closure where AppHandle is available
    let scheduler_for_setup = scheduler;
    let channel_manager_for_setup = channel_manager;

    // Bridge manager — started in setup closure (only if configured)
    let weixin_service = Arc::new(bridge::weixin::WeixinService::new());
    let bridge_cfg = crate::config::load_with_env().bridge.unwrap_or_default();
    let bridge_manager = Arc::new(bridge::BridgeManager::new(
        bridge_cfg,
        alpha_for_bridge,
        weixin_service,
    ));
    let app_state = AppState {
        alpha,
        file_layer,
        bridge_manager: bridge_manager.clone(),
    };

    tauri::Builder::default()
        .manage(app_state)
        .manage(permission_checker)
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::abort_message,
            commands::check_onboarding_completed,
            commands::save_onboarding_data,
            commands::get_owner_profile,
            // Skills
            commands::list_skills,
            commands::set_skill_enabled,
            commands::run_skill,
            // MCP servers
            commands::list_mcp_servers,
            commands::add_mcp_server,
            commands::remove_mcp_server,
            commands::toggle_mcp_server,
            // Workspace backup
            commands::export_workspace,
            commands::import_workspace,
            // Memory
            commands::list_long_term_memories,
            commands::update_long_term_memory,
            commands::delete_long_term_memory,
            commands::get_top_memories,
            commands::list_timeline_events,
            // Diary
            commands::list_diary_dates,
            commands::read_diary_entry,
            // Config
            commands::get_llm_provider,
            commands::get_llm_config,
            commands::get_safe_config,
            commands::set_llm_provider,
            commands::quick_set_model,
            commands::get_bridge_config,
            commands::save_bridge_config,
            commands::get_bridge_status,
            commands::start_weixin_qr_login,
            commands::wait_weixin_qr_login,
            commands::cancel_weixin_qr_login,
            commands::list_weixin_accounts,
            commands::activate_weixin_account,
            // Pup management
            commands::list_pups,
            commands::update_pup,
            commands::add_custom_pup,
            commands::remove_custom_pup,
            // MCP tools
            commands::list_mcp_tools,
            commands::refresh_mcp_tools,
            // Permissions
            commands::approve_permission,
            commands::deny_permission,
            commands::get_execution_mode,
            commands::set_execution_mode,
            // Skill history
            commands::list_skill_runs,
            // Conversation search
            commands::search_conversations,
            // Task tracking
            commands::create_task,
            commands::list_tasks,
            commands::update_task_status,
            commands::delete_task,
            // Scheduled jobs
            commands::list_scheduled_jobs,
            commands::delete_scheduled_job,
            // System
            commands::open_url,
            // Pack Channel
            commands::list_channels,
            commands::get_channel_messages,
            commands::get_channel_plan,
            commands::get_channel_workflow_state,
            commands::submit_channel_review_comment,
            commands::request_channel_changes,
            commands::continue_channel,
            commands::get_active_channel_count,
            commands::clear_completed_channels,
            commands::clear_stale_channels,
            // Pack page per-pup conversation
            commands::get_pup_conversation,
            commands::get_pup_message_count,
            commands::clear_pup_history,
            commands::compress_pup_context,
            commands::get_context_stats,
            commands::get_token_usage,
            commands::reset_token_usage,
        ])
        .setup(move |app| {
            let event_sink = Arc::new(crate::runtime_tauri::TauriEventSink::new(
                app.handle().clone(),
            ));
            checker_for_setup.set_event_sink(event_sink.clone());
            channel_manager_for_setup.set_event_sink(event_sink.clone());
            tauri::async_runtime::spawn(async move {
                scheduler_for_setup.start(Some(event_sink));
                bridge_manager.clone().start();
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running openpup tauri application");
}
