use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use openpup_core::bridge::control as bridge_control;
use openpup_core::bridge::BridgeManager;
use openpup_core::headless::HeadlessRuntime;
use openpup_core::ipc::{
    daemon_log_path, daemon_pid_path, daemon_runtime_dir, daemon_socket_path, ChannelDetails,
    DaemonEvent, DaemonRequest, DaemonResponse, DaemonStatus,
};
use openpup_core::runtime::EventSink;
use openpup_core::skills::scheduler::SkillScheduler;
use openpup_runtime_desktop::DesktopRuntimeFactory;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};
use tracing::{error, info};

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

#[cfg(not(unix))]
use tokio::net::{TcpListener, TcpStream};

#[derive(Parser)]
#[command(name = "openpupd", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Serve,
}

fn init_logging(log_path: &Path) {
    let Some(log_dir) = log_path.parent() else {
        return;
    };
    let Some(file_name) = log_path.file_name().and_then(|name| name.to_str()) else {
        return;
    };

    let _ = std::fs::create_dir_all(log_dir);

    let file_appender = tracing_appender::rolling::never(log_dir, file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = std::env::var("OPENPUP_LOG")
        .unwrap_or_else(|_| "openpup_core=debug,openpup_daemon=debug,warn".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .init();

    std::mem::forget(guard);
}

#[derive(Clone)]
struct DaemonState {
    runtime: HeadlessRuntime,
    bridge_manager: Arc<BridgeManager>,
    started_at: i64,
    socket_path: String,
    log_path: String,
}

struct SocketEventSink {
    tx: mpsc::UnboundedSender<DaemonEvent>,
}

impl EventSink for SocketEventSink {
    fn emit_value(&self, event: &str, payload: Value) {
        let daemon_event =
            match event {
                "stream_token" => payload.as_str().map(|token| DaemonEvent::Token {
                    token: token.to_string(),
                }),
                "stream_activity" => payload.get("label").and_then(Value::as_str).map(|label| {
                    DaemonEvent::Activity {
                        label: label.to_string(),
                    }
                }),
                "stream_done" => payload
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|content| DaemonEvent::Done {
                        content: content.to_string(),
                    }),
                "stream_error" => payload.as_str().map(|message| DaemonEvent::Error {
                    message: message.to_string(),
                }),
                _ => None,
            };

        if let Some(event) = daemon_event {
            let _ = self.tx.send(event);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => serve().await,
    }
}

async fn serve() -> Result<()> {
    let run_dir = daemon_runtime_dir();
    std::fs::create_dir_all(&run_dir)
        .with_context(|| format!("failed to create runtime dir `{}`", run_dir.display()))?;

    let socket_path = daemon_socket_path();
    let pid_path = daemon_pid_path();
    let log_path = daemon_log_path();

    init_logging(&log_path);
    info!("daemon_log_path: {}", log_path.display());

    write_pid_file(&pid_path)?;
    let listener = bind_listener(&socket_path).await?;

    let runtime = DesktopRuntimeFactory::build_headless(None).await?;
    let bridge_config = bridge_control::get_bridge_config();
    let weixin_service = Arc::new(openpup_core::bridge::weixin::WeixinService::new());
    let bridge_manager = Arc::new(BridgeManager::new(
        bridge_config,
        runtime.alpha.clone(),
        runtime.workspace_root.clone(),
        weixin_service,
        runtime.app.bridge_outbox.clone(),
        None,
    ));
    bridge_manager.clone().start();

    let scheduler = SkillScheduler {
        executor: runtime.skill_executor.clone(),
        memory: runtime.memory.clone(),
        permissions: runtime.permissions.clone(),
        file_layer: runtime.file_layer.clone(),
        jobs_path: runtime.workspace_root.join("scheduled_jobs.json"),
        bridge_outbox: Some(runtime.app.bridge_outbox.clone()),
    };
    scheduler.start(None);

    let state = Arc::new(DaemonState {
        runtime,
        bridge_manager,
        started_at: chrono::Utc::now().timestamp(),
        socket_path: socket_path.display().to_string(),
        log_path: log_path.display().to_string(),
    });

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    loop {
        #[cfg(unix)]
        let accept_fut = listener.accept();
        #[cfg(not(unix))]
        let accept_fut = listener.accept();

        tokio::select! {
            accept_result = accept_fut => {
                let stream = accept_result?;
                let state = state.clone();
                let shutdown_tx = shutdown_tx.clone();
                #[cfg(unix)]
                tokio::spawn(async move {
                    let (stream, _) = stream;
                    if let Err(error) = handle_stream(stream, state, shutdown_tx).await {
                        error!("[openpupd] connection error: {error}");
                    }
                });
                #[cfg(not(unix))]
                tokio::spawn(async move {
                    let (stream, _) = stream;
                    if let Err(error) = handle_stream(stream, state, shutdown_tx).await {
                        error!("[openpupd] connection error: {error}");
                    }
                });
            }
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = termination_signal() => {
                break;
            }
        }
    }

    state.bridge_manager.stop().await;
    cleanup_runtime_files(&socket_path, &pid_path);
    Ok(())
}

#[cfg(unix)]
async fn bind_listener(socket_path: &Path) -> Result<UnixListener> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind `{}`", socket_path.display()))
}

#[cfg(not(unix))]
async fn bind_listener(_socket_path: &Path) -> Result<TcpListener> {
    TcpListener::bind("127.0.0.1:47829")
        .await
        .context("failed to bind daemon tcp listener")
}

#[cfg(unix)]
async fn handle_stream(
    stream: UnixStream,
    state: Arc<DaemonState>,
    shutdown_tx: watch::Sender<bool>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    handle_connection(reader, writer, state, shutdown_tx).await
}

#[cfg(not(unix))]
async fn handle_stream(
    stream: TcpStream,
    state: Arc<DaemonState>,
    shutdown_tx: watch::Sender<bool>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    handle_connection(reader, writer, state, shutdown_tx).await
}

async fn handle_connection<R, W>(
    reader: R,
    writer: W,
    state: Arc<DaemonState>,
    shutdown_tx: watch::Sender<bool>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        return Ok(());
    }

    let request: DaemonRequest =
        serde_json::from_str(line.trim()).context("invalid daemon request")?;

    let (tx, rx) = mpsc::unbounded_channel();
    let writer_task = tokio::spawn(write_events(writer, rx));
    if let Err(error) = handle_request(&state, request, tx.clone(), shutdown_tx).await {
        let _ = tx.send(DaemonEvent::Response {
            response: DaemonResponse::Error {
                message: error.to_string(),
            },
        });
    }
    drop(tx);
    writer_task.await??;
    Ok(())
}

