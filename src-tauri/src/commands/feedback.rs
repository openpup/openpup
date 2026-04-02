use tauri::State;

use super::AppState;

/// Submit thumbs-up/down feedback on a message.
#[tauri::command]
pub async fn submit_message_feedback(
    state: State<'_, AppState>,
    message_id: String,
    channel_id: Option<String>,
    feedback: Option<String>,
) -> Result<(), String> {
    match feedback.as_deref() {
        Some(f @ ("up" | "down")) => state
            .app
            .memory
            .upsert_message_feedback(&message_id, channel_id.as_deref(), f)
            .await
            .map_err(|e| e.to_string()),
        None => state
            .app
            .memory
            .delete_message_feedback(&message_id)
            .await
            .map_err(|e| e.to_string()),
        Some(other) => Err(format!("invalid feedback value: {other}")),
    }
}

/// Save artifact content to a file on disk.
#[tauri::command]
pub async fn save_artifact_to_file(
    state: State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), String> {
    if state.is_mobile_runtime() {
        return Err("save_artifact_to_file is not supported on mobile".to_string());
    }
    std::fs::write(&path, &content).map_err(|e| e.to_string())
}
