use tauri::State;

use crate::bridge::types::{BridgeConfig, BridgeConnectionStatus};

use super::AppState;

#[tauri::command]
pub async fn get_bridge_config() -> Result<BridgeConfig, String> {
    Ok(crate::bridge::control::get_bridge_config())
}

#[tauri::command]
pub async fn save_bridge_config(
    state: State<'_, AppState>,
    config: BridgeConfig,
) -> Result<(), String> {
    crate::bridge::control::save_bridge_config(Some(&state.bridge_manager), config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_bridge_status(
    state: State<'_, AppState>,
) -> Result<Vec<BridgeConnectionStatus>, String> {
    crate::bridge::control::get_bridge_status(&state.alpha.memory)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_weixin_qr_login(
    state: State<'_, AppState>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    account_id: Option<String>,
    bot_type: Option<String>,
    force: Option<bool>,
) -> Result<crate::bridge::weixin::WeixinQrStartResult, String> {
    crate::bridge::control::start_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        base_url,
        proxy_url,
        route_tag,
        account_id,
        bot_type,
        force.unwrap_or(false),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn wait_weixin_qr_login(
    state: State<'_, AppState>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    session_key: String,
    bot_type: Option<String>,
    timeout_ms: Option<i64>,
) -> Result<crate::bridge::weixin::WeixinQrWaitResult, String> {
    crate::bridge::control::wait_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        Some(&state.bridge_manager),
        base_url,
        proxy_url,
        route_tag,
        session_key,
        bot_type,
        timeout_ms,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_weixin_qr_login(
    state: State<'_, AppState>,
    session_key: String,
) -> Result<(), String> {
    crate::bridge::control::cancel_weixin_qr_login(
        &state.bridge_manager.weixin_service(),
        &session_key,
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn list_weixin_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<crate::bridge::weixin::StoredWeixinAccount>, String> {
    Ok(crate::bridge::control::list_weixin_accounts(
        &state.bridge_manager.weixin_service(),
    ))
}

#[tauri::command]
pub async fn activate_weixin_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<BridgeConfig, String> {
    crate::bridge::control::activate_weixin_account(
        &state.bridge_manager.weixin_service(),
        Some(&state.bridge_manager),
        &account_id,
    )
    .await
    .map_err(|e| e.to_string())
}