async fn write_events<W>(mut writer: W, mut rx: mpsc::UnboundedReceiver<DaemonEvent>) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(event) = rx.recv().await {
        let line = serde_json::to_string(&event)?;
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    writer.flush().await?;
    Ok(())
}

async fn handle_request(
    state: &Arc<DaemonState>,
    request: DaemonRequest,
    tx: mpsc::UnboundedSender<DaemonEvent>,
    shutdown_tx: watch::Sender<bool>,
) -> Result<()> {
    match request {
        DaemonRequest::Ping => {
            respond(&tx, DaemonResponse::Pong);
        }
        DaemonRequest::Status => {
            respond(&tx, DaemonResponse::Status(build_status(state).await?));
        }
        DaemonRequest::Ask { input, pup } => {
            let sink = Arc::new(SocketEventSink { tx: tx.clone() });
            state
                .runtime
                .alpha
                .clone()
                .process_user_message_stream(input, normalize_forced_pup(pup), sink)
                .await;
        }
        DaemonRequest::BridgeGetConfig => {
            respond(
                &tx,
                DaemonResponse::BridgeConfig(bridge_control::get_bridge_config()),
            );
        }
        DaemonRequest::BridgeSaveConfig { config } => {
            bridge_control::save_bridge_config(Some(&state.bridge_manager), config.clone()).await?;
            respond(&tx, DaemonResponse::BridgeSaved(config));
        }
        DaemonRequest::BridgeReload => {
            let config = bridge_control::reload_bridge_config(&state.bridge_manager).await?;
            respond(&tx, DaemonResponse::BridgeSaved(config));
        }
        DaemonRequest::BridgeStop => {
            state.bridge_manager.stop().await;
            respond(
                &tx,
                DaemonResponse::Ack {
                    message: "bridges stopped".to_string(),
                },
            );
        }
        DaemonRequest::BridgeStatus => {
            let statuses = bridge_control::get_bridge_status(&state.runtime.memory).await?;
            respond(&tx, DaemonResponse::BridgeStatus(statuses));
        }
        DaemonRequest::WeixinQrStart {
            base_url,
            proxy_url,
            route_tag,
            account_id,
            bot_type,
            force,
        } => {
            let result = bridge_control::start_weixin_qr_login(
                &state.bridge_manager.weixin_service(),
                base_url,
                proxy_url,
                route_tag,
                account_id,
                bot_type,
                force,
            )
            .await?;
            respond(&tx, DaemonResponse::WeixinQrStart(result));
        }
        DaemonRequest::WeixinQrWait {
            base_url,
            proxy_url,
            route_tag,
            session_key,
            bot_type,
            timeout_ms,
        } => {
            let result = bridge_control::wait_weixin_qr_login(
                &state.bridge_manager.weixin_service(),
                Some(&state.bridge_manager),
                base_url,
                proxy_url,
                route_tag,
                session_key,
                bot_type,
                timeout_ms,
            )
            .await?;
            respond(&tx, DaemonResponse::WeixinQrWait(result));
        }
        DaemonRequest::WeixinQrCancel { session_key } => {
            bridge_control::cancel_weixin_qr_login(
                &state.bridge_manager.weixin_service(),
                &session_key,
            )
            .await;
            respond(
                &tx,
                DaemonResponse::Ack {
                    message: "weixin login cancelled".to_string(),
                },
            );
        }
        DaemonRequest::WeixinAccounts => {
            let accounts =
                bridge_control::list_weixin_accounts(&state.bridge_manager.weixin_service());
            respond(&tx, DaemonResponse::WeixinAccounts(accounts));
        }
        DaemonRequest::WeixinActivate { account_id } => {
            let config = bridge_control::activate_weixin_account(
                &state.bridge_manager.weixin_service(),
                Some(&state.bridge_manager),
                &account_id,
            )
            .await?;
            respond(&tx, DaemonResponse::WeixinActivated(config));
        }
        DaemonRequest::ChannelList { limit } => {
            let channels = state.runtime.memory.list_channels(limit).await?;
            respond(&tx, DaemonResponse::ChannelList(channels));
        }
        DaemonRequest::ChannelShow { channel_id } => {
            let details = build_channel_details(state, &channel_id).await?;
            respond(&tx, DaemonResponse::ChannelDetails(details));
        }
        DaemonRequest::ChannelContinue {
            channel_id,
            sender,
            comment,
        } => {
            state
                .runtime
                .channel_manager
                .continue_channel(&channel_id, &sender, &comment)
                .await?;
            respond(&tx, DaemonResponse::ChannelActionOk);
        }
        DaemonRequest::ChannelRequestChanges {
            channel_id,
            sender,
            comment,
            reply_to,
        } => {
            state
                .runtime
                .channel_manager
                .request_changes(&channel_id, &sender, &comment, reply_to.as_deref(), None)
                .await?;
            respond(&tx, DaemonResponse::ChannelActionOk);
        }
        DaemonRequest::ChannelComment {
            channel_id,
            sender,
            comment,
            reply_to,
        } => {
            state
                .runtime
                .channel_manager
                .submit_review_comment(&channel_id, &sender, &comment, reply_to.as_deref())
                .await?;
            respond(&tx, DaemonResponse::ChannelActionOk);
        }
        DaemonRequest::Shutdown => {
            respond(
                &tx,
                DaemonResponse::Ack {
                    message: "daemon shutting down".to_string(),
                },
            );
            let _ = shutdown_tx.send(true);
        }
    }

    Ok(())
}

