use std::collections::HashSet;

use tauri::State;

use super::AppState;

fn extract_mentions(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|token| {
            let mention = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    ',' | '，' | '.' | '。' | ':' | '：' | ';' | '；' | ')' | '）' | '(' | '（'
                )
            });
            let mention = mention
                .strip_prefix('@')
                .or_else(|| mention.strip_prefix('＠'))?;
            let mut key = String::new();
            for ch in mention.chars() {
                if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '/' {
                    key.extend(ch.to_lowercase());
                } else {
                    break;
                }
            }
            if key.is_empty() {
                None
            } else {
                Some(key)
            }
        })
        .collect()
}

fn bridge_target(platform: &str) -> Option<(crate::bridge::types::Platform, String)> {
    let cfg = crate::config::load_with_env().bridge.unwrap_or_default();
    match platform {
        "qqbot" => cfg.qqbot.as_ref().map(|qq| {
            let chat_id = if qq.owner_user_id.starts_with("c2c:") {
                qq.owner_user_id.clone()
            } else {
                format!("c2c:{}", qq.owner_user_id)
            };
            (crate::bridge::types::Platform::QQBot, chat_id)
        }),
        "weixin" => cfg.weixin.as_ref().map(|wx| {
            (
                crate::bridge::types::Platform::Weixin,
                wx.owner_user_id.clone(),
            )
        }),
        "telegram" => cfg.telegram.as_ref().map(|tg| {
            (
                crate::bridge::types::Platform::Telegram,
                tg.owner_user_id.clone(),
            )
        }),
        _ => None,
    }
}

fn bridge_platform_for_member(
    member: &crate::conversation::types::ConversationMemberRecord,
) -> Option<&'static str> {
    if let Some(rest) = member.identity_id.strip_prefix("bridge:") {
        if let Some(platform) = rest.split(':').next() {
            return match platform {
                "qqbot" => Some("qqbot"),
                "weixin" => Some("weixin"),
                "telegram" => Some("telegram"),
                _ => None,
            };
        }
    }

    let route = member.route_label.to_ascii_lowercase();
    if route.contains("qqbot") {
        Some("qqbot")
    } else if route.contains("weixin") || route.contains("wechat") {
        Some("weixin")
    } else if route.contains("telegram") {
        Some("telegram")
    } else {
        None
    }
}

fn is_alpha_member(member: &crate::conversation::types::ConversationMemberRecord) -> bool {
    member.identity_id == "agent_alpha"
}

async fn post_system_message(
    state: &State<'_, AppState>,
    conversation_id: &str,
    content: &str,
) -> Result<(), String> {
    let message = state
        .app
        .memory
        .post_conversation_message(
            conversation_id,
            "system",
            None,
            "系统",
            "system",
            None,
            content,
        )
        .await
        .map_err(|e| e.to_string())?;
    state.app.emit_conversation_message_created(message);
    state.app.emit_conversation_spaces_changed().await;
    Ok(())
}

async fn send_to_mentioned_bridges(
    state: &State<'_, AppState>,
    conversation_id: &str,
    group_title: &str,
    content: &str,
    mentioned_members: &[crate::conversation::types::ConversationMemberRecord],
) {
    let mut seen = HashSet::new();
    let platforms: Vec<&'static str> = mentioned_members
        .iter()
        .filter_map(bridge_platform_for_member)
        .filter(|platform| seen.insert(*platform))
        .collect();
    if platforms.is_empty() {
        return;
    }

    let tx = match state.app.bridge_outbox.lock().await.clone() {
        Some(tx) => tx,
        None => {
            let _ =
                post_system_message(state, conversation_id, "Bridge 未运行，消息未发送。").await;
            return;
        }
    };

    let mut sent = Vec::new();
    let mut missing = Vec::new();
    for platform in platforms {
        let Some((target_platform, chat_id)) = bridge_target(platform) else {
            missing.push(platform);
            continue;
        };
        let text = format!("群「{group_title}」\n我: {content}");
        if tx
            .send(crate::bridge::types::OutboundMessage {
                platform: target_platform,
                chat_id,
                text,
                reply_to_id: None,
                msg_type: crate::bridge::types::OutboundType::Result,
            })
            .await
            .is_ok()
        {
            sent.push(platform);
        }
    }

    if !sent.is_empty() {
        let _ = post_system_message(
            state,
            conversation_id,
            &format!("已发送到 bridge：{}", sent.join(", ")),
        )
        .await;
    }
    if !missing.is_empty() {
        let _ = post_system_message(
            state,
            conversation_id,
            &format!("Bridge 未配置：{}", missing.join(", ")),
        )
        .await;
    }
}

async fn publish_message_to_xmtp(
    state: &State<'_, AppState>,
    space: &crate::conversation::types::ConversationSpaceRecord,
    message: &crate::conversation::types::ConversationMessageRecord,
) {
    if let Err(e) = state
        .xmtp_helper
        .publish_message(&state.app.memory, &state.app.workspace_root, space, message)
        .await
    {
        let _ = post_system_message(state, &space.id, &format!("XMTP 发送失败：{e}")).await;
    }
}

