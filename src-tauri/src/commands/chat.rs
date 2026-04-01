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
