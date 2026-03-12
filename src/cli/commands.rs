use anyhow::{Context, Result};

use crate::audit;
use crate::config;
use crate::core::{llm, memory, persona, registry, runtime, runtime_audit, scheduler};
use crate::tools;
use crate::tools::integrations::{home_assistant, market as market_news};

use super::{
    AddAgentCmd, AddChannelSub, AddToolSub, AgentCmd, CronCmd, CronSub, MemorySub, NodeSub,
    OpenpupCli, PersonaSub, PlanCmd, PlanSub, RunSub, SafetySub, SpawnCmd, ToolSub,
};

/// 顶层入口：解析 CLI 并分发到对应子命令实现。
pub fn dispatch(cli: &OpenpupCli) -> Result<()> {
    // 所有 CLI 调用统一进入审计流水（人类层面的操作）
    audit::record_invocation(cli)?;

    match &cli.command {
        super::Command::Init => cmd_init()?,
        super::Command::Up => cmd_up()?,
        super::Command::Down => cmd_down()?,
        super::Command::Status => cmd_status()?,
        super::Command::Persona(p) => match &p.sub {
            PersonaSub::Init => persona::init()?,
            PersonaSub::Doctor => persona::doctor()?,
        },
        super::Command::Memory(m) => match &m.sub {
            MemorySub::Compact => cmd_memory_compact()?,
            MemorySub::List { kind, limit } => {
                cmd_memory_list(kind.as_deref(), *limit)?;
            }
            MemorySub::Search { query, kind, limit } => {
                cmd_memory_search(query, kind.as_deref(), *limit)?;
            }
            MemorySub::Forget { id } => {
                cmd_memory_forget(*id)?;
            }
        },
        super::Command::Safety(s) => match &s.sub {
            SafetySub::Readonly => cmd_safety_readonly()?,
            SafetySub::DraftOnly => cmd_safety_draft_only()?,
        },
        super::Command::Run(r) => {
            let ev = match &r.sub {
                RunSub::WorkMorning => runtime::RuntimeEvent::manual("work_morning"),
                RunSub::WorkPlanDraft => runtime::RuntimeEvent::manual("work_plan_draft"),
                RunSub::InvestMorning => runtime::RuntimeEvent::manual("invest_morning"),
                RunSub::InvestClose => runtime::RuntimeEvent::manual("invest_close"),
                RunSub::LifeMorning => runtime::RuntimeEvent::manual("life_morning"),
                RunSub::LifeEvening => runtime::RuntimeEvent::manual("life_evening"),
            };
            let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
            rt.block_on(runtime::handle_event(&ev))?;
        }
        super::Command::Agent(a) => cmd_agent(a)?,
        super::Command::Spawn(s) => cmd_spawn(s)?,
        super::Command::Node(n) => match &n.sub {
            NodeSub::Spawn { name, host } => cmd_node_spawn(name, host.as_deref())?,
            NodeSub::List => cmd_node_list()?,
        },
        super::Command::Cron(c) => cmd_cron(c)?,
        super::Command::Dashboard => cmd_dashboard()?,
        super::Command::AddChannel(a) => match &a.sub {
            AddChannelSub::Telegram => cmd_add_channel_telegram()?,
        },
        super::Command::AddTool(a) => match &a.sub {
            AddToolSub::HomeAssistant => cmd_add_tool_home_assistant()?,
            AddToolSub::Market => cmd_add_tool_market()?,
            AddToolSub::NewsRss => cmd_add_tool_news_rss()?,
            AddToolSub::Imap => cmd_add_tool_imap()?,
            AddToolSub::Caldav => cmd_add_tool_caldav()?,
        },
        super::Command::AddAgent(a) => cmd_add_agent(a)?,
        super::Command::Tool(t) => match &t.sub {
            ToolSub::HaGetState { entity_id } => cmd_tool_ha_get_state(entity_id)?,
            ToolSub::MarketQuote { symbol } => cmd_tool_market_quote(symbol)?,
            ToolSub::NewsRssHeadlines { limit } => cmd_tool_news_rss_headlines(*limit)?,
            ToolSub::EmailUnreadSubjects { mailbox, limit } => {
                cmd_tool_email_unread_subjects(mailbox.as_deref(), *limit)?
            }
            ToolSub::CaldavEventsToday { limit } => cmd_tool_caldav_events_today(*limit)?,
            ToolSub::CaldavTasks { limit } => cmd_tool_caldav_tasks(*limit)?,
        },
        super::Command::Plan(p) => cmd_plan(p)?,
        super::Command::AgentsList => cmd_agents_list()?,
        super::Command::ToolsList => cmd_tools_list()?,
    }

    Ok(())
}

fn cmd_init() -> Result<()> {
    let cfg = config::load_or_init()?;
    let path = config::config_path()?;
    println!(
        "openpup init: config is ready at {:?} (spawn.mode = {}, safe by default).",
        path, cfg.autonomy.spawn.mode
    );
    println!("Persona workspace and tools can now be configured via openpup CLI (no external engine required).");
    Ok(())
}

