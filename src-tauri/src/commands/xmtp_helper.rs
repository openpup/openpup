use serde::Serialize;
use serde_json::json;
use tauri::State;

use super::AppState;
use crate::{import_xmtp_group, XmtpHelperGroupPayload};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpConversationBinding {
    pub conversation_id: String,
    pub transport_ref: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpIdentity {
    pub inbox_id: String,
    pub env: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpAddMemberResult {
    pub conversation_id: String,
    pub inbox_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpRemoveMemberResult {
    pub conversation_id: String,
    pub inbox_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpLeaveConversationResult {
    pub conversation_id: String,
    pub transport_ref: String,
    pub pending_removal: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmtpSyncGroupsResult {
    pub imported: usize,
}

fn xmtp_init_params(workspace_root: &std::path::Path) -> Result<serde_json::Value, String> {
    let xmtp = crate::config::ensure_xmtp_config().map_err(|e| e.to_string())?;
    Ok(json!({
        "env": "dev",
        "dataDir": workspace_root.join("xmtp"),
        "identityPrivateKey": xmtp.identity_private_key,
        "dbEncryptionKey": xmtp.db_encryption_key,
    }))
}

#[tauri::command]
pub async fn get_xmtp_helper_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    state
        .xmtp_helper
        .request("status", json!({}))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_xmtp_identity(state: State<'_, AppState>) -> Result<XmtpIdentity, String> {
    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    let identity = state
        .xmtp_helper
        .request("identity", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    Ok(XmtpIdentity {
        inbox_id: identity
            .get("inboxId")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        env: identity
            .get("env")
            .and_then(|value| value.as_str())
            .unwrap_or("dev")
            .to_string(),
    })
}

#[tauri::command]
pub async fn enable_xmtp_for_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<XmtpConversationBinding, String> {
    let space = state
        .app
        .find_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "群不存在".to_string())?;

    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;

    let created = state
        .xmtp_helper
        .request(
            "createGroup",
            json!({
                "conversationId": space.id,
                "title": space.title,
            }),
        )
        .await
        .map_err(|e| e.to_string())?;
    let transport_ref = created
        .get("transportRef")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "XMTP helper 未返回 transportRef".to_string())?
        .to_string();
    state
        .app
        .bind_conversation_transport(&conversation_id, "xmtp", "XMTP", &transport_ref)
        .await
        .map_err(|e| e.to_string())?;
    let _ = state.xmtp_helper.request("startStream", json!({})).await;
    state.app.emit_conversation_spaces_changed().await;

    Ok(XmtpConversationBinding {
        conversation_id,
        transport_ref,
        status: "active".to_string(),
    })
}

#[tauri::command]
pub async fn add_xmtp_conversation_member(
    state: State<'_, AppState>,
    conversation_id: String,
    inbox_id: String,
) -> Result<XmtpAddMemberResult, String> {
    let inbox_id = inbox_id.trim().to_string();
    if inbox_id.is_empty() {
        return Err("请输入 XMTP ID".to_string());
    }
    let space = state
        .app
        .find_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "群不存在".to_string())?;
    let transport_ref = space
        .transports
        .iter()
        .find(|transport| transport.kind == "xmtp" && transport.status == "active")
        .and_then(|transport| transport.transport_ref.as_deref())
        .ok_or_else(|| "群尚未启用 XMTP".to_string())?;

    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    state
        .xmtp_helper
        .request(
            "addMembers",
            json!({
                "transportRef": transport_ref,
                "inboxIds": [inbox_id],
            }),
        )
        .await
        .map_err(|e| e.to_string())?;
    state.app.emit_conversation_spaces_changed().await;

    Ok(XmtpAddMemberResult {
        conversation_id,
        inbox_id,
    })
}

#[tauri::command]
pub async fn remove_xmtp_conversation_member(
    state: State<'_, AppState>,
    conversation_id: String,
    inbox_id: String,
) -> Result<XmtpRemoveMemberResult, String> {
    let inbox_id = inbox_id.trim().to_string();
    if inbox_id.is_empty() {
        return Err("缺少 XMTP ID".to_string());
    }
    let space = state
        .app
        .find_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "群不存在".to_string())?;
    let transport_ref = space
        .transports
        .iter()
        .find(|transport| transport.kind == "xmtp" && transport.status == "active")
        .and_then(|transport| transport.transport_ref.as_deref())
        .ok_or_else(|| "群尚未启用 XMTP".to_string())?;

    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    state
        .xmtp_helper
        .request(
            "removeMembers",
            json!({
                "transportRef": transport_ref,
                "inboxIds": [inbox_id],
            }),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(XmtpRemoveMemberResult {
        conversation_id,
        inbox_id,
    })
}

#[tauri::command]
pub async fn leave_xmtp_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<XmtpLeaveConversationResult, String> {
    let space = state
        .app
        .find_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "群不存在".to_string())?;
    let transport_ref = space
        .transports
        .iter()
        .find(|transport| transport.kind == "xmtp" && transport.status == "active")
        .and_then(|transport| transport.transport_ref.as_deref())
        .ok_or_else(|| "群尚未启用 XMTP".to_string())?
        .to_string();

    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    let result = state
        .xmtp_helper
        .request(
            "requestRemoval",
            json!({
                "transportRef": transport_ref,
            }),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(XmtpLeaveConversationResult {
        conversation_id,
        transport_ref,
        pending_removal: result
            .get("pendingRemoval")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    })
}

#[tauri::command]
pub async fn sync_xmtp_groups(
    state: State<'_, AppState>,
) -> Result<XmtpSyncGroupsResult, String> {
    state
        .xmtp_helper
        .request("init", xmtp_init_params(&state.app.workspace_root)?)
        .await
        .map_err(|e| e.to_string())?;
    state
        .xmtp_helper
        .request("startStream", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    let result = state
        .xmtp_helper
        .request("syncGroups", json!({}))
        .await
        .map_err(|e| e.to_string())?;

    let groups = result
        .get("groups")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let app = state.app.clone();
    let mut imported = 0;
    for group in groups {
        if let Ok(payload) = serde_json::from_value::<XmtpHelperGroupPayload>(group) {
            let existed = state
                .app
                .memory
                .find_conversation_by_transport("xmtp", &payload.transport_ref)
                .await
                .map_err(|e| e.to_string())?
                .is_some();
            import_xmtp_group(app.clone(), serde_json::to_value(payload).map_err(|e| e.to_string())?)
                .await
                .map_err(|e| e.to_string())?;
            if !existed {
                imported += 1;
            }
        }
    }
    Ok(XmtpSyncGroupsResult { imported })
}
