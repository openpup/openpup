use serde::{Deserialize, Serialize};
use tauri::State;

use crate::mcp::orchestrator::{McpServerEntry, McpToolInfo};

use super::AppState;

#[derive(Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerInfo>, String> {
    let servers = state.app.list_mcp_servers().await;
    Ok(servers
        .into_iter()
        .map(|e| McpServerInfo {
            name: e.name,
            base_url: e.base_url,
            token: e.token,
            description: e.description,
            enabled: e.enabled,
        })
        .collect())
}

#[derive(Deserialize)]
pub struct AddMcpServerInput {
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    entry: AddMcpServerInput,
) -> Result<(), String> {
    state
        .app
        .add_mcp_server(McpServerEntry {
            name: entry.name,
            base_url: entry.base_url,
            token: entry.token,
            description: entry.description,
            enabled: true,
        })
        .await
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct UpdateMcpServerInput {
    pub original_name: String,
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn update_mcp_server(
    state: State<'_, AppState>,
    entry: UpdateMcpServerInput,
) -> Result<(), String> {
    state
        .app
        .update_mcp_server(
            &entry.original_name,
            McpServerEntry {
                name: entry.name,
                base_url: entry.base_url,
                token: entry.token,
                description: entry.description,
                enabled: entry.enabled,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .app
        .remove_mcp_server(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_mcp_server(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .app
        .toggle_mcp_server(&name, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    Ok(state.app.list_mcp_tools().await)
}

#[tauri::command]
pub async fn refresh_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    Ok(state.app.refresh_mcp_tools().await)
}