fn cmd_schedule() -> Result<()> {
    println!("Scheduler started (times in UTC). Use Ctrl+C to stop.");
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    rt.block_on(scheduler::run_scheduler_loop())
}

fn cmd_memory_compact() -> Result<()> {
    // Phase 1：本地记忆压缩实现（SQLite），同时记录一次运行时审计事件。
    memory::compact_all()?;

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "memory_compact",
        "Compact local session and semantic memory (SQLite) triggered via CLI.",
    );
    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;
    println!(
        "openpup memory compact: local memory.db compacted (session trim + VACUUM). \
         See ~/.openpup/runtime-audit.log for details."
    );
    Ok(())
}

fn cmd_memory_list(kind: Option<&str>, limit: Option<usize>) -> Result<()> {
    let limit = limit.unwrap_or(10).min(100);
    let items = memory::search_semantic_items(kind, "", limit)?;
    if items.is_empty() {
        println!("No semantic items found.");
        return Ok(());
    }

    println!(
        "Recent semantic items (kind = {:?}, limit = {}):",
        kind, limit
    );
    for it in items {
        println!(
            "- id={} kind={} ts={} tags={:?}",
            it.id, it.kind, it.created_ts, it.tags
        );
        let mut content = it.content.trim().to_string();
        let max_len = 200usize;
        if content.len() > max_len {
            content.truncate(max_len);
            content.push_str(" ...");
        }
        println!("  {}", content);
    }

    Ok(())
}

fn cmd_memory_search(query: &str, kind: Option<&str>, limit: Option<usize>) -> Result<()> {
    let limit = limit.unwrap_or(10).min(100);
    let items = memory::search_semantic_items(kind, query, limit)?;
    if items.is_empty() {
        println!(
            "No semantic items matched query {:?} (kind = {:?}).",
            query, kind
        );
        return Ok(());
    }

    println!(
        "Semantic items matching {:?} (kind = {:?}, limit = {}):",
        query, kind, limit
    );
    for it in items {
        println!(
            "- id={} kind={} ts={} tags={:?}",
            it.id, it.kind, it.created_ts, it.tags
        );
        let mut content = it.content.trim().to_string();
        let max_len = 200usize;
        if content.len() > max_len {
            content.truncate(max_len);
            content.push_str(" ...");
        }
        println!("  {}", content);
    }

    Ok(())
}

fn cmd_memory_forget(id: i64) -> Result<()> {
    let deleted = memory::delete_semantic_item(id)?;

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "memory_forget",
        format!("Forget semantic item id={}", id),
    );
    if deleted {
        event.result = runtime_audit::RuntimeAuditResult {
            status: "success".to_string(),
            error: None,
        };
        println!("Semantic item {} deleted.", id);
    } else {
        event.result = runtime_audit::RuntimeAuditResult {
            status: "skipped".to_string(),
            error: Some("id not found".to_string()),
        };
        println!("Semantic item {} not found.", id);
    }
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    let _ = runtime_audit::record(&event);

    Ok(())
}

fn cmd_agents_list() -> Result<()> {
    let agents = registry::list_sub_agents()?;
    if agents.is_empty() {
        println!("No sub-agents registered (agents.toml is empty or missing).");
        return Ok(());
    }

    println!("Registered sub-agents:");
    for a in agents {
        println!("- name: {}", a.name);
        if let Some(model) = &a.model {
            println!("  model: {}", model);
        }
        if let Some(persona) = &a.persona {
            let mut p = persona.trim().to_string();
            let max_len = 120usize;
            if p.len() > max_len {
                p.truncate(max_len);
                p.push_str(" ...");
            }
            println!("  persona: {}", p);
        }
    }
    Ok(())
}

fn cmd_tools_list() -> Result<()> {
    let cfg = config::load_or_init()?;
    let exposed = tools::exposed_tools_from_config(&cfg);
    if exposed.is_empty() {
        println!("No tools are currently exposed from config.");
        return Ok(());
    }

    println!("Exposed tools (from config and workspace):");
    for t in exposed {
        println!("- {} (level {})", t.name, t.level);
        if !t.description.trim().is_empty() {
            println!("  {}", t.description.trim());
        }
        if !t.args.trim().is_empty() {
            println!("  args: {}", t.args.trim());
        }
    }
    Ok(())
}

fn cmd_add_channel_telegram() -> Result<()> {
    use std::io::{self, Read as _};

    let mut cfg = config::load_or_init()?;

    println!("Telegram bot token env var name (default TELEGRAM_BOT_TOKEN):");
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let name = line.trim();
    let bot_token_env = if name.is_empty() {
        "TELEGRAM_BOT_TOKEN".to_string()
    } else {
        name.to_string()
    };

    println!("Allowed chat ids (comma-separated, numeric chat.id from Telegram). Empty => no chats allowed:");
    let mut chats = String::new();
    io::stdin().read_line(&mut chats)?;
    let allowed_chat_ids: Vec<String> = chats
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let tg_cfg = config::TelegramChannelConfig {
        bot_token_env,
        allowed_chat_ids,
    };

    let mut channels = cfg.channels.take().unwrap_or_default();
    channels.telegram = Some(tg_cfg);
    cfg.channels = Some(channels);
    config::save(&cfg)?;

    println!(
        "Telegram channel config saved to {:?}. Start bot with `openpup telegram-bot`.",
        config::config_path()?
    );
    Ok(())
}

