use serde::Serialize;
use tauri::State;

use super::AppState;

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
    let memory = state.alpha.memory.clone();
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
