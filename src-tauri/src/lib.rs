pub use openpup_core::agents;
pub use openpup_core::bridge;
pub use openpup_core::channel;
mod commands;
pub use openpup_core::config;
pub use openpup_core::conversation;
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
#[cfg(any(target_os = "android", target_os = "ios"))]
use openpup_core::app::OpenPupApp;
#[cfg(any(target_os = "android", target_os = "ios"))]
use openpup_core::skills::permissions::PermissionUi;
use openpup_core::xmtp_helper::{XmtpHelperConfig, XmtpNodeHelper};
#[cfg(target_os = "android")]
use openpup_runtime_android::AndroidRuntimeFactory;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use openpup_runtime_desktop::DesktopRuntimeFactory;
#[cfg(target_os = "ios")]
use openpup_runtime_ios::IosRuntimeFactory;
use serde::{Deserialize, Serialize};
use skills::scheduler::SkillScheduler;
use tokio::runtime::Runtime;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct XmtpHelperMessagePayload {
    transport_ref: String,
    remote_message_id: String,
    envelope: Option<conversation::types::AgentChatEnvelope>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct XmtpHelperGroupPayload {
    pub transport_ref: String,
    pub title: String,
    pub description: String,
    pub created_at: Option<i64>,
    pub added_by_inbox_id: Option<String>,
}

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
            .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
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

macro_rules! all_commands {
    () => {
        tauri::generate_handler![
            commands::send_message,
            commands::abort_message,
            commands::check_onboarding_completed,
            commands::save_onboarding_data,
            commands::get_owner_profile,
            commands::list_skills,
            commands::refresh_skills,
            commands::set_skill_enabled,
            commands::run_skill,
            commands::list_mcp_servers,
            commands::add_mcp_server,
            commands::update_mcp_server,
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
            commands::toggle_scheduled_job,
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
            commands::list_conversation_spaces,
            commands::create_conversation_space,
            commands::find_conversation_space,
            commands::get_conversation_members,
            commands::add_conversation_member,
            commands::remove_conversation_member,
            commands::delete_conversation_space,
            commands::get_conversation_messages,
            commands::post_conversation_message,
            commands::get_xmtp_helper_status,
            commands::get_xmtp_identity,
            commands::enable_xmtp_for_conversation,
            commands::add_xmtp_conversation_member,
            commands::remove_xmtp_conversation_member,
            commands::leave_xmtp_conversation,
            commands::sync_xmtp_groups,
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
        ]
    };
}

fn run_inner() -> anyhow::Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return run_mobile();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return run_desktop();
}

fn xmtp_helper_config() -> XmtpHelperConfig {
    let node_bin = std::env::var_os("OPENPUP_NODE_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("node"));
    let script_path = std::env::var_os("OPENPUP_XMTP_HELPER_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest_dir
                .parent()
                .unwrap_or(manifest_dir.as_path())
                .join("xmtp-helper")
                .join("dist")
                .join("index.js")
        });
    XmtpHelperConfig {
        node_bin,
        script_path,
    }
}

fn spawn_xmtp_event_pump(app: Arc<openpup_core::app::OpenPupApp>, helper: Arc<XmtpNodeHelper>) {
    let mut rx = helper.subscribe_events();
    tauri::async_runtime::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let result = match event.event.as_str() {
                "message" => import_xmtp_message(app.clone(), event.payload).await,
                "group" => import_xmtp_group(app.clone(), event.payload)
                    .await
                    .map(|_| ()),
                "stream" => {
                    tracing::info!("[xmtp-helper] stream status: {}", event.payload);
                    app.emit_xmtp_stream_status(event.payload);
                    Ok(())
                }
                _ => continue,
            };
            if let Err(e) = result {
                tracing::warn!("[xmtp-helper] import event failed: {e}");
            }
        }
    });
}