fn cmd_add_tool_home_assistant() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    println!("Enter Home Assistant base URL (e.g. http://homeassistant.local:8123):");
    let mut base_url = String::new();
    let _ = std::io::stdin().read_line(&mut base_url);
    let base_url = base_url.trim().to_string();
    if base_url.is_empty() {
        anyhow::bail!("base_url is required.");
    }

    println!("Enter token env var name (default HOME_ASSISTANT_TOKEN):");
    let mut token_env = String::new();
    let _ = std::io::stdin().read_line(&mut token_env);
    let token_env = token_env.trim().to_string();
    let token_env = if token_env.is_empty() {
        "HOME_ASSISTANT_TOKEN".to_string()
    } else {
        token_env
    };

    println!("Enter allowed entities (comma-separated), or leave empty for no whitelist:");
    let mut entities = String::new();
    let _ = std::io::stdin().read_line(&mut entities);
    let allowed_entities: Vec<String> = entities
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let ha_cfg = config::HomeAssistantConfig {
        base_url,
        token_env,
        allowed_entities,
    };

    let mut integrations = cfg.integrations.take().unwrap_or_default();
    integrations.home_assistant = Some(ha_cfg);
    cfg.integrations = Some(integrations);
    config::save(&cfg)?;

    println!(
        "Home Assistant integration saved to {:?}.",
        config::config_path()?
    );
    Ok(())
}

fn cmd_tool_ha_get_state(entity_id: &str) -> Result<()> {
    let cfg = config::load_or_init()?;
    let ha = home_assistant::get_home_assistant_config(&cfg)?;

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "tool_ha_get_state",
        format!("Read Home Assistant state for {}.", entity_id),
    );
    event.tools.push(runtime_audit::tool_call(
        "home_assistant_get_state",
        Some(entity_id.to_string()),
    ));

    let v = home_assistant::get_state(entity_id, &ha)?;
    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;

    let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("unknown");
    let friendly = v
        .get("attributes")
        .and_then(|a| a.get("friendly_name"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    println!("{} {} => {}", entity_id, friendly, state);
    Ok(())
}

fn cmd_add_tool_market() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    println!("Market provider (default stooq):");
    let mut provider = String::new();
    let _ = std::io::stdin().read_line(&mut provider);
    let provider = provider.trim().to_string();
    let provider = if provider.is_empty() {
        "stooq".to_string()
    } else {
        provider
    };

    println!("Watchlist symbols (comma-separated, optional; e.g. AAPL.US,TSLA.US):");
    let mut wl = String::new();
    let _ = std::io::stdin().read_line(&mut wl);
    let watchlist: Vec<String> = wl
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let m = config::MarketConfig {
        provider,
        watchlist,
    };
    let mut integrations = cfg.integrations.take().unwrap_or_default();
    integrations.market = Some(m);
    cfg.integrations = Some(integrations);
    config::save(&cfg)?;
    println!("Market integration saved to {:?}.", config::config_path()?);
    Ok(())
}

fn cmd_add_tool_news_rss() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    println!("Enter RSS feed URLs (comma-separated):");
    let mut feeds = String::new();
    let _ = std::io::stdin().read_line(&mut feeds);
    let feeds: Vec<String> = feeds
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if feeds.is_empty() {
        anyhow::bail!("at least one feed URL is required");
    }
    let n = config::NewsRssConfig { feeds };
    let mut integrations = cfg.integrations.take().unwrap_or_default();
    integrations.news_rss = Some(n);
    cfg.integrations = Some(integrations);
    config::save(&cfg)?;
    println!(
        "News RSS integration saved to {:?}.",
        config::config_path()?
    );
    Ok(())
}

fn cmd_tool_market_quote(symbol: &str) -> Result<()> {
    let cfg = config::load_or_init()?;
    let provider = cfg
        .integrations
        .as_ref()
        .and_then(|i| i.market.as_ref())
        .map(|m| m.provider.as_str())
        .unwrap_or("stooq");
    if provider != "stooq" {
        anyhow::bail!(
            "unsupported provider {} (only stooq is implemented)",
            provider
        );
    }

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "tool_market_quote",
        format!("Read market quote for {}.", symbol),
    );
    event.tools.push(runtime_audit::tool_call(
        "market_quote",
        Some(symbol.to_string()),
    ));

    let v = market_news::stooq_quote_daily(symbol)?;
    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;

    let sym = v.get("symbol").and_then(|x| x.as_str()).unwrap_or(symbol);
    let close = v.get("close").and_then(|x| x.as_str()).unwrap_or("");
    let date = v.get("date").and_then(|x| x.as_str()).unwrap_or("");
    println!("{} close={} date={}", sym, close, date);
    Ok(())
}

