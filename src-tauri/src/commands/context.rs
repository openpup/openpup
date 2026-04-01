use serde::Serialize;
use tauri::State;

use super::AppState;

// ─── Per-pup conversation ─────────────────────────────────────────────────────

#[derive(Serialize)]
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
        .app
        .pup_conversation_display(&pup_key, limit, before_timestamp)
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
    state.app.pup_message_count(&pup_key).await.map_err(|e| e.to_string())
}

/// Clear all conversation history for a pup (context reset).
#[tauri::command]
pub async fn clear_pup_history(state: State<'_, AppState>, pup_key: String) -> Result<(), String> {
    state.app.clear_pup_history(&pup_key).await.map_err(|e| e.to_string())
}

/// Manually trigger context compression for a pup.
#[tauri::command]
pub async fn compress_pup_context(
    state: State<'_, AppState>,
    pup_key: String,
) -> Result<(), String> {
    state.app.compress_pup_context(&pup_key).await.map_err(|e| e.to_string())
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
    /// Real prompt token count from the last API call for this pup (0 if no data yet).
    pub context_tokens: u64,
    /// Model's context window limit in tokens.
    pub context_limit: u64,
    pub compression_status: CompressionStatus,
}

/// Fetch comprehensive context statistics for a pup (real token counts, compression status).
#[tauri::command]
pub async fn get_context_stats(
    state: State<'_, AppState>,
    pup_key: String,
) -> Result<ContextStats, String> {
    let message_count = state
        .app
        .pup_message_count(&pup_key)
        .await
        .map_err(|e| e.to_string())?;

    let (is_compressed, last_compression_row) = state
        .app
        .compression_status(&pup_key)
        .await
        .map_err(|e| e.to_string())?;

    let context_tokens = state.app.context_tokens(&pup_key).await;
    let context_limit = state.app.context_limit();

    Ok(ContextStats {
        pup_key,
        message_count,
        context_tokens,
        context_limit,
        compression_status: CompressionStatus {
            is_compressed,
            last_compression_row,
        },
    })
}

/// Manually trigger multi-layer compaction and return results.
#[tauri::command]
pub async fn compact_pup_context(
    state: State<'_, AppState>,
    pup_key: String,
    force_level: Option<String>,
) -> Result<Vec<CompactionResultInfo>, String> {
    let context_limit = state.app.context_limit();
    let current_tokens = state.app.context_tokens(&pup_key).await;

    // Allow forcing a specific pressure level for debugging
    let simulated_tokens = match force_level.as_deref() {
        Some("moderate") => (context_limit as f64 * 0.50) as u64,
        Some("high") => (context_limit as f64 * 0.70) as u64,
        Some("critical") => (context_limit as f64 * 0.90) as u64,
        _ => current_tokens.max((context_limit as f64 * 0.70) as u64),
    };

    let results = state
        .app
        .compact_pup_context(&pup_key, simulated_tokens)
        .await
        .map_err(|e| e.to_string())?;

    Ok(results
        .into_iter()
        .map(|r| CompactionResultInfo {
            strategy: r.strategy.to_string(),
            messages_before: r.messages_before,
            messages_after: r.messages_after,
            estimated_tokens_saved: r.estimated_tokens_saved,
        })
        .collect())
}

#[derive(Serialize)]
pub struct CompactionResultInfo {
    pub strategy: String,
    pub messages_before: usize,
    pub messages_after: usize,
    pub estimated_tokens_saved: u64,
}

// ─── Token usage ─────────────────────────────────────────────────────────────

/// Return cumulative session token usage from the LLM client.
#[tauri::command]
pub async fn get_token_usage(
    state: State<'_, AppState>,
) -> Result<crate::llm::client::TokenUsage, String> {
    Ok(state.app.token_usage_snapshot())
}

/// Reset the cumulative session token counters.
#[tauri::command]
pub async fn reset_token_usage(state: State<'_, AppState>) -> Result<(), String> {
    state.app.reset_token_usage();
    Ok(())
}