pub(crate) async fn import_xmtp_group(
    app: Arc<openpup_core::app::OpenPupApp>,
    payload: serde_json::Value,
) -> anyhow::Result<String> {
    let payload: XmtpHelperGroupPayload = serde_json::from_value(payload)?;
    if let Some(conversation_id) = app
        .memory
        .find_conversation_by_transport("xmtp", &payload.transport_ref)
        .await?
    {
        return Ok(conversation_id);
    }

    let title = if payload.title.trim().is_empty() {
        "XMTP 群聊"
    } else {
        payload.title.trim()
    };
    let description = if payload.description.trim().is_empty() {
        "XMTP remote group"
    } else {
        payload.description.trim()
    };
    let space = app
        .memory
        .create_conversation_space(title, Some(description), Some("owner"), Some("#378ADD"))
        .await?;
    app.memory
        .bind_conversation_transport(&space.id, "xmtp", "XMTP", &payload.transport_ref)
        .await?;
    app.emit_conversation_spaces_changed().await;
    Ok(space.id)
}

async fn import_xmtp_message(
    app: Arc<openpup_core::app::OpenPupApp>,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let payload: XmtpHelperMessagePayload = serde_json::from_value(payload)?;
    if app
        .memory
        .xmtp_message_seen(&payload.transport_ref, &payload.remote_message_id)
        .await?
    {
        return Ok(());
    }

    let conversation_id = match app
        .memory
        .find_conversation_by_transport("xmtp", &payload.transport_ref)
        .await?
    {
        Some(conversation_id) => conversation_id,
        None => {
            import_xmtp_group(
                app.clone(),
                serde_json::json!({
                    "transportRef": payload.transport_ref,
                    "title": "XMTP 群聊",
                    "description": "",
                    "createdAt": chrono::Utc::now().timestamp(),
                    "addedByInboxId": "",
                }),
            )
            .await?
        }
    };

    let Some(envelope) = payload.envelope else {
        return Ok(());
    };
    let sender = &envelope.sender;
    let content = envelope.content.text.clone();
    if content.trim().is_empty() {
        return Ok(());
    }

    let inbox_id = sender.transport.inbox_id.as_str();
    let client_kind = sender.client.kind.as_str();
    let client_instance_id = sender.client.instance_id.as_str();
    let client_display_name = if sender.client.display_name.trim().is_empty() {
        "XMTP Client"
    } else {
        sender.client.display_name.as_str()
    };
    let actor_kind = sender.actor.kind.as_str();
    let actor_id = sender.actor.actor_id.as_str();
    let actor_display_name = if sender.actor.display_name.trim().is_empty() {
        "Human"
    } else {
        sender.actor.display_name.as_str()
    };
    let agent_key = sender.actor.agent_key.as_deref();
    let via_kind = sender.via.as_ref().map(|via| via.kind.as_str());
    let via_label = sender.via.as_ref().map(|via| via.label.as_str());
    let display_name = display_name_for_remote_actor(
        client_display_name,
        actor_kind,
        actor_display_name,
        agent_key,
    );
    let route_label = route_label_for_remote_actor(client_kind, via_label);
    let identity_id = format!("xmtp:{inbox_id}:{client_instance_id}:{actor_id}");
    let mention_key = mention_key_for_remote_actor(client_instance_id, actor_id);

    app.memory
        .upsert_network_identity(
            "xmtp",
            inbox_id,
            &display_name,
            actor_kind,
            client_kind,
            agent_key,
        )
        .await?;
    app.memory
        .ensure_conversation_member_with_sender_meta(
            &conversation_id,
            &identity_id,
            &display_name,
            Some(&mention_key),
            &route_label,
            if actor_kind == "agent" {
                "agent"
            } else {
                "member"
            },
            Some("xmtp"),
            Some(inbox_id),
            Some(client_kind),
            Some(client_instance_id),
            Some(client_display_name),
            Some(actor_kind),
            Some(actor_id),
            Some(actor_display_name),
            via_kind,
            via_label,
        )
        .await?;
    app.emit_conversation_members_changed(&conversation_id)
        .await;

    let message = app
        .memory
        .post_conversation_message_with_sender_meta(
            &conversation_id,
            &identity_id,
            Some(&payload.remote_message_id),
            &display_name,
            if actor_kind == "agent" {
                "agent"
            } else {
                "human"
            },
            Some(&route_label),
            &content,
            Some("xmtp"),
            Some(inbox_id),
            Some(client_kind),
            Some(client_instance_id),
            Some(client_display_name),
            Some(actor_kind),
            Some(actor_id),
            Some(actor_display_name),
            via_kind,
            via_label,
        )
        .await?;
    app.memory
        .insert_xmtp_message_map(
            &message.id,
            &payload.remote_message_id,
            &payload.transport_ref,
            "inbound",
        )
        .await?;
    app.emit_conversation_message_created(message);
    app.emit_conversation_spaces_changed().await;
    Ok(())
}