fn cmd_tool_news_rss_headlines(limit: Option<usize>) -> Result<()> {
    let cfg = config::load_or_init()?;
    let feeds = cfg
        .integrations
        .as_ref()
        .and_then(|i| i.news_rss.as_ref())
        .map(|n| n.feeds.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("news_rss is not configured. Run `openpup add-tool news-rss`.")
        })?;

    let limit = limit.unwrap_or(5).min(20);

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "tool_news_rss_headlines",
        format!("Fetch RSS headlines ({} feeds).", feeds.len()),
    );
    event.tools.push(runtime_audit::tool_call(
        "news_rss_headlines",
        Some(format!("feeds={},limit={}", feeds.len(), limit)),
    ));

    for feed in feeds {
        println!("--- {}", feed);
        let items = market_news::rss_headlines(&feed, limit)?;
        for it in items {
            let title = it.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let link = it.get("link").and_then(|x| x.as_str()).unwrap_or("");
            println!("- {} ({})", title, link);
        }
    }

    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;
    Ok(())
}

fn cmd_add_tool_imap() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    println!("IMAP host (e.g. imap.gmail.com):");
    let mut host = String::new();
    let _ = std::io::stdin().read_line(&mut host);
    let host = host.trim().to_string();
    if host.is_empty() {
        anyhow::bail!("host is required");
    }

    println!("IMAP port (default 993):");
    let mut port = String::new();
    let _ = std::io::stdin().read_line(&mut port);
    let port = port.trim().parse::<u16>().unwrap_or(993);

    println!("Username env var (default IMAP_USERNAME):");
    let mut u = String::new();
    let _ = std::io::stdin().read_line(&mut u);
    let username_env = u.trim();
    let username_env = if username_env.is_empty() {
        "IMAP_USERNAME".to_string()
    } else {
        username_env.to_string()
    };

    println!("Password env var (default IMAP_PASSWORD):");
    let mut p = String::new();
    let _ = std::io::stdin().read_line(&mut p);
    let password_env = p.trim();
    let password_env = if password_env.is_empty() {
        "IMAP_PASSWORD".to_string()
    } else {
        password_env.to_string()
    };

    println!("Allowed mailboxes (comma-separated). Empty => INBOX only:");
    let mut mb = String::new();
    let _ = std::io::stdin().read_line(&mut mb);
    let allowed_mailboxes: Vec<String> = mb
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let imap_cfg = config::ImapConfig {
        host,
        port,
        username_env,
        password_env,
        allowed_mailboxes,
    };
    let mut integrations = cfg.integrations.take().unwrap_or_default();
    integrations.imap = Some(imap_cfg);
    cfg.integrations = Some(integrations);
    config::save(&cfg)?;
    println!("IMAP integration saved to {:?}.", config::config_path()?);
    Ok(())
}

fn cmd_tool_email_unread_subjects(mailbox: Option<&str>, limit: Option<usize>) -> Result<()> {
    let cfg = config::load_or_init()?;
    let imap_cfg = tools::integrations::email_imap::get_imap_config(&cfg)?;
    let mailbox = mailbox.unwrap_or("INBOX");
    let limit = limit.unwrap_or(10).min(50);

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "tool_email_unread_subjects",
        format!("List unread email subjects in {}.", mailbox),
    );
    event.tools.push(runtime_audit::tool_call(
        "email_imap_unseen_envelope",
        Some(format!("mailbox={},limit={}", mailbox, limit)),
    ));

    let items = tools::integrations::email_imap::unread_envelopes(&imap_cfg, mailbox, limit)?;
    for (subj, from, date) in items {
        println!("- {} | {} | {}", subj, from, date);
    }

    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;
    Ok(())
}

fn cmd_add_tool_caldav() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    println!("CalDAV base URL (must return ICS on GET for now):");
    let mut base = String::new();
    let _ = std::io::stdin().read_line(&mut base);
    let base_url = base.trim().to_string();
    if base_url.is_empty() {
        anyhow::bail!("base_url is required");
    }

    println!("Username env var (default CALDAV_USERNAME):");
    let mut u = String::new();
    let _ = std::io::stdin().read_line(&mut u);
    let username_env = u.trim();
    let username_env = if username_env.is_empty() {
        "CALDAV_USERNAME".to_string()
    } else {
        username_env.to_string()
    };

    println!("Password env var (default CALDAV_PASSWORD):");
    let mut p = String::new();
    let _ = std::io::stdin().read_line(&mut p);
    let password_env = p.trim();
    let password_env = if password_env.is_empty() {
        "CALDAV_PASSWORD".to_string()
    } else {
        password_env.to_string()
    };

    println!("Calendar ICS URLs (comma-separated). Empty => base_url only:");
    let mut urls = String::new();
    let _ = std::io::stdin().read_line(&mut urls);
    let calendar_urls: Vec<String> = urls
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let c = config::CaldavConfig {
        base_url,
        username_env,
        password_env,
        calendar_urls,
    };
    let mut integrations = cfg.integrations.take().unwrap_or_default();
    integrations.caldav = Some(c);
    cfg.integrations = Some(integrations);
    config::save(&cfg)?;
    println!("CalDAV integration saved to {:?}.", config::config_path()?);
    Ok(())
}

