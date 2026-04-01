use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::mcp::orchestrator::{MCPOrchestrator, McpServerEntry, McpToolInfo};

use super::AppState;

fn mcp<'a>(state: &'a State<'_, AppState>) -> &'a Arc<MCPOrchestrator> {
    &state.alpha.mcp_orchestrator
}

#[derive(Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub base_url: String,
    pub description: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerInfo>, String> {
    let servers = mcp(&state).list_servers().await;
    Ok(servers
        .into_iter()
        .map(|e| McpServerInfo {
            name: e.name,
            base_url: e.base_url,
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
    mcp(&state)
        .add_server(McpServerEntry {
            name: entry.name,
            base_url: entry.base_url,
            token: entry.token,
            description: entry.description,
            enabled: true,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, name: String) -> Result<(), String> {
    mcp(&state)
        .remove_server(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_mcp_server(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    mcp(&state)
        .toggle_server(&name, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    Ok(state.alpha.mcp_orchestrator.list_all_tools().await)
}

#[tauri::command]
pub async fn refresh_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpToolInfo>, String> {
    state.alpha.mcp_orchestrator.refresh_all_tools().await;
    Ok(state.alpha.mcp_orchestrator.list_all_tools().await)
}
