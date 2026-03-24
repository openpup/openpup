mod daemon_client;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use colored::Colorize;
use openpup_core::headless::HeadlessRuntime;
use openpup_core::ipc::{ChannelDetails, DaemonEvent, DaemonRequest, DaemonResponse, DaemonStatus};
use openpup_core::runtime::EventSink;
use openpup_core::skills::permissions::{PermissionRequestPayload, PermissionUi};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(
    name = "openpup",
    about = "🐾 openpup CLI — your pup pack, at the terminal",
    version = env!("CARGO_PKG_VERSION")
)]
struct Cli {
    /// Force local in-process runtime instead of daemon
    #[arg(long, global = true)]
    local: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session with Alpha Pup
    Chat {
        /// Which pup to talk to (alpha, dev, writer, ops, research, life_admin)
        #[arg(long, default_value = "alpha")]
        pup: String,
    },

    /// Send one prompt and print the answer
    Ask {
        /// Prompt text
        input: String,

        /// Which pup to force (alpha, dev, writer, ops, research, life_admin)
        #[arg(long)]
        pup: Option<String>,
    },

    /// Manage the local daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonCommands,
    },

    /// Control bridge runtime through daemon
    Bridge {
        #[command(subcommand)]
        action: BridgeCommands,
    },

    /// Control Pack Channels through daemon
    Channel {
        #[command(subcommand)]
        action: ChannelCommands,
    },

    /// Manage long-term memories
    Memory {
        #[command(subcommand)]
        action: MemoryCommands,
    },

    /// Run a skill
    Skill {
        #[command(subcommand)]
        action: SkillCommands,
    },

    /// Show current status (memories, mode, pups)
    Status,
}

#[derive(Subcommand)]
enum MemoryCommands {
    /// List memories
    List {
        /// Filter by type (preference, boundary, fact, event, ...)
        #[arg(long)]
        r#type: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: i64,
    },

    /// Search memories
    Search {
        /// Search query
        query: String,

        #[arg(long, default_value = "10")]
        limit: i64,
    },

    /// Show memory count
    Count,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// List installed skills
    List,

