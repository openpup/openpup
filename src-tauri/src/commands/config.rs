use anyhow::Result;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::{
    default_api_base_for_provider, load, save, LlmProviderConfig, LlmRouteTarget, LlmRoutingConfig,
};

use super::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPayload {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub provider: String,
    pub api_base: String,
    pub api_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTargetPayload {
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingPayload {
    pub primary: RouteTargetPayload,
    pub mini: RouteTargetPayload,
    pub embedding: RouteTargetPayload,
}

#[derive(Serialize)]
pub struct LlmConfigInfo {
    pub provider: String,
    pub model: String,
    pub mini_model: String,
    pub embed_model: String,
    pub api_base: Option<String>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub message: String,
    pub models_found: usize,
}

fn payload_from_provider(provider: &LlmProviderConfig) -> ProviderPayload {
    ProviderPayload {
        id: provider.id.clone(),
        name: provider.name.clone(),
        kind: provider.kind.clone(),
        provider: provider.provider.clone(),
        api_base: provider.api_base.clone(),
        api_key: String::new(),
        enabled: provider.enabled,
        models: provider.models.clone(),
    }
}

fn provider_from_payload(payload: ProviderPayload) -> LlmProviderConfig {
    let kind = if payload.kind.trim().is_empty() {
        if payload.provider.eq_ignore_ascii_case("ollama") {
            "ollama".to_string()
        } else {
            "openai_compatible".to_string()
        }
    } else {
        payload.kind
    };
    LlmProviderConfig {
        id: payload.id.trim().to_string(),
        name: payload.name.trim().to_string(),
        kind: kind.clone(),
        provider: payload.provider.trim().to_string(),
        api_base: if payload.api_base.trim().is_empty() {
            default_api_base_for_provider(&kind, &payload.provider)
        } else {
            payload.api_base.trim().to_string()
        },
        api_key: payload.api_key,
        enabled: payload.enabled,
        models: payload
            .models
            .into_iter()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty())
            .collect(),
    }
}

fn payload_to_routing(payload: RoutingPayload) -> LlmRoutingConfig {
    LlmRoutingConfig {
        primary: LlmRouteTarget {
            provider_id: payload.primary.provider_id,
            model: payload.primary.model,
        },
        mini: LlmRouteTarget {
            provider_id: payload.mini.provider_id,
            model: payload.mini.model,
        },
        embedding: LlmRouteTarget {
            provider_id: payload.embedding.provider_id,
            model: payload.embedding.model,
        },
    }
}

fn routing_to_payload(routing: &LlmRoutingConfig) -> RoutingPayload {
    RoutingPayload {
        primary: RouteTargetPayload {
            provider_id: routing.primary.provider_id.clone(),
            model: routing.primary.model.clone(),
        },
        mini: RouteTargetPayload {
            provider_id: routing.mini.provider_id.clone(),
            model: routing.mini.model.clone(),
        },
        embedding: RouteTargetPayload {
            provider_id: routing.embedding.provider_id.clone(),
            model: routing.embedding.model.clone(),
        },
    }
}

fn reload_runtime(state: &State<'_, AppState>) {
    state.app.reload_llm_from_config();
}

fn normalize_provider_for_test(mut provider: LlmProviderConfig) -> LlmProviderConfig {
    if provider.api_base.trim().is_empty() {
        provider.api_base = default_api_base_for_provider(&provider.kind, &provider.provider);
    }
    provider
}

fn ollama_root(api_base: &str) -> String {
    api_base
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

async fn fetch_provider_models(provider: &LlmProviderConfig) -> Result<Vec<String>> {
    let provider = normalize_provider_for_test(provider.clone());
    let client = reqwest::Client::new();
    if provider.kind == "ollama" {
        let url = format!("{}/api/tags", ollama_root(&provider.api_base));
        let val: serde_json::Value = client.get(&url).send().await?.error_for_status()?.json().await?;
        let models = val["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["name"].as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        return Ok(models);
    }

    let url = format!("{}/models", provider.api_base.trim_end_matches('/'));
    let mut req = client
        .get(&url)
        .header(CONTENT_TYPE, "application/json");
    if !provider.api_key.trim().is_empty() {
        req = req.header(AUTHORIZATION, format!("Bearer {}", provider.api_key));
    }
    let val: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
    let models = val["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    Ok(models)
}

#[tauri::command]
pub async fn get_llm_provider(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.app.llm_provider_name())
}

#[tauri::command]
pub async fn get_llm_config(_state: State<'_, AppState>) -> Result<LlmConfigInfo, String> {
    let cfg = load();
    Ok(LlmConfigInfo {
        provider: cfg.llm.provider,
        model: cfg.llm.model,
        mini_model: cfg.llm.mini_model,
        embed_model: cfg.llm.embed_model,
        api_base: if cfg.llm.api_base.trim().is_empty() {
            None
        } else {
            Some(cfg.llm.api_base)
        },
    })
}

#[tauri::command]
pub async fn get_safe_config() -> Result<SafeConfig, String> {
    let cfg = load();
    Ok(SafeConfig {
        skills_search_paths: cfg.skills.search_paths,
        llm_model: cfg.llm.model,
        llm_provider: cfg.llm.provider,
        llm_api_base: cfg.llm.api_base,
        llm_api_key_set: !cfg.llm.api_key.is_empty(),
    })
}

#[tauri::command]
pub async fn list_llm_providers() -> Result<Vec<ProviderPayload>, String> {
    let cfg = load();
    Ok(cfg
        .llm
        .providers
        .iter()
        .map(payload_from_provider)
        .collect())
}

#[tauri::command]
pub async fn get_llm_routing() -> Result<RoutingPayload, String> {
    let cfg = load();
    Ok(routing_to_payload(&cfg.llm.routing))
}

#[tauri::command]
pub async fn save_llm_provider(
    state: State<'_, AppState>,
    provider: ProviderPayload,
) -> Result<(), String> {
    let mut cfg = load();
    let provider = provider_from_payload(provider);
    if provider.id.is_empty() {
        return Err("Provider ID 不能为空".to_string());
    }
    if provider.name.is_empty() {
        return Err("Provider 名称不能为空".to_string());
    }
    if let Some(existing) = cfg
        .llm
        .providers
        .iter_mut()
        .find(|item| item.id == provider.id)
    {
        *existing = provider;
    } else {
        cfg.llm.providers.push(provider);
    }
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_llm_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<(), String> {
    let mut cfg = load();
    cfg.llm.providers.retain(|provider| provider.id != provider_id);
    if cfg.llm.routing.primary.provider_id == provider_id {
        cfg.llm.routing.primary.provider_id.clear();
    }
    if cfg.llm.routing.mini.provider_id == provider_id {
        cfg.llm.routing.mini.provider_id.clear();
    }
    if cfg.llm.routing.embedding.provider_id == provider_id {
        cfg.llm.routing.embedding.provider_id.clear();
    }
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(())
}

#[tauri::command]
pub async fn set_llm_routing(
    state: State<'_, AppState>,
    routing: RoutingPayload,
) -> Result<(), String> {
    let mut cfg = load();
    cfg.llm.routing = payload_to_routing(routing);
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(())
}

#[tauri::command]
pub async fn test_llm_provider(provider: ProviderPayload) -> Result<ProviderTestResult, String> {
    let provider = provider_from_payload(provider);
    match fetch_provider_models(&provider).await {
        Ok(models) => Ok(ProviderTestResult {
            ok: true,
            message: if models.is_empty() {
                "连接成功，但未返回模型列表".to_string()
            } else {
                format!("连接成功，发现 {} 个模型", models.len())
            },
            models_found: models.len(),
        }),
        Err(err) => Ok(ProviderTestResult {
            ok: false,
            message: err.to_string(),
            models_found: 0,
        }),
    }
}

#[tauri::command]
pub async fn refresh_llm_provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let mut cfg = load();
    let provider = cfg
        .llm
        .providers
        .iter()
        .find(|item| item.id == provider_id)
        .cloned()
        .ok_or_else(|| "未找到对应的 Provider".to_string())?;
    let models = fetch_provider_models(&provider)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(existing) = cfg
        .llm
        .providers
        .iter_mut()
        .find(|item| item.id == provider_id)
    {
        existing.models = models.clone();
    }
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(models)
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
    let mut cfg = load();
    let default_id = "default".to_string();
    let kind = if provider == "ollama" {
        "ollama".to_string()
    } else {
        "openai_compatible".to_string()
    };
    let provider_name = provider.clone();
    let default_provider = LlmProviderConfig {
        id: default_id.clone(),
        name: "Default Provider".to_string(),
        kind: kind.clone(),
        provider: provider_name.clone(),
        api_base: api_base.unwrap_or_else(|| default_api_base_for_provider(&kind, &provider_name)),
        api_key: api_key.unwrap_or_default(),
        enabled: true,
        models: vec![
            model.clone(),
            mini_model.clone().unwrap_or_else(|| model.clone()),
            embed_model
                .clone()
                .unwrap_or_else(|| cfg.llm.embed_model.clone()),
        ],
    };
    if let Some(existing) = cfg
        .llm
        .providers
        .iter_mut()
        .find(|item| item.id == default_id)
    {
        *existing = default_provider;
    } else {
        cfg.llm.providers.push(default_provider);
    }
    cfg.llm.routing = LlmRoutingConfig {
        primary: LlmRouteTarget {
            provider_id: default_id.clone(),
            model: model.clone(),
        },
        mini: LlmRouteTarget {
            provider_id: default_id.clone(),
            model: mini_model.unwrap_or_else(|| model.clone()),
        },
        embedding: LlmRouteTarget {
            provider_id: default_id,
            model: embed_model.unwrap_or_else(|| cfg.llm.embed_model.clone()),
        },
    };
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(())
}

/// Quick model switch from chat header — only changes the primary model field.
#[tauri::command]
pub async fn quick_set_model(state: State<'_, AppState>, model: String) -> Result<(), String> {
    let mut cfg = load();
    if model.trim().is_empty() {
        return Err("模型名不能为空".to_string());
    }
    cfg.llm.routing.primary.model = model;
    save(&cfg).map_err(|e| e.to_string())?;
    reload_runtime(&state);
    Ok(())
}
