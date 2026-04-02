use serde::Serialize;
use tauri::State;

use crate::workspace::backup::{export_workspace_default, import_workspace_from_path};

use super::AppState;

#[tauri::command]
pub async fn export_workspace(state: State<'_, AppState>) -> Result<String, String> {
    if state.is_mobile_runtime() {
        return Err("workspace export is not supported on mobile yet".to_string());
    }
    export_workspace_default().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_workspace(
    state: State<'_, AppState>,
    backup_path: String,
) -> Result<(), String> {
    if state.is_mobile_runtime() {
        return Err("workspace import is not supported on mobile yet".to_string());
    }
    import_workspace_from_path(&backup_path)
        .await
        .map_err(|e| e.to_string())
}

/// Open a URL in the system's default browser.
#[tauri::command]
pub fn open_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    if state.is_mobile_runtime() {
        return Err("open_url is not supported on mobile yet".to_string());
    }
    open::that(&url).map_err(|e| e.to_string())
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
    let rows = state
        .app
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
