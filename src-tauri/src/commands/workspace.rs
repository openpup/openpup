use serde::Serialize;
use tauri::State;

use crate::workspace::backup::{export_workspace_default, import_workspace_from_path};

use super::AppState;

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

/// Open a URL in the system's default browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
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
    let memory = state.alpha.memory.clone();
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
