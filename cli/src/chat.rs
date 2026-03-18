use anyhow::{Context, Result};
use colored::Colorize;
use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;
use crate::db;

// ── LLM types ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMsg>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize, Clone)]
struct ChatMsg {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: String,
}

// ── Public entry points ───────────────────────────────────────────────────────

pub async fn run_chat(db_path: &str, pup: &str) -> Result<()> {
    let cfg = crate::config::load_llm_config().context(
        "LLM config not found. Run the GUI app first, or set ~/.openpup/llm_config.json",
    )?;

    let pool = db::open(db_path).await?;
    let mem_count = db::count_memories(&pool).await.unwrap_or(0);

    // Header
    let pup_display = pup_display_name(pup);
    println!(
        "{} {} · {} · {}",
        "🐾".bold(),
        pup_display.cyan().bold(),
        format!("记忆 {} 条", mem_count).dimmed(),
        format!("模式: 牵绳").dimmed(),
    );
    println!("{}", "输入消息，按 Enter 发送。Ctrl-C 退出。".dimmed());
    println!();

    // Load recent context (last 6 turns)
    let history = db::get_recent_history(&pool, 6).await.unwrap_or_default();
    let mut messages: Vec<ChatMsg> = build_system_prompt(pup, &pool).await;

    // Add recent history for context
    for h in &history {
        messages.push(ChatMsg {
            role: h.role.clone(),
            content: h.content.clone(),
        });
    }

    // REPL
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

                // Save user message
                let _ = db::save_conversation(&pool, "user", &line, None).await;

                messages.push(ChatMsg {
                    role: "user".into(),
                    content: line.clone(),
                });

                // Stream response
                print!("\n{} ", pup_display.green().bold());
                let response = stream_response(&cfg, &messages).await?;
                println!("\n");

                // Save assistant response
                let _ =
                    db::save_conversation(&pool, "assistant", &response, Some(&pup_display)).await;

                messages.push(ChatMsg {
                    role: "assistant".into(),
                    content: response,
                });
            }

            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("\n{}", "再见！🐾".dimmed());
                break;
            }

            Err(e) => {
                eprintln!("{}", format!("输入错误: {}", e).red());
                break;
            }
        }
    }

    Ok(())
}

pub async fn run_skill_prompt(
    cfg: &LlmConfig,
    manifest: &str,
    input: Option<&str>,
) -> Result<String> {
    // Simple single-turn skill execution via LLM
    let system =
        "You are executing a skill. Use the manifest as instructions. Return the result directly.";
    let user = if let Some(inp) = input {
        format!("Manifest:\n{}\n\nInput:\n{}", manifest, inp)
    } else {
        format!("Manifest:\n{}", manifest)
    };

    let messages = vec![
        ChatMsg {
            role: "system".into(),
            content: system.into(),
        },
        ChatMsg {
            role: "user".into(),
            content: user,
        },
    ];

    // Non-streaming for skill runs
    non_stream_response(cfg, &messages).await
}

// ── LLM helpers ───────────────────────────────────────────────────────────────

async fn build_system_prompt(pup: &str, pool: &sqlx::sqlite::SqlitePool) -> Vec<ChatMsg> {
    let base = match pup {
        "dev" => "You are Dev Pup, a specialist software engineering assistant. You help with code, debugging, architecture, and development tasks.",
        "writer" => "You are Writer Pup, a specialist writing assistant. You help with articles, reports, emails, and any written content.",
        "ops" => "You are Ops Pup, a specialist DevOps/infrastructure assistant. You help with servers, deployments, and operations.",
        "research" => "You are Research Pup, a specialist research assistant. You help with finding information, summarizing, and analysis.",
        "life_admin" => "You are Life Admin Pup, a specialist personal assistant. You help with scheduling, planning, and life organization.",
        _ => "You are Alpha Pup, the main AI orchestrator. You understand the user deeply and coordinate with specialist pups to help them.",
    };

    // Load top memories for context
    let memory_context = if let Ok(memories) = db::get_top_memories(pool, 5).await {
        if memories.is_empty() {
            String::new()
        } else {
            let lines: Vec<String> = memories
                .iter()
                .map(|m| format!("- [{}] {}", m.memory_type, m.content))
                .collect();
            format!("\n\n## Relevant Memories\n{}", lines.join("\n"))
        }
    } else {
        String::new()
    };

    let system_content = format!("{}{}", base, memory_context);
    vec![ChatMsg {
        role: "system".into(),
        content: system_content,
    }]
}

async fn stream_response(cfg: &LlmConfig, messages: &[ChatMsg]) -> Result<String> {
    let api_base = cfg.api_base.as_deref().unwrap_or("https://api.openai.com");
    let url = format!("{}/v1/chat/completions", api_base.trim_end_matches('/'));

    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
        stream: true,
        max_tokens: Some(2048),
    };

    let client = reqwest::Client::new();
    let mut builder = client.post(&url).json(&req);
    if let Some(key) = &cfg.api_key {
        builder = builder.bearer_auth(key);
    }

    let resp = builder.send().await?.error_for_status()?;
    let mut stream = resp.bytes_stream();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            let line = line.trim();
            if line == "data: [DONE]" {
                break;
            }
            let Some(json_part) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<StreamChunk>(json_part) else {
                continue;
            };
            for choice in parsed.choices {
                let token = choice.delta.content;
                if !token.is_empty() {
                    print!("{}", token);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                    full.push_str(&token);
                }
            }
        }
    }

    Ok(full)
}

async fn non_stream_response(cfg: &LlmConfig, messages: &[ChatMsg]) -> Result<String> {
    #[derive(Deserialize)]
    struct Resp {
        choices: Vec<RespChoice>,
    }
    #[derive(Deserialize)]
    struct RespChoice {
        message: RespMsg,
    }
    #[derive(Deserialize)]
    struct RespMsg {
        content: String,
    }

    let api_base = cfg.api_base.as_deref().unwrap_or("https://api.openai.com");
    let url = format!("{}/v1/chat/completions", api_base.trim_end_matches('/'));

    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
        stream: false,
        max_tokens: Some(2048),
    };

    let client = reqwest::Client::new();
    let mut builder = client.post(&url).json(&req);
    if let Some(key) = &cfg.api_key {
        builder = builder.bearer_auth(key);
    }

    let resp: Resp = builder.send().await?.error_for_status()?.json().await?;
    Ok(resp
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default())
}

fn pup_display_name(key: &str) -> String {
    match key {
        "dev" => "Dev Pup".into(),
        "writer" => "Writer Pup".into(),
        "ops" => "Ops Pup".into(),
        "research" => "Research Pup".into(),
        "life_admin" => "Life Admin Pup".into(),
        _ => "Alpha Pup".into(),
    }
}
