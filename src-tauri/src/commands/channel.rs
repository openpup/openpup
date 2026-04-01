use tauri::State;

use super::AppState;

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

/// Abort a Pack Channel during review — terminates execution immediately.
#[tauri::command]
pub async fn abort_channel(
    state: State<'_, AppState>,
    channel_id: String,
    comment: Option<String>,
    sender: Option<String>,
) -> Result<(), String> {
    state
        .alpha
        .channel_manager
        .abort_channel(
            &channel_id,
            sender.as_deref().unwrap_or("you"),
            comment
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("terminated by owner"),
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
