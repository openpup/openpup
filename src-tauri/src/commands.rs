use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use tracing::{debug, warn};

use crate::agents::alpha::{AlphaPup, PupConfig};
use crate::bridge::types::{BridgeConfig, BridgeConnectionStatus};
use crate::llm::client::Provider;
use crate::mcp::orchestrator::{MCPOrchestrator, McpServerEntry, McpToolInfo};
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::memory::system::SkillRunRecord;
use crate::skills::permissions::{ExecutionMode, PermissionChecker};
use crate::skills::registry::InstalledSkill;
use crate::workspace::backup::{export_workspace_default, import_workspace_from_path};

#[derive(Clone)]
pub struct AppState {
    pub alpha: Arc<AlphaPup>,
    pub file_layer: Arc<FileLayer>,
    pub bridge_manager: Arc<crate::bridge::BridgeManager>,
}

// ─── Chat ────────────────────────────────────────────────────────────────────

/// Starts streaming a response.
/// Returns immediately; tokens are emitted as `stream_token` events,
/// completion as `stream_done` (pup name string), errors as `stream_error`.
/// `forced_pup` bypasses intent classification and routes directly to that pup key.
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    input: String,
    forced_pup: Option<String>,
) -> Result<(), String> {
    // Guard: require API key before any LLM call
    let (_, model, mini_model, _embed_model, api_key, api_base) =
        state.alpha.llm_client.current_config();
    debug!(
        "[cmd] send_message: model={model:?} mini={mini_model:?} base={api_base:?} has_key={}",
        api_key.is_some()
    );
    if api_key.as_deref().unwrap_or("").trim().is_empty() {
        let _ = app_handle.emit(
      "stream_error",
      "未配置 API Key。请编辑 ~/.openpup/config.toml，在 [llm] 下填写 api_key，然后重启应用。\n\n示例：\n[llm]\napi_key = \"sk-...\"\nmodel = \"gpt-4o\"",
    );
        return Ok(());
    }

    let alpha = state.alpha.clone();
    let event_sink = Arc::new(crate::runtime_tauri::TauriEventSink::new(app_handle));
    tauri::async_runtime::spawn(async move {
        alpha
            .process_user_message_stream(input, forced_pup, event_sink)
            .await;
    });
    Ok(())
}

/// Cancel the current in-progress streaming response.
#[tauri::command]
pub async fn abort_message(state: State<'_, AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    state.alpha.abort_flag.store(true, Ordering::Relaxed);
    Ok(())
}

// ─── Onboarding ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_onboarding_completed(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.file_layer.is_onboarding_completed())
}

#[derive(Deserialize)]
pub struct OnboardingData {
    pub name: String,
    pub boundaries: String,
    pub pain_points: String,
    pub language: String,
    pub work_schedule: String,
    pub tools: String,
}

#[tauri::command]
pub async fn save_onboarding_data(
    state: State<'_, AppState>,
    data: OnboardingData,
) -> Result<(), String> {
    let content = format!(
        "# Owner Profile\n\n\
     ## Name\n{}\n\n\
     ## Boundaries\n{}\n\n\
     ## Pain Points\n{}\n\n\
     ## Language\n{}\n\n\
     ## Work Schedule\n{}\n\n\
     ## Tools\n{}\n",
        data.name.trim(),
        data.boundaries.trim(),
        data.pain_points.trim(),
        data.language.trim(),
        data.work_schedule.trim(),
        data.tools.trim(),
    );
    state
        .file_layer
        .write_owner_profile(&content)
        .map_err(|e| e.to_string())?;

    // ── Skills path first-run init ────────────────────────────────────────────
    // Ensure ~/.openpup/skills/ is created and registered in search_paths so
    // the LLM always has a known, writable directory for user-generated skills.
    {
        let mut cfg = crate::config::load();
        if cfg.skills.search_paths.is_empty() {
            let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            let default_skills_dir = home.join(".openpup").join("skills");
            let _ = std::fs::create_dir_all(&default_skills_dir);
            cfg.skills.search_paths = vec!["~/.openpup/skills/".to_string()];
            if let Err(e) = crate::config::save(&cfg) {
                warn!("failed to persist default skills path: {e}");
            }
        }
    }

    // Seed long-term memory DB with the key facts from onboarding
    let memory: Arc<MemorySystem> = state.alpha.memory.clone();
    let _ = memory
        .add_long_term_memory(
            &format!("行为边界：{}", data.boundaries.trim()),
            "rule",
            0.99,
        )
        .await;
    if !data.pain_points.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("常见痛点/重复工作：{}", data.pain_points.trim()),
                "fact",
                0.85,
            )
            .await;
    }
    if !data.language.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("语言偏好：{}", data.language.trim()),
                "preference",
                0.95,
            )
            .await;
    }
    if !data.name.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("用户名字/称呼：{}", data.name.trim()),
                "preference",
                0.99,
            )
            .await;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_owner_profile(state: State<'_, AppState>) -> Result<String, String> {
    state
        .file_layer
        .read_owner_profile()
        .map_err(|e| e.to_string())
}