fn is_remote_agent_member(member: &crate::conversation::types::ConversationMemberRecord) -> bool {
    member.role == "agent" && member.identity_id != "agent_alpha"
}

#[tauri::command]
pub async fn list_conversation_spaces(
    state: State<'_, AppState>,
) -> Result<Vec<crate::conversation::types::ConversationSpaceRecord>, String> {
    state
        .app
        .list_conversation_spaces()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_conversation_space(
    state: State<'_, AppState>,
    title: String,
) -> Result<crate::conversation::types::ConversationSpaceRecord, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("群名称不能为空".to_string());
    }
    let space = state
        .app
        .create_conversation_space(title)
        .await
        .map_err(|e| e.to_string())?;
    state.app.emit_conversation_spaces_changed().await;
    state.app.emit_conversation_members_changed(&space.id).await;
    Ok(space)
}

#[tauri::command]
pub async fn find_conversation_space(
    state: State<'_, AppState>,
    target: String,
) -> Result<Option<crate::conversation::types::ConversationSpaceRecord>, String> {
    state
        .app
        .find_conversation_space(&target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_conversation_members(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<crate::conversation::types::ConversationMemberRecord>, String> {
    state
        .app
        .conversation_members(&conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_conversation_member(
    state: State<'_, AppState>,
    conversation_id: String,
    display_name: String,
    mention_key: Option<String>,
    route_label: Option<String>,
    role: Option<String>,
) -> Result<crate::conversation::types::ConversationMemberRecord, String> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err("成员名称不能为空".to_string());
    }
    let member = state
        .app
        .add_conversation_member(
            &conversation_id,
            display_name,
            mention_key.as_deref(),
            route_label.as_deref(),
            role.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    state
        .app
        .emit_conversation_members_changed(&conversation_id)
        .await;
    state.app.emit_conversation_spaces_changed().await;
    Ok(member)
}

#[tauri::command]
pub async fn remove_conversation_member(
    state: State<'_, AppState>,
    conversation_id: String,
    identity_id: String,
) -> Result<(), String> {
    state
        .app
        .remove_conversation_member(&conversation_id, &identity_id)
        .await
        .map_err(|e| e.to_string())?;
    state
        .app
        .emit_conversation_members_changed(&conversation_id)
        .await;
    state.app.emit_conversation_spaces_changed().await;
    Ok(())
}

#[tauri::command]
pub async fn delete_conversation_space(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<(), String> {
    state
        .app
        .delete_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    state.app.emit_conversation_spaces_changed().await;
    Ok(())
}

#[tauri::command]
pub async fn get_conversation_messages(
    state: State<'_, AppState>,
    conversation_id: String,
    limit: Option<i64>,
) -> Result<Vec<crate::conversation::types::ConversationMessageRecord>, String> {
    state
        .app
        .conversation_messages(&conversation_id, limit.unwrap_or(200))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn post_conversation_message(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
) -> Result<crate::conversation::types::ConversationMessageRecord, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("消息不能为空".to_string());
    }
    let message = state
        .app
        .post_conversation_message(&conversation_id, content)
        .await
        .map_err(|e| e.to_string())?;
    state.app.emit_conversation_message_created(message.clone());
    state.app.emit_conversation_spaces_changed().await;

    let space = state
        .app
        .find_conversation_space(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let group_title = space
        .as_ref()
        .map(|space| space.title.as_str())
        .unwrap_or("群聊");
    if let Some(space) = space.as_ref() {
        publish_message_to_xmtp(&state, space, &message).await;
    }

    let mentions = extract_mentions(content);
    let members = state
        .app
        .conversation_members(&conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let mentioned_members: Vec<_> = members
        .into_iter()
        .filter(|member| member.status == "active" && mentions.contains(&member.mention_key))
        .collect();
    let local_alpha_mentioned = mentioned_members.iter().any(is_alpha_member);
    let remote_agent_mentioned = mentioned_members.iter().any(is_remote_agent_member);
    send_to_mentioned_bridges(
        &state,
        &conversation_id,
        group_title,
        content,
        &mentioned_members,
    )
    .await;

    if remote_agent_mentioned && !local_alpha_mentioned {
        return Ok(message);
    }

    if local_alpha_mentioned {
        match state
            .app
            .alpha
            .process_group_message(&conversation_id, group_title, content)
            .await
        {
            Ok(reply) if !reply.trim().is_empty() => {
                let alpha_message = state
                    .app
                    .memory
                    .post_conversation_message(
                        &conversation_id,
                        "agent_alpha",
                        None,
                        "Alpha",
                        "agent",
                        Some("openpup.alpha"),
                        &reply,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(space) = space.as_ref() {
                    publish_message_to_xmtp(&state, space, &alpha_message).await;
                }
                state.app.emit_conversation_message_created(alpha_message);
                state.app.emit_conversation_spaces_changed().await;
            }
            Ok(_) => {}
            Err(e) => {
                let _ =
                    post_system_message(&state, &conversation_id, &format!("Alpha 回复失败：{e}"))
                        .await;
            }
        }
    }

    Ok(message)
}
