use tauri::State;

use crate::agents::alpha::{PupConfig, PupPermissionConfig};

use super::AppState;

#[tauri::command]
pub async fn list_pups(state: State<'_, AppState>) -> Result<Vec<PupConfig>, String> {
    Ok(state.alpha.list_pup_configs().await)
}

#[tauri::command]
pub async fn update_pup(
    state: State<'_, AppState>,
    key: String,
    system_prompt_override: String,
    enabled: bool,
    permissions: Option<PupPermissionConfig>,
) -> Result<(), String> {
    state
        .alpha
        .update_pup_config(&key, system_prompt_override, enabled, permissions)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_custom_pup(
    state: State<'_, AppState>,
    key: String,
    display_name: String,
    description: String,
    system_prompt: String,
) -> Result<(), String> {
    state
        .alpha
        .add_custom_pup(key, display_name, description, system_prompt)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_custom_pup(state: State<'_, AppState>, key: String) -> Result<(), String> {
    state
        .alpha
        .remove_custom_pup(&key)
        .await
        .map_err(|e| e.to_string())
}