// ─── Memory ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct LongTermMemoryItem {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub importance: f32,
    pub created_at: i64,
}

fn memory_system_from_state(state: &State<'_, AppState>) -> Arc<MemorySystem> {
    state.alpha.memory.clone()
}

#[tauri::command]
pub async fn list_long_term_memories(
    state: State<'_, AppState>,
    offset: i64,
    limit: i64,
    query: Option<String>,
) -> Result<Vec<LongTermMemoryItem>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory
        .list_long_term_memories(offset, limit, query.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(
            |(id, content, memory_type, importance, created_at)| LongTermMemoryItem {
                id,
                content,
                memory_type,
                importance,
                created_at,
            },
        )
        .collect())
}

#[tauri::command]
pub async fn update_long_term_memory(
    state: State<'_, AppState>,
    id: String,
    content: String,
    memory_type: String,
    importance: f32,
) -> Result<(), String> {
    let memory = memory_system_from_state(&state);
    memory
        .update_long_term_memory(&id, &content, &memory_type, importance)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_long_term_memory(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let memory = memory_system_from_state(&state);
    memory
        .delete_long_term_memory(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Top memories by importance — shown as context chips in the chat header.
#[derive(Serialize)]
pub struct MemoryChip {
    pub content: String,
    pub memory_type: String,
    pub importance: f32,
}

#[tauri::command]
pub async fn get_top_memories(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<MemoryChip>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory
        .get_top_memories(limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(content, memory_type, importance)| MemoryChip {
            content,
            memory_type,
            importance,
        })
        .collect())
}

// ─── Timeline ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TimelineEvent {
    pub role: String,
    pub pup_key: String,
    pub pup_name: String,
    pub content: String,
    pub timestamp: i64,
}

#[tauri::command]
pub async fn list_timeline_events(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<TimelineEvent>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory
        .list_conversations_for_timeline(limit)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|(role, content, timestamp)| TimelineEvent {
            pup_key: if role == "assistant" {
                "alpha".to_string()
            } else {
                "you".to_string()
            },
            pup_name: if role == "assistant" {
                "Alpha".to_string()
            } else {
                "You".to_string()
            },
            role,
            content,
            timestamp,
        })
        .collect())
}

// ─── Skills ──────────────────────────────────────────────────────────────────

fn registry<'a>(state: &'a State<'_, AppState>) -> &'a crate::skills::registry::SkillRegistry {
    &state.alpha.skill_executor.registry
}

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<InstalledSkill>, String> {
    Ok(registry(&state).list_installed().await)
}