fn display_name_for_remote_actor(
    client_display_name: &str,
    actor_kind: &str,
    actor_display_name: &str,
    agent_key: Option<&str>,
) -> String {
    if actor_kind == "agent" {
        let actor = agent_key
            .map(|key| if key == "alpha" { "Alpha" } else { key })
            .unwrap_or(actor_display_name);
        format!("{actor} · {client_display_name}")
    } else if actor_kind == "human" {
        format!("{client_display_name} / {actor_display_name}")
    } else {
        format!("{client_display_name} / {actor_display_name}")
    }
}

fn route_label_for_remote_actor(client_kind: &str, via_label: Option<&str>) -> String {
    match via_label {
        Some(label) if !label.trim().is_empty() => format!("xmtp · {client_kind} · {label}"),
        _ => format!("xmtp · {client_kind}"),
    }
}

fn mention_key_for_remote_actor(client_instance_id: &str, actor_id: &str) -> String {
    let client = client_instance_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let actor = actor_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if client.is_empty() {
        actor
    } else if actor.is_empty() {
        client
    } else {
        format!("{client}/{actor}")
    }
}

/// Desktop startup: resolve workspace root up-front (dirs crate works fine),
/// build the OpenPupApp, then launch Tauri.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_desktop() -> anyhow::Result<()> {
    let workspace_root = DesktopRuntimeFactory::workspace_root()?;
    std::env::set_var("OPENPUP_APP_ROOT", &workspace_root);
    init_logging(&workspace_root);

    let rt = Runtime::new()?;
    let app = Arc::new(rt.block_on(async { DesktopRuntimeFactory::build_app(None).await })?);

    let permission_checker = app.permissions.clone();
    let channel_manager_for_setup = app.channel_manager.clone();
    let scheduler_for_setup = SkillScheduler {
        executor: app.skill_executor.clone(),
        memory: app.memory.clone(),
        permissions: app.permissions.clone(),
        file_layer: app.file_layer.clone(),
        jobs_path: app.workspace_root.join("scheduled_jobs.json"),
        bridge_outbox: Some(app.bridge_outbox.clone()),
    };

    let weixin_service = Arc::new(bridge::weixin::WeixinService::new());
    let bridge_cfg = crate::config::load_with_env().bridge.unwrap_or_default();
    let xmtp_helper = Arc::new(XmtpNodeHelper::new(xmtp_helper_config()));
    let bridge_manager = Arc::new(bridge::BridgeManager::new(
        bridge_cfg,
        app.alpha.clone(),
        app.workspace_root.clone(),
        weixin_service,
        app.bridge_outbox.clone(),
        Some(xmtp_helper.clone()),
    ));

    let app_state = AppState {
        app: app.clone(),
        bridge_manager: bridge_manager.clone(),
        xmtp_helper: xmtp_helper.clone(),
    };
    let app_for_setup = app.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(permission_checker.clone())
        .invoke_handler(all_commands!())
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
            app_for_setup.set_conversation_event_sink(event_sink.clone());
            bridge_manager.set_event_sink(event_sink.clone());
            spawn_xmtp_event_pump(app_for_setup.clone(), xmtp_helper.clone());
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

