use anyhow::Result;
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use colored::Colorize;
use openpup_core::headless::HeadlessRuntime;
use openpup_core::runtime::EventSink;
use openpup_core::skills::permissions::{PermissionRequestPayload, PermissionUi};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(
    name = "openpup",
    about = "🐾 openpup CLI — your pup pack, at the terminal",
    version = env!("CARGO_PKG_VERSION")
)]
struct Cli {
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = HeadlessRuntime::new(Some(Arc::new(StdioPermissionUi))).await?;

    match cli.command {
        Commands::Chat { pup } => {
            run_chat(runtime, &pup).await?;
        }

        Commands::Ask { input, pup } => {
            let output = run_single_prompt(&runtime, input, pup).await?;
            if output.starts_with("⚠️ ") {
                println!("{output}");
            }
        }

        Commands::Memory { action } => match action {
            MemoryCommands::List { r#type, limit } => {
                memory_list(&runtime, r#type.as_deref(), limit).await?;
            }
            MemoryCommands::Search { query, limit } => {
                memory_search(&runtime, &query, limit).await?;
            }
            MemoryCommands::Count => {
                memory_count(&runtime).await?;
            }
        },

        Commands::Skill { action } => match action {
            SkillCommands::List => {
                skill_list(&runtime).await?;
            }
            SkillCommands::Run {
                name,
                input,
                dry_run,
            } => {
                skill_run(&runtime, &name, input.as_deref(), dry_run).await?;
            }
        },

        Commands::Status => {
            show_status(&runtime).await?;
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

async fn run_chat(runtime: HeadlessRuntime, pup: &str) -> Result<()> {
    let llm_cfg = runtime.alpha.llm_client.current_config();
    let model = llm_cfg.1;
    let mem_preview = runtime.memory.get_top_memories(5).await.unwrap_or_default();
    println!(
        "{} {} · {} · {}",
        "🐾".bold(),
        "OpenPup CLI".cyan().bold(),
        format!("模型 {}", model).dimmed(),
        format!("记忆 {} 条", mem_preview.len()).dimmed(),
    );
    println!("{}", "输入消息，按 Enter 发送。Ctrl-C / Ctrl-D 退出。".dimmed());
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
                let output = run_single_prompt(
                    &runtime,
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

async fn memory_list(runtime: &HeadlessRuntime, filter_type: Option<&str>, limit: i64) -> Result<()> {
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

async fn skill_run(runtime: &HeadlessRuntime, name: &str, input: Option<&str>, dry_run: bool) -> Result<()> {
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