    /// Run a skill
    Run {
        /// Skill name
        name: String,

        /// Input text to pass to the skill
        #[arg(long)]
        input: Option<String>,

        /// Preview what the skill will do without executing
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum DaemonCommands {
    Start,
    Stop,
    Status,
    Logs {
        #[arg(long, default_value_t = 80)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum BridgeCommands {
    Status,
    Config,
    Reload,
    Stop,
    Weixin {
        #[command(subcommand)]
        action: WeixinCommands,
    },
}

#[derive(Subcommand)]
enum WeixinCommands {
    QrStart {
        #[arg(long, default_value = "")]
        base_url: String,
        #[arg(long)]
        proxy_url: Option<String>,
        #[arg(long)]
        route_tag: Option<String>,
        #[arg(long)]
        account_id: Option<String>,
        #[arg(long)]
        bot_type: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    QrWait {
        session_key: String,
        #[arg(long, default_value = "")]
        base_url: String,
        #[arg(long)]
        proxy_url: Option<String>,
        #[arg(long)]
        route_tag: Option<String>,
        #[arg(long)]
        bot_type: Option<String>,
        #[arg(long)]
        timeout_ms: Option<i64>,
    },
    QrCancel {
        session_key: String,
    },
    Accounts,
    Activate {
        account_id: String,
    },
}

#[derive(Subcommand)]
enum ChannelCommands {
    List {
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    Show {
        channel_id: String,
    },
    Watch {
        channel_id: String,
        #[arg(long, default_value_t = 2)]
        interval_secs: u64,
    },
    Continue {
        channel_id: String,
        comment: String,
        #[arg(long, default_value = "owner")]
        sender: String,
    },
    RequestChanges {
        channel_id: String,
        comment: String,
        #[arg(long, default_value = "owner")]
        sender: String,
        #[arg(long)]
        reply_to: Option<String>,
    },
    Comment {
        channel_id: String,
        comment: String,
        #[arg(long, default_value = "owner")]
        sender: String,
        #[arg(long)]
        reply_to: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut local_runtime: Option<HeadlessRuntime> = None;

    match cli.command {
        Commands::Chat { pup } => {
            run_chat(&mut local_runtime, cli.local, &pup).await?;
        }

        Commands::Ask { input, pup } => {
            let output =
                run_single_prompt_with_mode(&mut local_runtime, cli.local, input, pup).await?;
            if output.starts_with("⚠️ ") {
                println!("{output}");
            }
        }

        Commands::Daemon { action } => {
            run_daemon_command(action).await?;
        }

        Commands::Bridge { action } => {
            if cli.local {
                return Err(anyhow!("bridge 控制仅支持 daemon 模式，请去掉 `--local`"));
            }
            ensure_daemon_running_for_command().await?;
            run_bridge_command(action).await?;
        }

        Commands::Channel { action } => {
            if cli.local {
                return Err(anyhow!("channel 控制仅支持 daemon 模式，请去掉 `--local`"));
            }
            ensure_daemon_running_for_command().await?;
            run_channel_command(action).await?;
        }

        Commands::Memory { action } => match action {
            MemoryCommands::List { r#type, limit } => {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                memory_list(&runtime, r#type.as_deref(), limit).await?;
            }
            MemoryCommands::Search { query, limit } => {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                memory_search(&runtime, &query, limit).await?;
            }
            MemoryCommands::Count => {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                memory_count(&runtime).await?;
            }
        },

        Commands::Skill { action } => match action {
            SkillCommands::List => {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                skill_list(&runtime).await?;
            }
            SkillCommands::Run {
                name,
                input,
                dry_run,
            } => {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                skill_run(&runtime, &name, input.as_deref(), dry_run).await?;
            }
        },

        Commands::Status => {
            if !cli.local && daemon_client::is_running().await {
                show_daemon_status().await?;
            } else {
                let runtime = ensure_local_runtime(&mut local_runtime).await?;
                show_status(&runtime).await?;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum CliStreamEvent {
    Token(String),
    Activity(String),
    Done(String),
    Error(String),
}

struct CliEventSink {
    tx: mpsc::UnboundedSender<CliStreamEvent>,
}

impl EventSink for CliEventSink {
    fn emit_value(&self, event: &str, payload: Value) {
        let stream_event = match event {
            "stream_token" => payload
                .as_str()
                .map(|value| CliStreamEvent::Token(value.to_string())),
            "stream_activity" => payload
                .get("label")
                .and_then(Value::as_str)
                .map(|value| CliStreamEvent::Activity(value.to_string())),
            "stream_done" => payload
                .get("content")
                .and_then(Value::as_str)
                .map(|value| CliStreamEvent::Done(value.to_string())),
            "stream_error" => payload
                .as_str()
                .map(|value| CliStreamEvent::Error(value.to_string())),
            _ => None,
        };

        if let Some(stream_event) = stream_event {
            let _ = self.tx.send(stream_event);
        }
    }
}

struct StdioPermissionUi;

#[async_trait]
impl PermissionUi for StdioPermissionUi {
    async fn request_permission(&self, payload: PermissionRequestPayload) -> Result<bool> {
        tokio::task::spawn_blocking(move || -> Result<bool> {
            eprintln!(
                "\n[权限确认] 技能 `{}` 请求执行：{}",
                payload.skill_name.cyan(),
                payload.action_description.yellow()
            );
            print!("允许继续？[y/N] ");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            let answer = line.trim();
            Ok(matches!(answer, "y" | "Y" | "yes" | "YES" | "Yes"))
        })
        .await?
    }
}

async fn run_single_prompt(
    runtime: &HeadlessRuntime,
    input: String,
    pup: Option<String>,
) -> Result<String> {
    let forced_pup = pup.and_then(|value| if value == "alpha" { None } else { Some(value) });
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sink = Arc::new(CliEventSink { tx });
    let alpha = runtime.alpha.clone();

    let task = tokio::spawn(async move {
        alpha
            .process_user_message_stream(input, forced_pup, sink)
            .await;
    });

    let mut final_output = String::new();
    let mut printed_anything = false;

    while let Some(event) = rx.recv().await {
        match event {
            CliStreamEvent::Token(token) => {
                print!("{token}");
                io::stdout().flush()?;
                printed_anything = true;
            }
            CliStreamEvent::Activity(label) => {
                eprintln!("{}", format!("· {label}").dimmed());
            }
            CliStreamEvent::Done(output) => {
                final_output = output;
                break;
            }
            CliStreamEvent::Error(err) => {
                if printed_anything {
                    println!();
                }
                return Ok(format!("⚠️ {err}"));
            }
        }
    }

    let _ = task.await;
    if printed_anything {
        println!();
    }
    Ok(final_output)
}

async fn run_single_prompt_with_mode(
    local_runtime: &mut Option<HeadlessRuntime>,
    force_local: bool,
    input: String,
    pup: Option<String>,
) -> Result<String> {
    if !force_local && daemon_client::is_running().await {
        return run_single_prompt_daemon(input, pup).await;
    }
    let runtime = ensure_local_runtime(local_runtime).await?;
    run_single_prompt(&runtime, input, pup).await
}

async fn run_single_prompt_daemon(input: String, pup: Option<String>) -> Result<String> {
    let mut final_output = String::new();
    let mut printed_anything = false;

    daemon_client::stream(DaemonRequest::Ask { input, pup }, |event| {
        match event {
            DaemonEvent::Token { token } => {
                print!("{token}");
                io::stdout().flush()?;
                printed_anything = true;
            }
            DaemonEvent::Activity { label } => {
                eprintln!("{}", format!("· {label}").dimmed());
            }
            DaemonEvent::Done { content } => {
                final_output = content;
            }
            DaemonEvent::Response { .. } => {}
            DaemonEvent::Error { message } => return Err(anyhow!(message)),
        }
        Ok(())
    })
    .await?;

    if printed_anything {
        println!();
    }
    Ok(final_output)
}

async fn run_chat(
    local_runtime: &mut Option<HeadlessRuntime>,
    force_local: bool,
    pup: &str,
) -> Result<()> {
    let model_label = if !force_local && daemon_client::is_running().await {
        "daemon".to_string()
    } else {
        let runtime = ensure_local_runtime(local_runtime).await?;
        let llm_cfg = runtime.alpha.llm_client.current_config();
        llm_cfg.1
    };
    println!(
        "{} {} · {} · {}",
        "🐾".bold(),
        "OpenPup CLI".cyan().bold(),
        format!("模型 {}", model_label).dimmed(),
        if !force_local && daemon_client::is_running().await {
            "daemon 模式".dimmed().to_string()
        } else {
            let runtime = ensure_local_runtime(local_runtime).await?;
            format!(
                "记忆 {} 条",
                runtime
                    .memory
                    .get_top_memories(5)
                    .await
                    .unwrap_or_default()
                    .len()
            )
            .dimmed()
            .to_string()
        },
    );
    println!(
        "{}",
        "输入消息，按 Enter 发送。Ctrl-C / Ctrl-D 退出。".dimmed()
    );
    println!();

    let mut rl = DefaultEditor::new()?;
    loop {
        let prompt = format!("{} ", "You:".yellow().bold());
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                rl.add_history_entry(&line)?;
                print!("{} ", "Pup:".green().bold());
                io::stdout().flush()?;
                let output = run_single_prompt_with_mode(
                    local_runtime,
                    force_local,
                    line,
                    if pup == "alpha" {
                        None
                    } else {
                        Some(pup.to_string())
                    },
                )
                .await?;
                if output.starts_with("⚠️ ") {
                    println!("{output}");
                }
                println!();
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("\n{}", "再见！".dimmed());
                break;
            }
            Err(err) => {
                eprintln!("{}", format!("输入错误: {err}").red());
                break;
            }
        }
    }

    Ok(())
}

async fn ensure_local_runtime(runtime: &mut Option<HeadlessRuntime>) -> Result<HeadlessRuntime> {
    if runtime.is_none() {
        *runtime = Some(HeadlessRuntime::new(Some(Arc::new(StdioPermissionUi))).await?);
    }
    runtime
        .clone()
        .ok_or_else(|| anyhow!("failed to initialize local runtime"))
}

// ── Memory commands ──────────────────────────────────────────────────────────

fn memory_type_icon(t: &str) -> &'static str {
    match t {
        "preference" => "❤️ ",
        "boundary" | "restriction" => "❌",
        "fact" => "📋",
        "event" | "recent" => "🔼",
        _ => "📌",
    }
}

async fn memory_list(
    runtime: &HeadlessRuntime,
    filter_type: Option<&str>,
    limit: i64,
) -> Result<()> {
    let mut memories = runtime
        .memory
        .list_long_term_memories(0, 100_000, None)
        .await?;
    if let Some(memory_type) = filter_type {
        memories.retain(|(_, _, ty, _, _)| ty == memory_type);
    }
    memories.truncate(limit.max(0) as usize);

    if memories.is_empty() {
        println!("{}", "暂无记忆".dimmed());
        return Ok(());
    }

    for (_id, content, memory_type, importance, _created_at) in &memories {
        let icon = memory_type_icon(memory_type);
        let importance = format!("({:.2})", importance);
        println!("{} {} {}", icon, content.white(), importance.dimmed());
    }

    println!("\n{}", format!("共 {} 条", memories.len()).dimmed());
    Ok(())
}

async fn memory_search(runtime: &HeadlessRuntime, query: &str, limit: i64) -> Result<()> {
    let memories = runtime
        .memory
        .list_long_term_memories(0, limit, Some(query))
        .await?;

    if memories.is_empty() {
        println!("{}", "未找到匹配记忆".dimmed());
        return Ok(());
    }

    for (_id, content, memory_type, _importance, _created_at) in &memories {
        let icon = memory_type_icon(memory_type);
        println!("{} {}", icon, content.white());
    }
    Ok(())
}

async fn memory_count(runtime: &HeadlessRuntime) -> Result<()> {
    let count = runtime
        .memory
        .list_long_term_memories(0, 100_000, None)
        .await?
        .len();
    println!("记忆 {} 条", count.to_string().cyan().bold());
    Ok(())
}

// ── Skill commands ────────────────────────────────────────────────────────────

async fn skill_list(runtime: &HeadlessRuntime) -> Result<()> {
    let skills = runtime.skill_executor.registry.list_installed().await;

    if skills.is_empty() {
        println!("{}", "暂无已安装技能".dimmed());
        return Ok(());
    }

    for skill in &skills {
        let enabled = if skill.enabled {
            "●".green()
        } else {
            "○".dimmed()
        };
        println!(
            "{} {} — {}",
            enabled,
            skill.name.white().bold(),
            skill.description.dimmed()
        );
    }
    Ok(())
}

async fn skill_run(
    runtime: &HeadlessRuntime,
    name: &str,
    input: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let manifest = runtime.skill_executor.registry.ensure_skill(name).await?;

    if dry_run {
        println!("{}", "[dry-run]".yellow().bold());
        println!("技能: {}", manifest.metadata.name.cyan());
        println!("描述: {}", manifest.metadata.description.dimmed());
        println!("输入: {}", input.unwrap_or("(无)").dimmed());
        println!(
            "权限: shell={} filesystem={} network={} mcp={}",
            manifest.permissions.shell,
            manifest.permissions.filesystem,
            manifest.permissions.network,
            manifest.permissions.mcp
        );
        print!("\n确认执行? [y/N] ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("{}", "已取消".dimmed());
            return Ok(());
        }
    }

    println!("→ 运行技能 {} …", name.cyan());
    let abort = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = runtime
        .skill_executor
        .execute_skill_stream(
            name,
            input.unwrap_or(""),
            Arc::new(|tok: String, _is_reasoning: bool| {
                print!("{tok}");
                let _ = io::stdout().flush();
            }),
            Arc::new(|kind: String, label: String| {
                eprintln!("{}", format!("· [{kind}] {label}").dimmed());
            }),
            abort,
        )
        .await?;

    println!("\n{}", result.white());
    Ok(())
}

// ── Status ────────────────────────────────────────────────────────────────────

async fn show_status(runtime: &HeadlessRuntime) -> Result<()> {
    println!("{}", "🐾 openpup 状态".bold());
    println!();

    let (provider, model, _mini, _embed, api_key, api_base) =
        runtime.alpha.llm_client.current_config();
    let provider_label = match provider {
        openpup_core::llm::client::Provider::OpenAI => "openai",
        openpup_core::llm::client::Provider::Ollama => "ollama",
    };
    println!(
        "  {} {} ({})",
        "模型:".dimmed(),
        model.cyan(),
        provider_label.dimmed()
    );
    println!(
        "  {} {}",
        "API Key:".dimmed(),
        if api_key.as_deref().unwrap_or("").is_empty() {
            "未配置".red().to_string()
        } else {
            "已配置".green().to_string()
        }
    );
    if let Some(base) = api_base {
        println!("  {} {}", "API Base:".dimmed(), base.dimmed());
    }

    let mem_count = runtime
        .memory
        .list_long_term_memories(0, 100_000, None)
        .await
        .map(|rows| rows.len())
        .unwrap_or(0);
    let task_count = runtime
        .memory
        .list_tasks(1_000)
        .await
        .map(|rows| {
            rows.into_iter()
                .filter(|task| task.status == "pending" || task.status == "in_progress")
                .count()
        })
        .unwrap_or(0);
    let skill_count = runtime
        .skill_executor
        .registry
        .list_installed()
        .await
        .into_iter()
        .filter(|skill| skill.enabled)
        .count();
    let pup_count = runtime
        .alpha
        .list_pup_configs()
        .await
        .into_iter()
        .filter(|pup| pup.enabled)
        .count();

    println!("  {} {}", "记忆:".dimmed(), mem_count.to_string().cyan());
    println!("  {} {}", "任务:".dimmed(), task_count.to_string().cyan());
    println!("  {} {}", "技能:".dimmed(), skill_count.to_string().cyan());
    println!("  {} {}", "Pups:".dimmed(), pup_count.to_string().cyan());
    println!(
        "  {} {}",
        "工作区:".dimmed(),
        runtime.workspace_root.display().to_string().dimmed()
    );

    println!();
    Ok(())
}

async fn run_daemon_command(action: DaemonCommands) -> Result<()> {
    match action {
        DaemonCommands::Start => {
            if daemon_client::is_running().await {
                println!("{}", "daemon 已在运行".green());
                return Ok(());
            }
            daemon_client::start_process().await?;
            println!("{}", "daemon 已启动".green());
            println!("socket: {}", daemon_client::socket_display().dimmed());
            println!(
                "pid: {}",
                daemon_client::pid_path().display().to_string().dimmed()
            );
            println!(
                "log: {}",
                daemon_client::log_path().display().to_string().dimmed()
            );
        }
        DaemonCommands::Stop => {
            daemon_client::stop_process().await?;
            println!("{}", "daemon 已停止".green());
        }
        DaemonCommands::Status => {
            show_daemon_status().await?;
        }
        DaemonCommands::Logs { lines } => {
            let path = daemon_client::log_path();
            if !path.exists() {
                println!("{}", "日志文件不存在".dimmed());
                return Ok(());
            }
            let content = std::fs::read_to_string(&path)?;
            let tail: Vec<_> = content.lines().rev().take(lines).collect();
            for line in tail.into_iter().rev() {
                println!("{line}");
            }
        }
    }
    Ok(())
}

async fn show_daemon_status() -> Result<()> {
    let response = daemon_client::call(DaemonRequest::Status).await?;
    let DaemonResponse::Status(status) = response else {
        return Err(anyhow!("daemon 返回了意外响应"));
    };
    print_daemon_status(&status);
    Ok(())
}

fn print_daemon_status(status: &DaemonStatus) {
    println!("{}", "🐾 openpupd 状态".bold());
    println!();
    println!("  {} {}", "PID:".dimmed(), status.pid.to_string().cyan());
    println!(
        "  {} {}",
        "启动时间:".dimmed(),
        status.started_at.to_string().cyan()
    );
    println!(
        "  {} {}",
        "Bridge:".dimmed(),
        bool_label(status.bridge_enabled)
    );
    println!(
        "  {} {}",
        "Scheduler:".dimmed(),
        bool_label(status.scheduler_enabled)
    );
    println!(
        "  {} {}",
        "记忆:".dimmed(),
        status.memory_count.to_string().cyan()
    );
    println!(
        "  {} {}",
        "任务:".dimmed(),
        status.active_task_count.to_string().cyan()
    );
    println!(
        "  {} {}",
        "技能:".dimmed(),
        status.enabled_skill_count.to_string().cyan()
    );
    println!(
        "  {} {}",
        "Pups:".dimmed(),
        status.enabled_pup_count.to_string().cyan()
    );
    println!(
        "  {} {}",
        "工作区:".dimmed(),
        status.workspace_root.dimmed()
    );
    println!("  {} {}", "Socket:".dimmed(), status.socket_path.dimmed());
    println!("  {} {}", "日志:".dimmed(), status.log_path.dimmed());
    println!();
}

fn bool_label(value: bool) -> colored::ColoredString {
    if value {
        "enabled".green()
    } else {
        "disabled".yellow()
    }
}

async fn ensure_daemon_running_for_command() -> Result<()> {
    if daemon_client::is_running().await {
        Ok(())
    } else {
        Err(anyhow!("daemon 未运行，请先执行 `openpup daemon start`"))
    }
}

async fn run_bridge_command(action: BridgeCommands) -> Result<()> {
    match action {
        BridgeCommands::Status => {
            let response = daemon_client::call(DaemonRequest::BridgeStatus).await?;
            let DaemonResponse::BridgeStatus(statuses) = response else {
                return Err(anyhow!("bridge status 返回了意外响应"));
            };
            if statuses.is_empty() {
                println!("{}", "暂无 bridge 状态".dimmed());
            } else {
                for status in statuses {
                    let label = if status.connected {
                        "connected".green()
                    } else {
                        "disconnected".yellow()
                    };
                    println!(
                        "{} {} {}",
                        status.platform.cyan().bold(),
                        format!("{:?}", status.status).dimmed(),
                        label
                    );
                    if let Some(error) = status.error_msg {
                        println!("  {}", error.red());
                    }
                }
            }
        }
        BridgeCommands::Config => {
            let response = daemon_client::call(DaemonRequest::BridgeGetConfig).await?;
            let DaemonResponse::BridgeConfig(config) = response else {
                return Err(anyhow!("bridge config 返回了意外响应"));
            };
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        BridgeCommands::Reload => {
            let response = daemon_client::call(DaemonRequest::BridgeReload).await?;
            let DaemonResponse::BridgeSaved(config) = response else {
                return Err(anyhow!("bridge reload 返回了意外响应"));
            };
            println!("{}", "bridge 已按当前配置重载".green());
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        BridgeCommands::Stop => {
            let _ = daemon_client::call(DaemonRequest::BridgeStop).await?;
            println!("{}", "bridge 已停止".green());
        }
        BridgeCommands::Weixin { action } => {
            run_weixin_command(action).await?;
        }
    }
    Ok(())
}

async fn run_weixin_command(action: WeixinCommands) -> Result<()> {
    match action {
        WeixinCommands::QrStart {
            base_url,
            proxy_url,
            route_tag,
            account_id,
            bot_type,
            force,
        } => {
            let response = daemon_client::call(DaemonRequest::WeixinQrStart {
                base_url,
                proxy_url,
                route_tag,
                account_id,
                bot_type,
                force,
            })
            .await?;
            let DaemonResponse::WeixinQrStart(result) = response else {
                return Err(anyhow!("weixin qr start 返回了意外响应"));
            };
            println!("session_key: {}", result.session_key.cyan());
            if let Some(url) = result.qrcode_url {
                println!("qr_url: {}", url);
            }
            println!("{}", result.message.dimmed());
        }
        WeixinCommands::QrWait {
            session_key,
            base_url,
            proxy_url,
            route_tag,
            bot_type,
            timeout_ms,
        } => {
            let response = daemon_client::call(DaemonRequest::WeixinQrWait {
                base_url,
                proxy_url,
                route_tag,
                session_key,
                bot_type,
                timeout_ms,
            })
            .await?;
            let DaemonResponse::WeixinQrWait(result) = response else {
                return Err(anyhow!("weixin qr wait 返回了意外响应"));
            };
            println!("status: {}", result.status.cyan());
            println!("connected: {}", result.connected.to_string().cyan());
            if let Some(account_id) = result.account_id {
                println!("account_id: {}", account_id);
            }
            if let Some(user_id) = result.user_id {
                println!("user_id: {}", user_id);
            }
            if let Some(url) = result.qrcode_url {
                println!("qr_url: {}", url);
            }
            println!("{}", result.message.dimmed());
        }
        WeixinCommands::QrCancel { session_key } => {
            let _ = daemon_client::call(DaemonRequest::WeixinQrCancel { session_key }).await?;
            println!("{}", "已取消微信登录会话".green());
        }
        WeixinCommands::Accounts => {
            let response = daemon_client::call(DaemonRequest::WeixinAccounts).await?;
            let DaemonResponse::WeixinAccounts(accounts) = response else {
                return Err(anyhow!("weixin accounts 返回了意外响应"));
            };
            if accounts.is_empty() {
                println!("{}", "暂无已保存账号".dimmed());
            } else {
                for account in accounts {
                    println!(
                        "{} {} {}",
                        account.account_id.white().bold(),
                        account.user_id.unwrap_or_else(|| "-".to_string()).dimmed(),
                        account.saved_at.unwrap_or_else(|| "-".to_string()).dimmed(),
                    );
                }
            }
        }
        WeixinCommands::Activate { account_id } => {
            let response =
                daemon_client::call(DaemonRequest::WeixinActivate { account_id }).await?;
            let DaemonResponse::WeixinActivated(config) = response else {
                return Err(anyhow!("weixin activate 返回了意外响应"));
            };
            println!("{}", "已切换微信账号并重载 bridge".green());
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
    }
    Ok(())
}

async fn run_channel_command(action: ChannelCommands) -> Result<()> {
    match action {
        ChannelCommands::List { limit } => {
            let response = daemon_client::call(DaemonRequest::ChannelList { limit }).await?;
            let DaemonResponse::ChannelList(channels) = response else {
                return Err(anyhow!("channel list 返回了意外响应"));
            };
            if channels.is_empty() {
                println!("{}", "暂无频道".dimmed());
            } else {
                for channel in channels {
                    println!(
                        "{} {} {} {}",
                        channel.id.cyan(),
                        channel.status.yellow(),
                        channel.title.white(),
                        format!("[{}]", channel.members.join(", ")).dimmed(),
                    );
                }
            }
        }
        ChannelCommands::Show { channel_id } => {
            print_channel_details(fetch_channel_details(&channel_id).await?);
        }
        ChannelCommands::Watch {
            channel_id,
            interval_secs,
        } => {
            let mut last_signature = String::new();
            loop {
                let details = fetch_channel_details(&channel_id).await?;
                let signature = format!(
                    "{}:{}",
                    details
                        .workflow
                        .as_ref()
                        .map(|workflow| workflow.status.clone())
                        .unwrap_or_default(),
                    details.messages.len()
                );
                if signature != last_signature {
                    print!("\x1B[2J\x1B[1;1H");
                    print_channel_details(details);
                    last_signature = signature;
                }
                tokio::time::sleep(Duration::from_secs(interval_secs.max(1))).await;
            }
        }
        ChannelCommands::Continue {
            channel_id,
            comment,
            sender,
        } => {
            let _ = daemon_client::call(DaemonRequest::ChannelContinue {
                channel_id,
                sender,
                comment,
            })
            .await?;
            println!("{}", "已继续频道执行".green());
        }
        ChannelCommands::RequestChanges {
            channel_id,
            comment,
            sender,
            reply_to,
        } => {
            let _ = daemon_client::call(DaemonRequest::ChannelRequestChanges {
                channel_id,
                sender,
                comment,
                reply_to,
            })
            .await?;
            println!("{}", "已请求修改".green());
        }
        ChannelCommands::Comment {
            channel_id,
            comment,
            sender,
            reply_to,
        } => {
            let _ = daemon_client::call(DaemonRequest::ChannelComment {
                channel_id,
                sender,
                comment,
                reply_to,
            })
            .await?;
            println!("{}", "已发表评论".green());
        }
    }
    Ok(())
}

async fn fetch_channel_details(channel_id: &str) -> Result<ChannelDetails> {
    let response = daemon_client::call(DaemonRequest::ChannelShow {
        channel_id: channel_id.to_string(),
    })
    .await?;
    let DaemonResponse::ChannelDetails(details) = response else {
        return Err(anyhow!("channel show 返回了意外响应"));
    };
    Ok(details)
}

fn print_channel_details(details: ChannelDetails) {
    if let Some(channel) = details.channel.as_ref() {
        println!(
            "{} {} {}",
            channel.id.cyan().bold(),
            channel.status.yellow(),
            channel.title.white().bold()
        );
        println!(
            "{} {}",
            "成员:".dimmed(),
            if channel.members.is_empty() {
                "-".dimmed().to_string()
            } else {
                channel.members.join(", ").dimmed().to_string()
            }
        );
    } else {
        println!("{}", "频道不存在".red());
    }

    if let Some(workflow) = details.workflow.as_ref() {
        println!(
            "{} status={} layer={:?} review_round={} awaiting_user={} blocked={}",
            "workflow:".dimmed(),
            workflow.status,
            workflow.current_layer,
            workflow.review_round,
            workflow.awaiting_user,
            workflow.blocked_reason.clone().unwrap_or_default()
        );
    }

    if let Some(plan) = details.plan.as_ref() {
        println!();
        println!("{}", "Plan".bold());
        for task in &plan.subtasks {
            println!(
                "- {}: {} {}",
                task.pup.cyan(),
                task.description,
                if task.depends_on.is_empty() {
                    "".dimmed().to_string()
                } else {
                    format!("(depends: {})", task.depends_on.join(", "))
                        .dimmed()
                        .to_string()
                }
            );
        }
    }

    println!();
    println!("{}", "Messages".bold());
    for message in details.messages {
        println!(
            "[{}] {} {}",
            message.msg_type.dimmed(),
            message.sender.green(),
            message.content
        );
    }
}
