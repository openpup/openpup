mod chat;
mod config;
mod db;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;

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
    let db_path = db::db_path()?;

    match cli.command {
        Commands::Chat { pup } => {
            chat::run_chat(&db_path, &pup).await?;
        }

        Commands::Memory { action } => match action {
            MemoryCommands::List { r#type, limit } => {
                memory_list(&db_path, r#type.as_deref(), limit).await?;
            }
            MemoryCommands::Search { query, limit } => {
                memory_search(&db_path, &query, limit).await?;
            }
            MemoryCommands::Count => {
                memory_count(&db_path).await?;
            }
        },

        Commands::Skill { action } => match action {
            SkillCommands::List => {
                skill_list(&db_path).await?;
            }
            SkillCommands::Run {
                name,
                input,
                dry_run,
            } => {
                skill_run(&db_path, &name, input.as_deref(), dry_run).await?;
            }
        },

        Commands::Status => {
            show_status(&db_path).await?;
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

async fn memory_list(db_path: &str, filter_type: Option<&str>, limit: i64) -> Result<()> {
    let pool = db::open(db_path).await?;
    let memories = db::list_memories(&pool, filter_type, limit).await?;

    if memories.is_empty() {
        println!("{}", "暂无记忆".dimmed());
        return Ok(());
    }

    for m in &memories {
        let icon = memory_type_icon(&m.memory_type);
        let importance = format!("({:.2})", m.importance);
        println!("{} {} {}", icon, m.content.white(), importance.dimmed());
    }

    println!("\n{}", format!("共 {} 条", memories.len()).dimmed());
    Ok(())
}

async fn memory_search(db_path: &str, query: &str, limit: i64) -> Result<()> {
    let pool = db::open(db_path).await?;
    let memories = db::search_memories(&pool, query, limit).await?;

    if memories.is_empty() {
        println!("{}", "未找到匹配记忆".dimmed());
        return Ok(());
    }

    for m in &memories {
        let icon = memory_type_icon(&m.memory_type);
        println!("{} {}", icon, m.content.white());
    }
    Ok(())
}

async fn memory_count(db_path: &str) -> Result<()> {
    let pool = db::open(db_path).await?;
    let count = db::count_memories(&pool).await?;
    println!("记忆 {} 条", count.to_string().cyan().bold());
    Ok(())
}

// ── Skill commands ────────────────────────────────────────────────────────────

async fn skill_list(db_path: &str) -> Result<()> {
    let pool = db::open(db_path).await?;
    let skills = db::list_skills(&pool).await?;

    if skills.is_empty() {
        println!("{}", "暂无已安装技能".dimmed());
        return Ok(());
    }

    for s in &skills {
        let enabled = if s.enabled {
            "●".green()
        } else {
            "○".dimmed()
        };
        println!(
            "{} {} — {}",
            enabled,
            s.name.white().bold(),
            s.description.dimmed()
        );
    }
    Ok(())
}

async fn skill_run(db_path: &str, name: &str, input: Option<&str>, dry_run: bool) -> Result<()> {
    let pool = db::open(db_path).await?;
    let skill = db::get_skill(&pool, name).await?;

    let Some(skill) = skill else {
        eprintln!("{}", format!("技能 '{}' 未找到", name).red());
        std::process::exit(1);
    };

    if dry_run {
        println!("{}", "[dry-run]".yellow().bold());
        println!("技能: {}", skill.name.cyan());
        println!("描述: {}", skill.description.dimmed());
        println!("输入: {}", input.unwrap_or("(无)").dimmed());

        // Estimate tokens from manifest
        let token_estimate =
            skill.manifest.len() / 4 + input.map(|i| i.len() / 4).unwrap_or(0) + 200;
        println!("预计 ~{} tokens", token_estimate.to_string().yellow());
        print!("\n确认执行? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("{}", "已取消".dimmed());
            return Ok(());
        }
    }

    println!("→ 运行技能 {} …", name.cyan());

    // Load LLM config and run via API
    let cfg = config::load_llm_config()?;
    let result = chat::run_skill_prompt(&cfg, &skill.manifest, input).await?;

    println!("\n{}", result.white());
    Ok(())
}

// ── Status ────────────────────────────────────────────────────────────────────

async fn show_status(db_path: &str) -> Result<()> {
    let cfg = config::load_llm_config();

    println!("{}", "🐾 openpup 状态".bold());
    println!();

    // LLM config
    match cfg {
        Ok(c) => {
            println!(
                "  {} {} ({})",
                "模型:".dimmed(),
                c.model.cyan(),
                c.provider.dimmed()
            );
        }
        Err(e) => {
            println!(
                "  {} {}",
                "模型:".dimmed(),
                format!("配置未找到 — {}", e).red()
            );
        }
    }

    // DB stats
    match db::open(db_path).await {
        Ok(pool) => {
            let mem_count = db::count_memories(&pool).await.unwrap_or(0);
            let task_count = db::count_tasks(&pool).await.unwrap_or(0);
            let skill_count = db::count_skills(&pool).await.unwrap_or(0);
            println!("  {} {}", "记忆:".dimmed(), mem_count.to_string().cyan());
            println!("  {} {}", "任务:".dimmed(), task_count.to_string().cyan());
            println!("  {} {}", "技能:".dimmed(), skill_count.to_string().cyan());
            println!("  {} {}", "数据库:".dimmed(), db_path.dimmed());
        }
        Err(e) => {
            println!(
                "  {} {}",
                "数据库:".dimmed(),
                format!("无法连接 — {}", e).red()
            );
        }
    }

    println!();
    Ok(())
}
