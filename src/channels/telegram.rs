use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{self, ChannelsConfig, TelegramChannelConfig};
use crate::core::kernel;
use crate::tools::net;
use crate::core::gateway::events::{ClientToGateway, GatewayEnvelope, GatewayToClient};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    #[serde(default)]
    message: Option<Message>,
    #[serde(default)]
    callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Deserialize)]
struct Message {
    message_id: i64,
    chat: Chat,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    id: String,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct GetUpdatesResponse {
    ok: bool,
    #[serde(default)]
    result: Vec<Update>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct AnswerCallbackQueryResponse {
    ok: bool,
}

/// 从配置加载 Telegram 通道设置。
fn load_telegram_config() -> Result<TelegramChannelConfig> {
    let cfg = config::load_or_init()?;
    let channels: ChannelsConfig = cfg.channels.unwrap_or_default();
    let tg = channels
        .telegram
        .context("telegram channel is not configured. Run `openpup add-channel telegram` first.")?;
    Ok(tg)
}

fn telegram_token_from_env(tg_cfg: &TelegramChannelConfig) -> Result<String> {
    let var = tg_cfg
        .bot_token_env
        .trim()
        .to_string();
    let name = if var.is_empty() {
        "TELEGRAM_BOT_TOKEN"
    } else {
        var.as_str()
    };
    let token =
        std::env::var(name).with_context(|| format!("missing Telegram bot token in env {}", name))?;
    Ok(token)
}

async fn send_message(
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    text: &str,
) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_web_page_preview": true,
        }))
        .send()
        .await
        .context("telegram sendMessage failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "telegram sendMessage error: status={} body={}",
            status,
            body
        ));
    }
    let body: SendMessageResponse = resp.json().await.unwrap_or(SendMessageResponse { ok: true });
    if !body.ok {
        return Err(anyhow::anyhow!("telegram sendMessage returned ok=false"));
    }
    Ok(())
}

async fn send_message_with_approval_buttons(
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    approval_id: &str,
    summary: &str,
) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let approve = format!("approve:{}:1", approval_id);
    let deny = format!("approve:{}:0", approval_id);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": format!("Needs approval:\n{}\n\napproval_id={}", summary, approval_id),
            "reply_markup": {
                "inline_keyboard": [[
                    { "text": "Approve", "callback_data": approve },
                    { "text": "Deny", "callback_data": deny }
                ]]
            }
        }))
        .send()
        .await
        .context("telegram sendMessage (approval buttons) failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "telegram sendMessage (approval) error: status={} body={}",
            status,
            body
        ));
    }
    Ok(())
}

async fn answer_callback_query(
    client: &reqwest::Client,
    token: &str,
    callback_query_id: &str,
    text: &str,
) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{}/answerCallbackQuery", token);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "callback_query_id": callback_query_id,
            "text": text,
            "show_alert": false
        }))
        .send()
        .await
        .context("telegram answerCallbackQuery failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "telegram answerCallbackQuery error: status={} body={}",
            status,
            body
        ));
    }
    let body: AnswerCallbackQueryResponse =
        resp.json().await.unwrap_or(AnswerCallbackQueryResponse { ok: true });
    if !body.ok {
        return Err(anyhow::anyhow!("telegram answerCallbackQuery returned ok=false"));
    }
    Ok(())
}

fn parse_run_command(text: &str) -> Option<String> {
    let t = text.trim();
    for prefix in ["/run ", "/orchestrate ", "/o "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            let goal = rest.trim();
            if !goal.is_empty() {
                return Some(goal.to_string());
            }
        }
    }
    None
}

fn parse_approval_callback(data: &str) -> Option<(String, bool)> {
    // "approve:<approval_id>:1|0"
    let parts: Vec<&str> = data.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    if parts[0] != "approve" {
        return None;
    }
    let approval_id = parts[1].to_string();
    let approve = parts[2] == "1";
    Some((approval_id, approve))
}

fn gateway_ws_url_from_config(cfg: &crate::config::OpenpupConfig) -> String {
    let bind = cfg
        .gateway
        .clone()
        .unwrap_or_default()
        .bind;
    format!("ws://{}/ws", bind.trim())
}