fn cmd_add_agent(cmd: &AddAgentCmd) -> Result<()> {
    let mut cfg = config::load_or_init()?;

    // 现有或默认 llm 配置
    let mut llm_cfg = cfg.llm.unwrap_or(config::LlmConfigDisk {
        base_url: String::new(),
        model: "gpt-4.1-mini".to_string(),
        temperature: 0.7,
        api_key: None,
    });

    // 若命令行未显式提供，则走交互式输入（类似 add-tool）。
    use std::io;

    if cmd.base_url.is_none() {
        println!("OpenAI base URL (e.g. https://api.openai.com/v1):");
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let v = line.trim();
        if !v.is_empty() {
            llm_cfg.base_url = v.to_string();
        }
    } else if let Some(b) = &cmd.base_url {
        llm_cfg.base_url = b.trim().to_string();
    }

    if cmd.model.is_none() {
        println!("Model name/alias (default {}):", llm_cfg.model);
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let v = line.trim();
        if !v.is_empty() {
            llm_cfg.model = v.to_string();
        }
    } else if let Some(m) = &cmd.model {
        llm_cfg.model = m.trim().to_string();
    }

    if cmd.temperature.is_none() {
        println!(
            "Temperature (current {:.2}, press Enter to keep):",
            llm_cfg.temperature
        );
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let v = line.trim();
        if !v.is_empty() {
            if let Ok(t) = v.parse::<f32>() {
                llm_cfg.temperature = t;
            }
        }
    } else if let Some(t) = cmd.temperature {
        llm_cfg.temperature = t;
    }

    println!("API key (optional; leave empty to keep using env OPENPUP_LLM_API_KEY):");
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let v = line.trim();
    if v.is_empty() {
        // 保持原来的 api_key（若已有）
    } else {
        llm_cfg.api_key = Some(v.to_string());
    }

    if llm_cfg.base_url.trim().is_empty() {
        anyhow::bail!("base-url is required (OpenAI).");
    }

    cfg.llm = Some(llm_cfg);
    config::save(&cfg)?;

    println!(
        "LLM/agent config saved to {:?} (section [llm]).",
        config::config_path()?
    );
    println!("Sensitive API keys are still read from env (e.g. OPENPUP_LLM_API_KEY).");

    Ok(())
}

fn cmd_tool_caldav_events_today(limit: Option<usize>) -> Result<()> {
    let cfg = config::load_or_init()?;
    let c = tools::integrations::caldav::get_caldav_config(&cfg)?;
    let limit = limit.unwrap_or(10).min(50);

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "tool_caldav_events_today",
        "Fetch CalDAV events (basic GET+parse).",
    );
    event.tools.push(runtime_audit::tool_call(
        "caldav_get_ics_events",
        Some(format!("limit={}", limit)),
    ));

    let blobs = tools::integrations::caldav::fetch_ics_blobs(&c, 3)?;
    for b in blobs {
        for ev in tools::integrations::caldav::parse_events(&b, limit)? {
            let s = ev.get("summary").and_then(|x| x.as_str()).unwrap_or("");
            let ds = ev.get("dtstart").and_then(|x| x.as_str()).unwrap_or("");
            println!("- {} ({})", s, ds);
        }
    }

    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;
    Ok(())
}

fn cmd_tool_caldav_tasks(limit: Option<usize>) -> Result<()> {
    let cfg = config::load_or_init()?;
    let c = tools::integrations::caldav::get_caldav_config(&cfg)?;
    let limit = limit.unwrap_or(10).min(50);

    let mut event = runtime_audit::new_event(
        "default",
        "core",
        "manual",
        "tool_caldav_tasks",
        "Fetch CalDAV tasks (basic GET+parse).",
    );
    event.tools.push(runtime_audit::tool_call(
        "caldav_get_ics_tasks",
        Some(format!("limit={}", limit)),
    ));

    let blobs = tools::integrations::caldav::fetch_ics_blobs(&c, 3)?;
    for b in blobs {
        for td in tools::integrations::caldav::parse_tasks(&b, limit)? {
            let s = td.get("summary").and_then(|x| x.as_str()).unwrap_or("");
            let due = td.get("due").and_then(|x| x.as_str()).unwrap_or("");
            let st = td.get("status").and_then(|x| x.as_str()).unwrap_or("");
            println!("- {} due={} status={}", s, due, st);
        }
    }

    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;
    Ok(())
}

fn cmd_up() -> Result<()> {
    // 现阶段：启动 openpup 自身的调度循环和已配置的通道（如 Telegram），作为最小「守护进程」。
    println!("openpup up: starting scheduler and channels (times in UTC). Use Ctrl+C to stop.");
    let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    rt.block_on(async {
        // 启动 Telegram 通道（若配置缺失，内部会报错并退出该任务，不影响调度循环）。
        tokio::spawn(async {
            if let Err(e) = crate::channels::run_telegram_channel().await {
                eprintln!("openpup up: telegram channel exited with error: {e:#}");
            }
        });
        scheduler::run_scheduler_loop().await
    })
}