async fn build_status(state: &DaemonState) -> Result<DaemonStatus> {
    let memory_count = state
        .runtime
        .memory
        .list_long_term_memories(0, 100_000, None)
        .await?
        .len();
    let active_task_count = state
        .runtime
        .memory
        .list_tasks(1_000)
        .await?
        .into_iter()
        .filter(|task| task.status == "pending" || task.status == "in_progress")
        .count();
    let enabled_skill_count = state
        .runtime
        .skill_executor
        .registry
        .list_installed()
        .await
        .into_iter()
        .filter(|skill| skill.enabled)
        .count();
    let enabled_pup_count = state
        .runtime
        .alpha
        .list_pup_configs()
        .await
        .into_iter()
        .filter(|pup| pup.enabled)
        .count();

    Ok(DaemonStatus {
        pid: std::process::id(),
        started_at: state.started_at,
        workspace_root: state.runtime.workspace_root.display().to_string(),
        socket_path: state.socket_path.clone(),
        log_path: state.log_path.clone(),
        bridge_enabled: state.bridge_manager.is_enabled(),
        scheduler_enabled: true,
        memory_count,
        active_task_count,
        enabled_skill_count,
        enabled_pup_count,
    })
}

async fn build_channel_details(state: &DaemonState, channel_id: &str) -> Result<ChannelDetails> {
    let channel = state
        .runtime
        .memory
        .list_channels(500)
        .await?
        .into_iter()
        .find(|channel| channel.id == channel_id);
    let workflow = state
        .runtime
        .memory
        .get_channel_workflow_state(channel_id)
        .await?;
    let plan = state.runtime.memory.get_channel_plan(channel_id).await?;
    let messages = state
        .runtime
        .memory
        .get_channel_messages(channel_id)
        .await?;
    Ok(ChannelDetails {
        channel,
        workflow,
        plan,
        messages,
    })
}

fn respond(tx: &mpsc::UnboundedSender<DaemonEvent>, response: DaemonResponse) {
    let _ = tx.send(DaemonEvent::Response { response });
}

fn normalize_forced_pup(pup: Option<String>) -> Option<String> {
    pup.and_then(|value| if value == "alpha" { None } else { Some(value) })
}

fn write_pid_file(pid_path: &Path) -> Result<()> {
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pid_path.exists() {
        let old_pid = std::fs::read_to_string(pid_path).unwrap_or_default();
        if !old_pid.trim().is_empty() && daemon_socket_path().exists() {
            return Err(anyhow!("daemon appears to be running already"));
        }
    }
    std::fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}

fn cleanup_runtime_files(socket_path: &Path, pid_path: &Path) {
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

#[cfg(unix)]
async fn termination_signal() {
    if let Ok(mut signal) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        let _ = signal.recv().await;
    }
}

#[cfg(not(unix))]
async fn termination_signal() {
    std::future::pending::<()>().await;
}