/// Mobile startup — three phases:
///
/// 1. `build()` — initialise the Tauri app / Android Context.  On Android
///    this triggers `Context.getFilesDir()` internally, which **creates**
///    the app-private `files/` directory if it doesn't already exist.
///    Without this step our UID-based path guess may point to a directory
///    that hasn't been created yet.
///
/// 2. Resolve the workspace root from Tauri's path resolver (guaranteed
///    correct), then do heavy initialisation (SQLite, config, skills …)
///    on the current thread.
///
/// 3. `app.run()` — start the event loop.  By this point all state has
///    been managed, so the WebView can invoke commands immediately.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn run_mobile() -> anyhow::Result<()> {
    // On mobile, init_logging only writes to stderr — no workspace needed.
    init_logging(std::path::Path::new(""));

    // ── Phase 1: build Tauri (creates Android Context / iOS sandbox) ────
    let tauri_app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(all_commands!())
        .build(tauri::generate_context!())
        .map_err(|err| anyhow::anyhow!("failed to build tauri app: {err}"))?;

    // ── Phase 2: resolve path & heavy init ──────────────────────────────
    use tauri::Manager;
    let workspace_root = tauri_app
        .path()
        .app_local_data_dir()
        .map(|p| p.join("openpup-mobile"))
        .or_else(|_| mobile_workspace_root_fallback())?;

    std::env::set_var("OPENPUP_APP_ROOT", &workspace_root);

    let rt = Runtime::new()?;
    let app = Arc::new(rt.block_on(async { build_mobile_app(workspace_root, None).await })?);

    let permission_checker = app.permissions.clone();
    let channel_manager_for_setup = app.channel_manager.clone();
    let scheduler_for_setup = SkillScheduler {
        executor: app.skill_executor.clone(),
        memory: app.memory.clone(),
        permissions: app.permissions.clone(),
        file_layer: app.file_layer.clone(),
        jobs_path: app.workspace_root.join("scheduled_jobs.json"),
        bridge_outbox: Some(app.bridge_outbox.clone()),
    };

    let weixin_service = Arc::new(bridge::weixin::WeixinService::new());
    let bridge_cfg = crate::config::load_with_env().bridge.unwrap_or_default();
    let xmtp_helper = Arc::new(XmtpNodeHelper::new(xmtp_helper_config()));
    let bridge_manager = Arc::new(bridge::BridgeManager::new(
        bridge_cfg,
        app.alpha.clone(),
        app.workspace_root.clone(),
        weixin_service,
        app.bridge_outbox.clone(),
        Some(xmtp_helper.clone()),
    ));

    let app_state = AppState {
        app: app.clone(),
        bridge_manager: bridge_manager.clone(),
        xmtp_helper: xmtp_helper.clone(),
    };
    let app_for_setup = app.clone();

    tauri_app.manage(app_state);
    tauri_app.manage(permission_checker.clone());

    let event_sink = Arc::new(crate::runtime_tauri::TauriEventSink::new(
        tauri_app.handle().clone(),
    ));
    permission_checker.set_event_sink(event_sink.clone());
    channel_manager_for_setup.set_event_sink(event_sink.clone());
    app_for_setup.set_conversation_event_sink(event_sink.clone());
    bridge_manager.set_event_sink(event_sink.clone());
    spawn_xmtp_event_pump(app_for_setup.clone(), xmtp_helper.clone());
    tauri::async_runtime::spawn(async move {
        scheduler_for_setup.start(Some(event_sink));
        bridge_manager.clone().start();
    });

    // ── Phase 3: run event loop ─────────────────────────────────────────
    tauri_app.run(|_, _| {});

    Ok(())
}

#[cfg(target_os = "android")]
async fn build_mobile_app(
    workspace_root: std::path::PathBuf,
    permission_ui: Option<Arc<dyn PermissionUi>>,
) -> anyhow::Result<OpenPupApp> {
    AndroidRuntimeFactory::build_app_with_root(workspace_root, permission_ui).await
}

#[cfg(target_os = "ios")]
async fn build_mobile_app(
    workspace_root: std::path::PathBuf,
    permission_ui: Option<Arc<dyn PermissionUi>>,
) -> anyhow::Result<OpenPupApp> {
    IosRuntimeFactory::build_app_with_root(workspace_root, permission_ui).await
}

/// Fallback workspace root for mobile when Tauri's path resolver fails.
#[cfg(target_os = "android")]
fn mobile_workspace_root_fallback() -> anyhow::Result<std::path::PathBuf> {
    AndroidRuntimeFactory::workspace_root()
}

#[cfg(target_os = "ios")]
fn mobile_workspace_root_fallback() -> anyhow::Result<std::path::PathBuf> {
    IosRuntimeFactory::workspace_root()
}