fn cmd_down() -> Result<()> {
    // 当前版本不管理外部 daemon，暂时仅提示用户手动停止对应进程。
    println!("openpup down: no external daemon to stop. Use Ctrl+C in the `openpup up` process.");
    Ok(())
}

fn cmd_dashboard() -> Result<()> {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let home = dirs::home_dir().context("failed to locate home directory")?;
    let path = home.join(".openpup").join("runtime-audit.log");
    println!("openpup dashboard");
    println!("  runtime-audit: {:?}", path);

    if !path.exists() {
        println!("  (no runtime-audit.log found yet; run some loops or tools first)");
        return Ok(());
    }

    let file = File::open(&path)
        .with_context(|| format!("failed to open runtime audit log at {:?}", path))?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
    let max_events = 100usize;
    if lines.len() > max_events {
        lines.drain(0..lines.len() - max_events);
    }

    #[derive(Debug, Default)]
    struct LoopStat {
        last_ts: String,
        count: usize,
    }
    let mut loop_stats: HashMap<String, LoopStat> = HashMap::new();
    let mut tool_call_success = 0usize;
    let mut tool_call_fail = 0usize;

    const LOOP_IDS: &[&str] = &[
        "work_morning",
        "work_plan_draft",
        "invest_morning",
        "invest_close",
        "life_morning",
        "life_evening",
    ];

    for line in &lines {
        if let Ok(ev) = serde_json::from_str::<runtime_audit::RuntimeAuditEvent>(line) {
            let entry = loop_stats
                .entry(ev.trigger_kind.clone())
                .or_insert_with(LoopStat::default);
            entry.count += 1;
            entry.last_ts = ev.ts.clone();

            if !ev.tools.is_empty() {
                if ev.result.status == "success" {
                    tool_call_success += 1;
                } else {
                    tool_call_fail += 1;
                }
            }
        }
    }

    println!();
    println!("--- Loops (file-based) ---");
    for &lid in LOOP_IDS {
        if let Some(stat) = loop_stats.get(lid) {
            println!(
                "  {:20} count = {:4}, last = {}",
                lid, stat.count, stat.last_ts
            );
        }
    }

    println!();
    println!("--- Tool calls (from audit) ---");
    println!(
        "  success = {}, failed = {}",
        tool_call_success, tool_call_fail
    );

    println!();
    println!("--- All activity (by trigger_kind) ---");
    for (kind, stat) in loop_stats {
        if !LOOP_IDS.contains(&kind.as_str()) {
            println!(
                "  {:24} count = {:4}, last_ts = {}",
                kind, stat.count, stat.last_ts
            );
        }
    }
    println!();
    Ok(())
}

fn cmd_cron(cmd: &CronCmd) -> Result<()> {
    match &cmd.sub {
        CronSub::Run { loop_id } => {
            println!("openpup cron run {} (single-shot)", loop_id);
            let ev = runtime::RuntimeEvent::time(loop_id);
            let rt = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
            rt.block_on(runtime::handle_event(&ev))?;
        }
    }
    Ok(())
}