#[tauri::command]
pub async fn set_skill_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    registry(&state)
        .set_enabled(&name, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_skill(
    state: State<'_, AppState>,
    name: String,
    input: String,
) -> Result<String, String> {
    state
        .alpha
        .skill_executor
        .execute_skill(&name, &input)
        .await
        .map_err(|e| e.to_string())
}

// ─── MCP servers ─────────────────────────────────────────────────────────────

fn mcp<'a>(state: &'a State<'_, AppState>) -> &'a Arc<MCPOrchestrator> {
    &state.alpha.mcp_orchestrator
}

#[derive(Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub base_url: String,
    pub description: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerInfo>, String> {
    let servers = mcp(&state).list_servers().await;
    Ok(servers
        .into_iter()
        .map(|e| McpServerInfo {
            name: e.name,
            base_url: e.base_url,
            description: e.description,
            enabled: e.enabled,
        })
        .collect())
}

#[derive(Deserialize)]
pub struct AddMcpServerInput {
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    entry: AddMcpServerInput,
) -> Result<(), String> {
    mcp(&state)
        .add_server(McpServerEntry {
            name: entry.name,
            base_url: entry.base_url,
            token: entry.token,
            description: entry.description,
            enabled: true,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    mcp(&state)
        .remove_server(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_mcp_server(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    mcp(&state)
        .toggle_server(&name, enabled)
        .await
        .map_err(|e| e.to_string())
}

// ─── Pup management ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_pups(state: State<'_, AppState>) -> Result<Vec<PupConfig>, String> {
    Ok(state.alpha.list_pup_configs().await)
}

#[tauri::command]
pub async fn update_pup(
    state: State<'_, AppState>,
    key: String,
    system_prompt_override: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .alpha
        .update_pup_config(&key, system_prompt_override, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_custom_pup(
    state: State<'_, AppState>,
    key: String,
    display_name: String,
    description: String,
    system_prompt: String,
) -> Result<(), String> {
    state
        .alpha
        .add_custom_pup(key, display_name, description, system_prompt)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_custom_pup(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state
        .alpha
        .remove_custom_pup(&key)
        .await
        .map_err(|e| e.to_string())
}

// ─── MCP tool discovery ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    Ok(state.alpha.mcp_orchestrator.list_all_tools().await)
}

#[tauri::command]
pub async fn refresh_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    state.alpha.mcp_orchestrator.refresh_all_tools().await;
    Ok(state.alpha.mcp_orchestrator.list_all_tools().await)
}

// ─── Skill run history ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SkillRunItem {
    pub id: String,
    pub skill_name: String,
    pub triggered_by: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub output: Option<String>,
}

impl From<SkillRunRecord> for SkillRunItem {
    fn from(r: SkillRunRecord) -> Self {
        Self {
            id: r.id,
            skill_name: r.skill_name,
            triggered_by: r.triggered_by,
            started_at: r.started_at,
            completed_at: r.completed_at,
            status: r.status,
            output: r.output,
        }
    }
}

#[tauri::command]
pub async fn list_skill_runs(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<SkillRunItem>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory
        .list_skill_runs(limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(SkillRunItem::from).collect())
}

// ─── Memory diary ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_diary_dates(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.file_layer.list_diary_dates())
}

#[tauri::command]
pub async fn read_diary_entry(state: State<'_, AppState>, date: String) -> Result<String, String> {
    state
        .file_layer
        .read_diary(&date)
        .map_err(|e| e.to_string())
}

// ─── Workspace ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_workspace() -> Result<String, String> {
    export_workspace_default().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_workspace(backup_path: String) -> Result<(), String> {
    import_workspace_from_path(&backup_path)
        .await
        .map_err(|e| e.to_string())
}

// ─── Config ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_llm_provider(state: State<'_, AppState>) -> Result<String, String> {
    let provider = state.alpha.llm_client.provider();
    let name = match provider {
        Provider::OpenAI => "openai",
        Provider::Ollama => "ollama",
    };
    Ok(name.to_string())
}

#[derive(Serialize)]
pub struct LlmConfigInfo {
    pub provider: String,
    pub model: String,
    pub mini_model: String,
    pub embed_model: String,
    pub api_base: Option<String>,
}

#[tauri::command]
pub async fn get_llm_config(state: State<'_, AppState>) -> Result<LlmConfigInfo, String> {
    let (provider, model, mini_model, embed_model, _api_key, api_base) =
        state.alpha.llm_client.current_config();
    let provider_str = match provider {
        Provider::OpenAI => "openai".to_string(),
        Provider::Ollama => "ollama".to_string(),
    };
    Ok(LlmConfigInfo {
        provider: provider_str,
        model,
        mini_model,
        embed_model,
        api_base,
    })
}

// ─── Safe config (LLM-visible, no secrets) ───────────────────────────────────

/// A sanitised view of the app config that the LLM can safely read.
/// `api_key` is intentionally absent — the LLM must never see it.
#[derive(Serialize)]
pub struct SafeConfig {
    /// User-configured extra skill directories (from config.toml [skills]).
    pub skills_search_paths: Vec<String>,
    /// Primary LLM model name.
    pub llm_model: String,
    /// LLM provider ("openai" | "ollama").
    pub llm_provider: String,
    /// Base URL override (empty = provider default).
    pub llm_api_base: String,
    /// Whether the api_key is configured (without revealing it).
    pub llm_api_key_set: bool,
}

#[tauri::command]
pub async fn get_safe_config() -> Result<SafeConfig, String> {
    let cfg = crate::config::load();
    Ok(SafeConfig {
        skills_search_paths: cfg.skills.search_paths,
        llm_model: cfg.llm.model,
        llm_provider: cfg.llm.provider,
        llm_api_base: cfg.llm.api_base,
        llm_api_key_set: !cfg.llm.api_key.is_empty(),
    })
}

#[tauri::command]
pub async fn set_llm_provider(
    state: State<'_, AppState>,
    provider: String,
    model: String,
    mini_model: Option<String>,
    embed_model: Option<String>,
    api_key: Option<String>,
    api_base: Option<String>,
) -> Result<(), String> {
    let p = match provider.as_str() {
        "ollama" => Provider::Ollama,
        _ => Provider::OpenAI,
    };
    state.alpha.llm_client.reconfigure(
        p,
        model.clone(),
        mini_model.clone(),
        embed_model.clone(),
        api_key.clone(),
        api_base.clone(),
    );

    // Persist to ~/.openpup/config.toml
    let mut cfg = crate::config::load();
    cfg.llm.provider = provider;
    cfg.llm.model = model;
    if let Some(mm) = mini_model {
        cfg.llm.mini_model = mm;
    }
    if let Some(em) = embed_model {
        cfg.llm.embed_model = em;
    }
    if let Some(k) = api_key {
        cfg.llm.api_key = k;
    }
    if let Some(b) = api_base {
        cfg.llm.api_base = b;
    }
    crate::config::save(&cfg).map_err(|e| e.to_string())?;

    Ok(())
}

/// Quick model switch from chat header — only changes the primary model field.
#[tauri::command]
pub async fn quick_set_model(state: State<'_, AppState>, model: String) -> Result<(), String> {
    let (provider, _old_model, mini_model, embed_model, api_key, api_base) =
        state.alpha.llm_client.current_config();
    state.alpha.llm_client.reconfigure(
        provider,
        model.clone(),
        Some(mini_model),
        Some(embed_model),
        api_key.clone(),
        api_base.clone(),
    );
    let mut cfg = crate::config::load();
    cfg.llm.provider = match provider {
        Provider::OpenAI => "openai",
        Provider::Ollama => "ollama",
    }
    .to_string();
    cfg.llm.model = model;
    if let Some(k) = api_key {
        cfg.llm.api_key = k;
    }
    if let Some(b) = api_base {
        cfg.llm.api_base = b;
    }
    crate::config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_bridge_config() -> Result<BridgeConfig, String> {
    Ok(crate::bridge::control::get_bridge_config())
}

#[tauri::command]
pub async fn save_bridge_config(
    state: State<'_, AppState>,
    config: BridgeConfig,
) -> Result<(), String> {
    crate::bridge::control::save_bridge_config(Some(&state.bridge_manager), config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_bridge_status(
    state: State<'_, AppState>,
) -> Result<Vec<BridgeConnectionStatus>, String> {
    crate::bridge::control::get_bridge_status(&state.alpha.memory)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_weixin_qr_login(
    state: State<'_, AppState>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    account_id: Option<String>,
    bot_type: Option<String>,
    force: Option<bool>,
) -> Result<crate::bridge::weixin::WeixinQrStartResult, String> {
    crate::bridge::control::start_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        base_url,
        proxy_url,
        route_tag,
        account_id,
        bot_type,
        force.unwrap_or(false),
    )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn wait_weixin_qr_login(
    state: State<'_, AppState>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    session_key: String,
    bot_type: Option<String>,
    timeout_ms: Option<i64>,
) -> Result<crate::bridge::weixin::WeixinQrWaitResult, String> {
    crate::bridge::control::wait_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        Some(&state.bridge_manager),
        base_url,
        proxy_url,
        route_tag,
        session_key,
        bot_type,
        timeout_ms,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_weixin_qr_login(
    state: State<'_, AppState>,
    session_key: String,
) -> Result<(), String> {
    crate::bridge::control::cancel_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        &session_key,
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn list_weixin_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<crate::bridge::weixin::StoredWeixinAccount>, String> {
    Ok(crate::bridge::control::list_weixin_accounts(
        &state.bridge_manager.weixin_service(),
    ))
}

#[tauri::command]
pub async fn activate_weixin_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<BridgeConfig, String> {
    crate::bridge::control::activate_weixin_account(
        &state.bridge_manager.weixin_service(),
        Some(&state.bridge_manager),
        &account_id,
    )
    .await
    .map_err(|e| e.to_string())
}

// ─── Conversation search ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConversationSearchResult {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[tauri::command]
pub async fn search_conversations(
    state: State<'_, AppState>,
    query: String,
    limit: i64,
) -> Result<Vec<ConversationSearchResult>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory
        .search_conversations(&query, limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(role, content, timestamp)| ConversationSearchResult {
            role,
            content,
            timestamp,
        })
        .collect())
}

// ─── Task tracking ────────────────────────────────────────────────────────────

use crate::memory::system::TaskRecord;

#[derive(Serialize)]
pub struct TaskItem {
    pub id: String,
    pub description: String,
    pub assigned_pup: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub result: Option<String>,
}

impl From<TaskRecord> for TaskItem {
    fn from(r: TaskRecord) -> Self {
        Self {
            id: r.id,
            description: r.description,
            assigned_pup: r.assigned_pup,
            status: r.status,
            created_at: r.created_at,
            completed_at: r.completed_at,
            result: r.result,
        }
    }
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    description: String,
    assigned_pup: Option<String>,
) -> Result<String, String> {
    let memory = memory_system_from_state(&state);
    memory
        .create_task(&description, assigned_pup.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>, limit: i64) -> Result<Vec<TaskItem>, String> {
    let memory = memory_system_from_state(&state);
    let rows = memory.list_tasks(limit).await.map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(TaskItem::from).collect())
}

#[tauri::command]
pub async fn update_task_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
    result: Option<String>,
) -> Result<(), String> {
    let memory = memory_system_from_state(&state);
    memory
        .update_task_status(&id, &status, result.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let memory = memory_system_from_state(&state);
    memory.delete_task(&id).await.map_err(|e| e.to_string())
}

// ─── Permissions ──────────────────────────────────────────────────────────────

/// Called by the frontend when the user clicks "Allow" in the PermissionDialog.
#[tauri::command]
pub async fn approve_permission(
    checker: State<'_, PermissionChecker>,
    request_id: String,
    skill_name: String,
    remember: bool,
) -> Result<(), String> {
    if remember {
        checker.trust_skill(&skill_name).await;
    }
    checker.respond(&request_id, true);
    Ok(())
}

/// Called by the frontend when the user clicks "Deny" in the PermissionDialog.
#[tauri::command]
pub async fn deny_permission(
    checker: State<'_, PermissionChecker>,
    request_id: String,
) -> Result<(), String> {
    checker.respond(&request_id, false);
    Ok(())
}

/// Return the current execution mode ("leashed" | "free_run").
#[tauri::command]
pub async fn get_execution_mode(checker: State<'_, PermissionChecker>) -> Result<String, String> {
    let mode = checker.get_mode().await;
    Ok(match mode {
        ExecutionMode::Leashed => "leashed".to_string(),
        ExecutionMode::FreeRun => "free_run".to_string(),
    })
}

/// Switch between leashed and free_run modes.
#[tauri::command]
pub async fn set_execution_mode(
    checker: State<'_, PermissionChecker>,
    mode: String,
) -> Result<(), String> {
    let m = match mode.as_str() {
        "free_run" => ExecutionMode::FreeRun,
        _ => ExecutionMode::Leashed,
    };
    checker.set_mode(m).await;
    Ok(())
}

/// Open a URL in the system's default browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| e.to_string())
}

/// List recent Pack Channels.
#[tauri::command]
pub async fn list_channels(
    state: State<'_, AppState>,
) -> Result<Vec<crate::channel::types::ChannelRecord>, String> {
    state
        .alpha
        .memory
        .list_channels(50)
        .await
        .map_err(|e| e.to_string())
}

/// Get the number of currently active Pack Channels.
#[tauri::command]
pub async fn get_active_channel_count(state: State<'_, AppState>) -> Result<i64, String> {
    state
        .alpha
        .channel_manager
        .active_count()
        .await
        .map_err(|e| e.to_string())
}

/// Get all messages for a specific channel.
#[tauri::command]
pub async fn get_channel_messages(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Vec<crate::channel::types::ChannelMessageRecord>, String> {
    state
        .alpha
        .memory
        .get_channel_messages(&channel_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get the persisted delegation plan for a specific channel.
#[tauri::command]
pub async fn get_channel_plan(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Option<crate::channel::types::DelegationPlan>, String> {
    state
        .alpha
        .memory
        .get_channel_plan(&channel_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get the current workflow state for a specific Pack Channel.
#[tauri::command]
pub async fn get_channel_workflow_state(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Option<crate::channel::types::ChannelWorkflowState>, String> {
    state
        .alpha
        .channel_manager
        .workflow_state(&channel_id)
        .await
        .map_err(|e| e.to_string())
}

/// Add a review comment without unblocking execution.
#[tauri::command]
pub async fn submit_channel_review_comment(
    state: State<'_, AppState>,
    channel_id: String,
    comment: String,
    reply_to: Option<String>,
    sender: Option<String>,
) -> Result<(), String> {
    state
        .alpha
        .channel_manager
        .submit_review_comment(
            &channel_id,
            sender.as_deref().unwrap_or("you"),
            &comment,
            reply_to.as_deref(),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Request changes for the current review gate.
#[tauri::command]
pub async fn request_channel_changes(
    state: State<'_, AppState>,
    channel_id: String,
    comment: String,
    reply_to: Option<String>,
    sender: Option<String>,
) -> Result<(), String> {
    state
        .alpha
        .channel_manager
        .request_changes(
            &channel_id,
            sender.as_deref().unwrap_or("you"),
            &comment,
            reply_to.as_deref(),
            None,
        )
        .await
        .map_err(|e| e.to_string())
}

/// Explicitly continue a paused Pack Channel.
#[tauri::command]
pub async fn continue_channel(
    state: State<'_, AppState>,
    channel_id: String,
    comment: Option<String>,
    sender: Option<String>,
) -> Result<(), String> {
    state
        .alpha
        .channel_manager
        .continue_channel(
            &channel_id,
            sender.as_deref().unwrap_or("you"),
            comment
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("继续"),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Delete all completed Pack Channels and their persisted messages/members.
#[tauri::command]
pub async fn clear_completed_channels(state: State<'_, AppState>) -> Result<i64, String> {
    state
        .alpha
        .memory
        .clear_completed_channels()
        .await
        .map_err(|e| e.to_string())
}

/// Delete active Pack Channels that have not updated for longer than max_age_seconds.
#[tauri::command]
pub async fn clear_stale_channels(
    state: State<'_, AppState>,
    max_age_seconds: Option<i64>,
) -> Result<i64, String> {
    state
        .alpha
        .memory
        .clear_stale_active_channels(max_age_seconds.unwrap_or(30 * 60))
        .await
        .map_err(|e| e.to_string())
}

// ─── Pack page — per-pup conversation ─────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct ConvMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Load conversation history for a specific pup.
#[tauri::command]
pub async fn get_pup_conversation(
    state: State<'_, AppState>,
    pup_key: String,
    limit: Option<i64>,
    before_timestamp: Option<i64>,
) -> Result<Vec<ConvMessage>, String> {
    let limit = limit.unwrap_or(100);
    let rows = state
        .alpha
        .memory
        .get_pup_conversation_display(&pup_key, limit, before_timestamp)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(role, content, timestamp)| ConvMessage {
            role,
            content,
            timestamp,
        })
        .collect())
}

/// Count messages for a pup (for context indicator).
#[tauri::command]
pub async fn get_pup_message_count(
    state: State<'_, AppState>,
    pup_key: String,
) -> Result<i64, String> {
    state
        .alpha
        .memory
        .get_pup_message_count(&pup_key)
        .await
        .map_err(|e| e.to_string())
}

/// Clear all conversation history for a pup (context reset).
#[tauri::command]
pub async fn clear_pup_history(state: State<'_, AppState>, pup_key: String) -> Result<(), String> {
    state
        .alpha
        .memory
        .clear_pup_conversation(&pup_key)
        .await
        .map_err(|e| e.to_string())
}

/// Manually trigger context compression for a pup.
#[tauri::command]
pub async fn compress_pup_context(
    state: State<'_, AppState>,
    pup_key: String,
) -> Result<(), String> {
    state
        .alpha
        .compress_pup_context_now(&pup_key)
        .await
        .map_err(|e| e.to_string())
}

/// Compression status for a pup's context.
#[derive(Serialize)]
pub struct CompressionStatus {
    pub is_compressed: bool,
    pub last_compression_row: i64,
}

/// Overall context statistics for a pup.
#[derive(Serialize)]
pub struct ContextStats {
    pub pup_key: String,
    pub message_count: i64,
    pub estimated_tokens: usize,
    pub compression_status: CompressionStatus,
}

/// Fetch comprehensive context statistics for a pup (message count, estimated tokens, compression status).
#[tauri::command]
pub async fn get_context_stats(
    state: State<'_, AppState>,
    pup_key: String,
) -> Result<ContextStats, String> {
    let memory = memory_system_from_state(&state);

    // Get message count
    let message_count = memory
        .get_pup_message_count(&pup_key)
        .await
        .map_err(|e| e.to_string())?;

    // Get compression status
    let (is_compressed, last_compression_row) = memory
        .get_compression_status(&pup_key)
        .await
        .map_err(|e| e.to_string())?;

    // Estimate tokens: conservatively assume ~100 tokens per message
    let estimated_tokens = (message_count as usize) * 100;

    Ok(ContextStats {
        pup_key,
        message_count,
        estimated_tokens,
        compression_status: CompressionStatus {
            is_compressed,
            last_compression_row,
        },
    })
}

// ─── Scheduled jobs ───────────────────────────────────────────────────────────

/// Return all scheduled jobs from ~/.openpup/scheduled_jobs.json.
#[tauri::command]
pub async fn list_scheduled_jobs(
    _state: State<'_, AppState>,
) -> Result<Vec<crate::skills::job_registry::ScheduledJob>, String> {
    let home_dir = dirs::home_dir().ok_or("cannot determine home directory")?;
    let jobs_path = home_dir.join(".openpup").join("scheduled_jobs.json");
    let registry = crate::skills::job_registry::JobRegistry::new(jobs_path);
    Ok(registry.load())
}

/// Delete a scheduled job by id.
#[tauri::command]
pub async fn delete_scheduled_job(_state: State<'_, AppState>, id: String) -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("cannot determine home directory")?;
    let jobs_path = home_dir.join(".openpup").join("scheduled_jobs.json");
    let registry = crate::skills::job_registry::JobRegistry::new(jobs_path);
    registry.delete(&id).map_err(|e| e.to_string())
}
