use serde::Serialize;
use tauri::State;

use crate::llm::client::Provider;

use super::AppState;

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

/// A sanitised view of the app config that the LLM can safely read.
#[derive(Serialize)]
pub struct SafeConfig {
    pub skills_search_paths: Vec<String>,
    pub llm_model: String,
    pub llm_provider: String,
    pub llm_api_base: String,
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
