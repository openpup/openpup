use std::sync::Arc;

use tauri::{Emitter, State};
use tracing::debug;

use super::AppState;

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
    let (providers, routing) = state.app.current_llm_routing();
    let primary_provider = providers
        .iter()
        .find(|provider| provider.id == routing.primary.provider_id)
        .or_else(|| providers.first());
    let model = routing.primary.model.clone();
    let mini_model = routing.mini.model.clone();
    let api_base = primary_provider.map(|provider| provider.api_base.clone());
    let has_key = primary_provider
        .map(|provider| provider.kind == "ollama" || !provider.api_key.trim().is_empty())
        .unwrap_or(false);
    debug!(
        "[cmd] send_message: model={model:?} mini={mini_model:?} base={api_base:?} has_key={}",
        has_key
    );
    if !state.app.llm_primary_ready() || !has_key {
        let config_path = crate::config::config_path()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "the app config file".to_string());
        let _ = app_handle.emit(
      "stream_error",
      format!("未配置可用的主模型 Provider。请在 `{}` 中配置 llm.providers 和 llm.routing，或在设置页里添加 Provider 并指定主模型。", config_path)
    );
        return Ok(());
    }

    let event_sink = Arc::new(crate::runtime_tauri::TauriEventSink::new(app_handle));
    let app = state.app.clone();
    tauri::async_runtime::spawn(async move {
        app.process_user_message_stream(input, forced_pup, event_sink)
            .await;
    });
    Ok(())
}

/// Cancel the current in-progress streaming response.
#[tauri::command]
pub async fn abort_message(state: State<'_, AppState>) -> Result<(), String> {
    state.app.abort_current_message();
    Ok(())
}