fn cmd_agent(cmd: &AgentCmd) -> Result<()> {
    let cfg = config::load_or_init()?;
    let mut system = match persona::load_assembled_persona() {
        Ok(s) => s,
        Err(_) => String::from("# Persona\n\n(未找到 workspace/persona，使用空 persona。)"),
    };
    if let Some(pf) = &cmd.persona_file {
        let content = std::fs::read_to_string(pf)
            .with_context(|| format!("failed to read persona-file {}", pf))?;
        system = content;
    }

    let exposed_tools = tools::exposed_tools_from_config(&cfg);
    let mut tools_section = String::new();
    if !exposed_tools.is_empty() {
        tools_section.push_str(
            "You can optionally request calling local tools by replying with a JSON object on a single line:\n\
{\"tool\": \"name\", \"args\": {...}}\n\
Do not add explanation text around it when you want a tool call.\n\
Available tools:\n",
        );
        for t in &exposed_tools {
            tools_section.push_str(&format!(
                "- {} (level {}): {} args: {}\n",
                t.name, t.level, t.description, t.args
            ));
        }
        tools_section.push_str("- save_composite_tool (management): {\"spec_toml\": string containing CompositeToolFile TOML}\n");
        tools_section.push_str("- register_sub_agent (management): {\"name\": string, \"model\": optional, \"persona\": optional} — register a sub-agent (allowed when spawn.mode != disabled)\n");
        tools_section.push_str("- register_node (management): {\"name\": string, \"host\": optional} — register a worker node (allowed when spawn.mode != disabled)\n");
        tools_section.push_str("- invoke_sub_agent (multi-agent): {\"name\": string, \"input\": string} — run one turn with a registered sub-agent and get its reply\n");
        tools_section.push_str("- invoke_node_tool (multi-node): {\"node\": string, \"tool\": string, \"args\": object} — run a tool on a registered node (node must have host; node must expose POST /tool)\n");
        tools_section.push_str("\n");
    }
    if let Some(s) = &cmd.subagents {
        let names: Vec<&str> = s
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .collect();
        if !names.is_empty() {
            let agents_file = registry::load_agents().unwrap_or_default();
            let mut sub =
                String::from("\n\n### Available sub-agents (you may delegate or refer to them):\n");
            for name in names {
                if let Some(spec) = agents_file.agents.get(name) {
                    let persona = spec.persona.as_deref().unwrap_or("(none)");
                    let model = spec.model.as_deref().unwrap_or("(default)");
                    sub.push_str(&format!(
                        "- {}: model={}, persona={}\n",
                        name, model, persona
                    ));
                }
            }
            system.push_str(&sub);
        }
    }

    system = format!(
        "You are openpup, a local agent.\n\n{}{}",
        tools_section, system
    );

    // CLI 入口统一走 AgentKernel（单轮 message 使用临时 Tokio runtime）。
    if let Some(m) = &cmd.message {
        let req = crate::core::kernel::AgentRequest {
            session_id: "cli".to_string(),
            input: m.clone(),
            semantic_kind: Some("loop_log".to_string()),
        };
        let rt = tokio::runtime::Runtime::new()
            .context("failed to create tokio runtime for single agent turn")?;
        let result = rt.block_on(async {
            let kernel = crate::core::kernel::DefaultKernel::from_config(cfg.clone());
            kernel.run_turn(req).await
        })?;
        if let Some((_call, res)) = &result.tool_call {
            println!("pup (tool): {:?}", res);
        }
        println!("{}", result.reply_text);
        return Ok(());
    }

    println!("openpup agent (interactive). Type your message and press Enter. Ctrl+C to exit.\n");
    let rt = tokio::runtime::Runtime::new()
        .context("failed to create tokio runtime for interactive agent")?;
    let session_cfg = cfg.clone();
    rt.block_on(async move {
        let kernel = crate::core::kernel::DefaultKernel::from_config(session_cfg);
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        use std::io::Write as _;
        loop {
            print!("you> ");
            stdout.flush().ok();
            let mut line = String::new();
            let n = stdin.read_line(&mut line)?;
            if n == 0 {
                break;
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let req = crate::core::kernel::AgentRequest {
                session_id: "cli".to_string(),
                input: line.to_string(),
                semantic_kind: Some("loop_log".to_string()),
            };
            match kernel.run_turn(req).await {
                Ok(result) => {
                    if let Some((call, res)) = &result.tool_call {
                        println!("pup (tool request): {:?} -> {:?}", call.kind, res);
                    }
                    println!("pup> {}\n", result.reply_text.trim());
                }
                Err(e) => {
                    eprintln!("pup (error): {:#}", e);
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

fn cmd_spawn(cmd: &SpawnCmd) -> Result<()> {
    let spec = registry::SubAgentSpec {
        name: cmd.name.clone(),
        model: cmd.model.clone(),
        persona: cmd.persona.clone(),
    };
    registry::register_sub_agent(spec)?;
    println!(
        "openpup spawn: sub-agent {:?} registered (~/.openpup/agents.toml).",
        cmd.name
    );
    Ok(())
}

fn cmd_node_spawn(name: &str, host: Option<&str>) -> Result<()> {
    let info = registry::NodeInfo {
        name: name.to_string(),
        host: host.map(String::from),
        tags: Vec::new(),
        last_seen_ts: memory::now_unix_ts(),
        status: "registered".to_string(),
    };
    registry::register_node(info)?;
    println!(
        "openpup node spawn: node {:?} registered (~/.openpup/nodes.toml).",
        name
    );
    Ok(())
}

fn cmd_node_list() -> Result<()> {
    let nodes = registry::list_nodes()?;
    if nodes.is_empty() {
        println!("openpup node list: no nodes registered. Use `openpup node spawn <name> [--host <host>]` to add one.");
        return Ok(());
    }
    println!("openpup node list:");
    println!(
        "  {:<20} {:<24} {:<12} {}",
        "name", "host", "status", "last_seen"
    );
    for n in nodes {
        let host = n.host.as_deref().unwrap_or("-");
        println!(
            "  {:<20} {:<24} {:<12} {}",
            n.name, host, n.status, n.last_seen_ts
        );
    }
    Ok(())
}

fn cmd_status() -> Result<()> {
    let cfg = config::load_or_init()?;
    let path = config::config_path()?;
    println!("openpup config: {:?}", path);
    println!("  spawn.mode = {}", cfg.autonomy.spawn.mode);
    println!("  execution_mode = {}", cfg.autonomy.execution_mode);
    match persona::load_assembled_persona() {
        Ok(s) => println!("Persona: assembled ({} chars, ready for runtime)", s.len()),
        Err(_) => println!("Persona: not loaded (run `openpup persona init` and edit ~/.openpup/workspace/persona/)"),
    }
    Ok(())
}

fn cmd_safety_readonly() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    cfg.autonomy.execution_mode = "readonly".to_string();
    config::save(&cfg)?;
    println!("Safety: execution_mode set to \"readonly\". Only L1 tools should be enabled.");
    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "safety_emergency",
        "Emergency downgrade: execution_mode set to readonly.",
    );
    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    let _ = runtime_audit::record(&event);
    Ok(())
}

fn cmd_safety_draft_only() -> Result<()> {
    let mut cfg = config::load_or_init()?;
    cfg.autonomy.execution_mode = "draft-only".to_string();
    config::save(&cfg)?;
    println!(
        "Safety: execution_mode set to \"draft-only\". Allow L1/L2 (draft), forbid L3/L4 execution."
    );
    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "manual",
        "safety_emergency",
        "Emergency downgrade: execution_mode set to draft-only.",
    );
    event.result = runtime_audit::RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = runtime_audit::RuntimeAuditRisk::default();
    let _ = runtime_audit::record(&event);
    Ok(())
}

fn cmd_plan(cmd: &PlanCmd) -> Result<()> {
    let cfg = config::load_or_init()?;
    match &cmd.sub {
        PlanSub::Run { goal } => {
            let llm_cfg = llm::load_openai_from_config(&cfg)?;
            let exposed = tools::exposed_tools_from_config(&cfg);
            if exposed.is_empty() {
                anyhow::bail!(
                    "no tools are exposed in config.toml [tools] section; cannot run planner."
                );
            }

            let system = {
                let mut s = String::new();
                s.push_str("You are openpup, a local planning agent.\n");
                s.push_str("Your task is to plan a small sequence of local tool calls to help with the given goal.\n");
                s.push_str("Respond ONLY with a JSON array, no extra text. Example:\n");
                s.push_str("[\n  {\"tool\": \"email_unread_subjects\", \"args\": {\"limit\": 5}},\n  {\"tool\": \"news_rss_headlines\", \"args\": {\"limit\": 3}}\n]\n\n");
                s.push_str(
                    "Wrong: text before/after the array. Wrong: object instead of array.\n\n",
                );
                s.push_str("Available tools:\n");
                for t in &exposed {
                    s.push_str(&format!(
                        "- {} (level {}): {} args: {}\n",
                        t.name, t.level, t.description, t.args
                    ));
                }
                s
            };

            let rt = tokio::runtime::Runtime::new()
                .context("failed to create tokio runtime for planner")?;
            let arr = rt.block_on(llm::tool_planner(&llm_cfg, &system, goal))?;
            let arr = arr
                .as_array()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("planner reply is not a JSON array"))?;

            println!("openpup plan run for goal: {}\n", goal);
            let mut step_count = 0usize;
            let mut success_count = 0usize;
            for (idx, step) in arr.iter().enumerate() {
                let tool_name = match step.get("tool").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => {
                        println!("#{}: invalid step (missing tool field), skipped", idx + 1);
                        continue;
                    }
                };
                let args = step
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                let exposed_tool = match exposed.iter().find(|t| t.name == tool_name) {
                    Some(t) => t,
                    None => {
                        println!(
                            "#{}: tool {} is not in exposed tools, skipped",
                            idx + 1,
                            tool_name
                        );
                        continue;
                    }
                };

                let call = tools::ToolCall {
                    kind: exposed_tool.kind.clone(),
                    args: args.clone(),
                };

                step_count += 1;
                println!("#{}: calling tool {}", idx + 1, tool_name);
                let res = tools::execute_tool(&cfg, &call, None);
                if res.ok {
                    success_count += 1;
                }
                println!("    result: {:?}\n", res);

                let mut ev = runtime_audit::new_event(
                    runtime_audit::REALM_DEFAULT,
                    runtime_audit::AGENT_CORE,
                    "manual",
                    "plan_run_step",
                    format!("plan step {}: {} for goal {}", idx + 1, tool_name, goal),
                );
                ev.tools.push(runtime_audit::tool_call(
                    tool_name,
                    Some(format!("{:?}", args)),
                ));
                ev.result = runtime_audit::RuntimeAuditResult {
                    status: if res.ok { "success" } else { "error" }.to_string(),
                    error: res.error.clone(),
                };
                ev.risk = runtime_audit::RuntimeAuditRisk::default();
                let _ = runtime_audit::record(&ev);
            }

            let summary = format!(
                "plan_run goal={} steps={} success={}",
                goal, step_count, success_count
            );
            let _ = memory::add_semantic_item("plan_run", &summary, Some(goal));
            Ok(())
        }
        PlanSub::SaveTool { spec } => {
            let content = if let Some(rest) = spec.strip_prefix('@') {
                std::fs::read_to_string(rest)
                    .with_context(|| format!("failed to read composite tool file {}", rest))?
            } else {
                spec.clone()
            };
            let path = tools::save_composite_tool_raw(&content)
                .context("failed to save composite tool spec")?;
            println!("Composite tool saved to {:?}", path);
            Ok(())
        }
    }
}