fn gateway_token_from_config(cfg: &crate::config::OpenpupConfig) -> Option<String> {
    let gw = cfg.gateway.clone().unwrap_or_default();
    if !gw.require_auth {
        return None;
    }
    gw.token_env
        .as_deref()
        .and_then(|k| std::env::var(k).ok())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("OPENPUP_GATEWAY_TOKEN").ok())
        .filter(|s| !s.trim().is_empty())
}

#[derive(Clone)]
struct ApprovalCtx {
    ws_tx: mpsc::UnboundedSender<WsMessage>,
    chat_id: i64,
}

/// 简单长轮询 bot：从 Telegram 拉取消息，将白名单 chat 的文本转发为 AgentRequest。
pub async fn run_bot_loop() -> Result<()> {
    let tg_cfg = load_telegram_config()?;
    let token = telegram_token_from_env(&tg_cfg)?;
    let client = net::async_client()?;
    let approvals: Arc<Mutex<HashMap<String, ApprovalCtx>>> = Arc::new(Mutex::new(HashMap::new()));

    let allowed: Vec<String> = tg_cfg.allowed_chat_ids;
    if allowed.is_empty() {
        eprintln!(
            "openpup telegram-bot: WARNING: allowed_chat_ids is empty; no chats will be accepted."
        );
    }

    let mut offset: i64 = 0;

    loop {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates",
            token
        );
        let resp = client
            .get(&url)
            .query(&[("timeout", "25"), ("offset", &offset.to_string())])
            .send()
            .await
            .context("telegram getUpdates failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            eprintln!(
                "openpup telegram-bot: getUpdates error: status={} body={}",
                status, body
            );
            // 简单退避
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        let body: GetUpdatesResponse = resp.json().await.context("parse telegram getUpdates")?;
        if !body.ok {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            continue;
        }

        for update in body.result {
            offset = update.update_id + 1;
            if let Some(cb) = update.callback_query {
                let data = cb.data.unwrap_or_default();
                let Some((approval_id, approve)) = parse_approval_callback(&data) else {
                    // ignore unknown callbacks
                    continue;
                };
                let ctx = {
                    let map = approvals.lock().unwrap();
                    map.get(&approval_id).cloned()
                };
                if let Some(ctx) = ctx {
                    let _ = ctx.ws_tx.send(WsMessage::Text(
                        serde_json::to_string(&GatewayEnvelope::v1(ClientToGateway::ApprovalResponse {
                            approval_id: approval_id.clone(),
                            approve,
                        }))
                        .unwrap(),
                    ));
                    let _ = answer_callback_query(&client, &token, &cb.id, if approve { "Approved" } else { "Denied" }).await;
                    let _ = send_message(&client, &token, ctx.chat_id, "openpup: approval response sent.").await;
                } else {
                    let _ = answer_callback_query(&client, &token, &cb.id, "Approval expired").await;
                }
                continue;
            }
            if let Some(msg) = update.message {
                let chat_id_str = msg.chat.id.to_string();
                if !allowed.is_empty() && !allowed.contains(&chat_id_str) {
                    continue;
                }
                let text = match msg.text {
                    Some(t) if !t.trim().is_empty() => t,
                    _ => continue,
                };

                let session_id = format!("telegram:{}", chat_id_str);
                // Remote entry (recommended): /run <goal> triggers orchestration and replies with summary.
                if let Some(goal) = parse_run_command(&text) {
                    let client2 = client.clone();
                    let token2 = token.clone();
                    let chat_id = msg.chat.id;
                    let approvals2 = Arc::clone(&approvals);
                    tokio::spawn(async move {
                        let _ = send_message(&client2, &token2, chat_id, "openpup: orchestration started (via gateway).").await;
                        let cfg = match config::load_or_init() {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = send_message(&client2, &token2, chat_id, &format!("openpup: failed to load config: {e:#}")).await;
                                return;
                            }
                        };
                        let ws_url = gateway_ws_url_from_config(&cfg);
                        let gw_token = gateway_token_from_config(&cfg);

                        let (ws_stream, _resp) = match tokio_tungstenite::connect_async(&ws_url).await {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = send_message(&client2, &token2, chat_id, &format!("openpup: failed to connect gateway WS: {e}")).await;
                                return;
                            }
                        };

                        let (mut ws_write, mut ws_read) = ws_stream.split();
                        let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();

                        // writer task
                        let writer = tokio::spawn(async move {
                            while let Some(m) = rx.recv().await {
                                let _ = ws_write.send(m).await;
                            }
                        });

                        // auth + subscribe + orchestrate
                        if let Some(t) = gw_token {
                            let _ = tx.send(WsMessage::Text(
                                serde_json::to_string(&GatewayEnvelope::v1(ClientToGateway::Auth { token: t })).unwrap(),
                            ));
                        }
                        let _ = tx.send(WsMessage::Text(
                            serde_json::to_string(&GatewayEnvelope::v1(ClientToGateway::Subscribe {
                                topics: vec![format!("session/{}", session_id.clone())],
                            }))
                            .unwrap(),
                        ));
                        let _ = tx.send(WsMessage::Text(
                            serde_json::to_string(&GatewayEnvelope::v1(ClientToGateway::Orchestrate {
                                session_id: session_id.clone(),
                                goal: goal.clone(),
                                agents: vec![],
                            }))
                            .unwrap(),
                        ));

                        // read loop: handle NeedsApproval、每步输出与最终总结
                        while let Some(Ok(msg)) = ws_read.next().await {
                            let WsMessage::Text(t) = msg else { continue };
                            let Ok(env) = serde_json::from_str::<GatewayEnvelope<GatewayToClient>>(&t) else { continue };
                            if env.v != 1 { continue; }
                            match env.data {
                                GatewayToClient::NeedsApproval { approval_id, summary, .. } => {
                                    {
                                        let mut map = approvals2.lock().unwrap();
                                        map.insert(approval_id.clone(), ApprovalCtx { ws_tx: tx.clone(), chat_id });
                                    }
                                    let _ = send_message_with_approval_buttons(&client2, &token2, chat_id, &approval_id, &summary).await;
                                }
                                GatewayToClient::OrchestrationStepFinished { agent, output, .. } => {
                                    // 实时转发每个子 Agent 的输出；非字符串则按 JSON 打印。
                                    let body = match output {
                                        serde_json::Value::String(s) => s,
                                        v => serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
                                    };
                                    let text = format!("Agent `{}` replied:\n{}", agent, body);
                                    let _ = send_message(&client2, &token2, chat_id, &text).await;
                                }
                                GatewayToClient::OrchestrationFinished { summary, .. } => {
                                    let _ = send_message(&client2, &token2, chat_id, &summary).await;
                                    break;
                                }
                                GatewayToClient::Error { message } => {
                                    let _ = send_message(&client2, &token2, chat_id, &format!("openpup gateway error: {}", message)).await;
                                }
                                _ => {}
                            }
                        }

                        drop(tx);
                        let _ = writer.await;
                    });
                    continue;
                }

                // Fallback：普通聊天走一次 Kernel::run_turn（纯 async），并把回复发回 Telegram。
                let client2 = client.clone();
                let token2 = token.clone();
                tokio::spawn(async move {
                    let cfg = match config::load_or_init() {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = send_message(&client2, &token2, msg.chat.id, &format!("openpup: failed to load config: {e:#}")).await;
                            return;
                        }
                    };
                    let k = kernel::DefaultKernel::from_config(cfg.clone());
                    let req = kernel::AgentRequest {
                        session_id: session_id.clone(),
                        input: text.clone(),
                        semantic_kind: Some("loop_log".to_string()),
                    };
                    let res = k.run_turn(req).await;
                    match res {
                        Ok(turn) => {
                            let _ = send_message(&client2, &token2, msg.chat.id, &turn.reply_text).await;
                        }
                        Err(e) => {
                            let _ = send_message(&client2, &token2, msg.chat.id, &format!("openpup: chat error: {e:#}")).await;
                        }
                    }
                });
            }
        }
    }
}
