pub use openpup_core::agents;
pub use openpup_core::bridge;
pub use openpup_core::channel;
mod commands;
pub use openpup_core::config;
pub use openpup_core::crypto;
pub use openpup_core::knowledge;
pub use openpup_core::llm;
pub use openpup_core::mcp;
pub use openpup_core::memory;
pub use openpup_core::runtime;
mod runtime_tauri;
pub use openpup_core::skills;
pub use openpup_core::tools;
pub use openpup_core::workspace;

use std::sync::Arc;

use commands::AppState;
use openpup_core::app::OpenPupApp;
use openpup_core::skills::permissions::PermissionUi;
#[cfg(target_os = "android")]
use openpup_runtime_android::AndroidRuntimeFactory;
use openpup_runtime_desktop::DesktopRuntimeFactory;
#[cfg(target_os = "ios")]
use openpup_runtime_ios::IosRuntimeFactory;
use skills::scheduler::SkillScheduler;
use tokio::runtime::Runtime;

fn init_logging(workspace_root: &std::path::Path) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let filter =
            std::env::var("OPENPUP_LOG").unwrap_or_else(|_| "openpup_tauri=debug,warn".to_string());

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(false)
            .init();

        return;
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
    let log_dir = workspace_root.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "openpup.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    #[cfg(debug_assertions)]
    let writer = non_blocking.and(std::io::stderr);
    #[cfg(not(debug_assertions))]
    let writer = non_blocking;

    let filter =
        std::env::var("OPENPUP_LOG").unwrap_or_else(|_| "openpup_tauri=debug,warn".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    std::mem::forget(guard);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = dotenvy::dotenv();

    if let Err(err) = run_inner() {
        eprintln!("openpup startup failed: {err:#}");
    }
}

fn run_inner() -> anyhow::Result<()> {
    let workspace_root = platform_workspace_root()?;
    std::env::set_var("OPENPUP_APP_ROOT", &workspace_root);
    init_logging(&workspace_root);

    let rt = Runtime::new()?;
    let app = Arc::new(rt.block_on(async { build_platform_app(None).await })?);

    let permission_checker = app.permissions.clone();
    let channel_manager_for_setup = app.channel_manager.clone();
    let scheduler_for_setup = SkillScheduler {
        executor: app.skill_executor.clone(),
        memory: app.memory.clone(),
        permissions: app.permissions.clone(),
        file_layer: app.file_layer.clone(),
        jobs_path: app.workspace_root.join("scheduled_jobs.json"),
    };

    let weixin_service = Arc::new(bridge::weixin::WeixinService::new());
    let bridge_cfg = crate::config::load_with_env().bridge.unwrap_or_default();
    let bridge_manager = Arc::new(bridge::BridgeManager::new(
        bridge_cfg,
        app.alpha.clone(),
        weixin_service,
    ));

    let app_state = AppState {
        app: app.clone(),
        bridge_manager: bridge_manager.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(permission_checker.clone())
        .invoke_handler(tauri::generate_handler![
            commands::send_message,
            commands::abort_message,
            commands::check_onboarding_completed,
            commands::save_onboarding_data,
            commands::get_owner_profile,
            commands::list_skills,
            commands::set_skill_enabled,
            commands::run_skill,
            commands::list_mcp_servers,
            commands::add_mcp_server,
            commands::remove_mcp_server,
            commands::toggle_mcp_server,
            commands::export_workspace,
            commands::import_workspace,
            commands::list_long_term_memories,
            commands::update_long_term_memory,
            commands::delete_long_term_memory,
            commands::get_top_memories,
            commands::list_timeline_events,
            commands::list_diary_dates,
            commands::read_diary_entry,
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
            commands::list_pups,
            commands::update_pup,
            commands::add_custom_pup,
            commands::remove_custom_pup,
            commands::list_mcp_tools,
            commands::refresh_mcp_tools,
            commands::approve_permission,
            commands::deny_permission,
            commands::get_execution_mode,
            commands::set_execution_mode,
            commands::list_skill_runs,
            commands::search_conversations,
            commands::create_task,
            commands::list_tasks,
            commands::update_task_status,
            commands::delete_task,
            commands::list_scheduled_jobs,
            commands::delete_scheduled_job,
            commands::open_url,
            commands::list_channels,
            commands::get_channel_messages,
            commands::get_channel_plan,
            commands::get_channel_workflow_state,
            commands::submit_channel_review_comment,
            commands::request_channel_changes,
            commands::continue_channel,
            commands::abort_channel,
            commands::get_active_channel_count,
            commands::clear_completed_channels,
            commands::clear_stale_channels,
            commands::get_pup_conversation,
            commands::get_pup_message_count,
            commands::clear_pup_history,
            commands::compress_pup_context,
            commands::get_context_stats,
            commands::get_token_usage,
            commands::reset_token_usage,
            commands::compact_pup_context,
            commands::kb_ingest_file,
            commands::kb_list_sources,
            commands::kb_delete_source,
            commands::kb_search,
            commands::kb_get_auto_ingest,
            commands::kb_set_auto_ingest,
            commands::kg_list_entities,
            commands::submit_message_feedback,
            commands::save_artifact_to_file,
        ])
        .setup(move |app| {
            #[cfg(target_os = "windows")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_decorations(false);
                }
            }

            let event_sink = Arc::new(crate::runtime_tauri::TauriEventSink::new(
                app.handle().clone(),
            ));
            permission_checker.set_event_sink(event_sink.clone());
            channel_manager_for_setup.set_event_sink(event_sink.clone());
            tauri::async_runtime::spawn(async move {
                scheduler_for_setup.start(Some(event_sink));
                bridge_manager.clone().start();
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|err| anyhow::anyhow!("error while running openpup tauri application: {err}"))?;

    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn build_platform_app(
    permission_ui: Option<Arc<dyn PermissionUi>>,
) -> anyhow::Result<OpenPupApp> {
    DesktopRuntimeFactory::build_app(permission_ui).await
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn platform_workspace_root() -> anyhow::Result<std::path::PathBuf> {
    DesktopRuntimeFactory::workspace_root()
}

#[cfg(target_os = "android")]
async fn build_platform_app(
    permission_ui: Option<Arc<dyn PermissionUi>>,
) -> anyhow::Result<OpenPupApp> {
    AndroidRuntimeFactory::build_app(permission_ui).await
}

#[cfg(target_os = "android")]
fn platform_workspace_root() -> anyhow::Result<std::path::PathBuf> {
    AndroidRuntimeFactory::workspace_root()
}

#[cfg(target_os = "ios")]
async fn build_platform_app(
    permission_ui: Option<Arc<dyn PermissionUi>>,
) -> anyhow::Result<OpenPupApp> {
    IosRuntimeFactory::build_app(permission_ui).await
}

#[cfg(target_os = "ios")]
fn platform_workspace_root() -> anyhow::Result<std::path::PathBuf> {
    IosRuntimeFactory::workspace_root()
}
