use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use uuid::Uuid;

use crate::agents::alpha::AlphaPup;
use crate::bridge::types::{
    BridgeConfig, BridgeConnectionState, BridgeContext, BridgeStatusEvent, InboundMessage, OutboundMessage,
    OutboundType, Platform,
};

use self::auth::OwnerAuth;
use self::telegram::TelegramBridge;

pub mod auth;
pub mod discord;
pub mod formatter;
pub mod slack;
pub mod telegram;
pub mod types;

pub struct BridgeManager {
    config: BridgeConfig,
    alpha: Arc<AlphaPup>,
}

fn is_stop_request(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "stop" | "abort" | "cancel" | "停止" | "停止吧" | "停下" | "停一下" | "取消" | "/stop"
    )
}

impl BridgeManager {
    pub fn new(config: BridgeConfig, alpha: Arc<AlphaPup>) -> Self {
        Self { config, alpha }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.telegram.is_some() || self.config.discord.is_some() || self.config.slack.is_some()
    }

    pub fn start(self: Arc<Self>) {
        if !self.is_enabled() {
            return;
        }
        info!("[bridge] starting configured platform bridges");

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<InboundMessage>(128);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundMessage>(256);
        let (status_tx, mut status_rx) = mpsc::channel::<BridgeStatusEvent>(32);

        if let Some(tg_cfg) = self.config.telegram.clone() {
            let tx = inbound_tx.clone();
            let status_sender = status_tx.clone();
            let bridge = TelegramBridge::new(tg_cfg.clone(), tx, Some(status_sender.clone()));
            let alpha = self.alpha.clone();

            let tg_cfg_out = tg_cfg.clone();
            let (tg_out_tx, mut tg_out_rx) = mpsc::channel::<OutboundMessage>(64);
            let tg_route_tx = tg_out_tx.clone();
            let alpha_for_status = alpha.clone();
            tauri::async_runtime::spawn(async move {
                let _ = alpha_for_status
                    .memory
                    .update_bridge_connection(
                        Platform::Telegram.as_str(),
                        &BridgeConnectionState::Connecting,
                        false,
                        None,
                        None,
                    )
                    .await;
                while let Some(msg) = outbound_rx.recv().await {
                    if msg.platform == Platform::Telegram {
                        let _ = tg_route_tx.send(msg).await;
                    }
                }
            });

            let alpha_for_sender = alpha.clone();
            tauri::async_runtime::spawn(async move {
                let sender = TelegramBridge::new(tg_cfg_out, mpsc::channel(1).0, None);
                while let Some(msg) = tg_out_rx.recv().await {
                    if let Err(e) = sender.send(&msg).await {
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Telegram.as_str(),
                                &BridgeConnectionState::Error,
                                false,
                                None,
                                Some(&e.to_string()),
                            )
                            .await;
                        warn!("[telegram] send error: {e}");
                    } else {
                        let _ = alpha_for_sender
                            .memory
                            .record_external_outbound(&Uuid::new_v4().to_string(), &msg)
                            .await;
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Telegram.as_str(),
                                &BridgeConnectionState::Connected,
                                true,
                                Some(chrono::Utc::now().timestamp()),
                                None,
                            )
                            .await;
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                let _ = status_sender
                    .send(BridgeStatusEvent {
                        platform: Platform::Telegram,
                        status: BridgeConnectionState::Connecting,
                        connected: false,
                        last_seen: None,
                        error_msg: None,
                    })
                    .await;
                if let Err(e) = bridge.start_polling().await {
                    let _ = status_sender
                        .send(BridgeStatusEvent {
                            platform: Platform::Telegram,
                            status: BridgeConnectionState::Error,
                            connected: false,
                            last_seen: None,
                            error_msg: Some(e.to_string()),
                        })
                        .await;
                    warn!("[telegram] polling stopped: {e}");
                }
            });
        }

        let status_alpha = self.alpha.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = status_rx.recv().await {
                let _ = status_alpha
                    .memory
                    .update_bridge_connection(
                        event.platform.as_str(),
                        &event.status,
                        event.connected,
                        event.last_seen,
                        event.error_msg.as_deref(),
                    )
                    .await;
            }
        });

        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(inbound) = inbound_rx.recv().await {
                let _ = manager
                    .alpha
                    .memory
                    .record_external_inbound(&Uuid::new_v4().to_string(), &inbound)
                    .await;
                let _ = manager
                    .alpha
                    .memory
                    .update_bridge_connection(
                        inbound.platform.as_str(),
                        &BridgeConnectionState::Connected,
                        true,
                        Some(inbound.timestamp),
                        None,
                    )
                    .await;

                let auth = OwnerAuth::new(manager.config.clone());
                if let Err(e) = auth.verify(&inbound) {
                    warn!("[bridge] rejected message: {e}");
                    continue;
                }

                let alpha = manager.alpha.clone();
                let out_tx = outbound_tx.clone();
                let chat_id = inbound.chat_id.clone();
                let msg_id = inbound.message_id.clone();
                let platform = inbound.platform.clone();
                let inbound_text = inbound.text.clone();

                if is_stop_request(&inbound_text) {
                    manager.alpha.abort_flag.store(true, Ordering::Relaxed);
                    let _ = outbound_tx
                        .send(OutboundMessage {
                            platform,
                            chat_id,
                            text: "⏹ 已发送停止信号，当前处理会尽快停止。".to_string(),
                            reply_to_id: Some(msg_id),
                            msg_type: OutboundType::Result,
                        })
                        .await;
                    continue;
                }

                tauri::async_runtime::spawn(async move {
                    let _ = out_tx
                        .send(OutboundMessage {
                            platform: platform.clone(),
                            chat_id: chat_id.clone(),
                            text: "🐾 收到，处理中…".to_string(),
                            reply_to_id: Some(msg_id.clone()),
                            msg_type: OutboundType::Ack,
                        })
                        .await;

                    let (reply_tx, reply_rx) = oneshot::channel::<String>();
                    let _ = reply_tx;
                    let bridge_ctx = BridgeContext {
                        platform: platform.clone(),
                        chat_id: chat_id.clone(),
                        out_tx: out_tx.clone(),
                        reply_rx: tokio::sync::Mutex::new(Some(reply_rx)),
                    };

                    alpha
                        .set_layer_hook(Some(Arc::new({
                            let out_tx = out_tx.clone();
                            let platform = platform.clone();
                            let chat_id = chat_id.clone();
                            move |layer_idx, done_pups| {
                                let out_tx = out_tx.clone();
                                let platform = platform.clone();
                                let chat_id = chat_id.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = out_tx
                                        .send(OutboundMessage {
                                            platform,
                                            chat_id,
                                            text: formatter::format_progress(&done_pups, layer_idx),
                                            reply_to_id: None,
                                            msg_type: OutboundType::Progress,
                                        })
                                        .await;
                                });
                            }
                        })))
                        .await;

                    let progress_hook = Arc::new({
                        let out_tx = out_tx.clone();
                        let platform = platform.clone();
                        let chat_id = chat_id.clone();
                        move |text: String| {
                            let out_tx = out_tx.clone();
                            let platform = platform.clone();
                            let chat_id = chat_id.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text,
                                        reply_to_id: None,
                                        msg_type: OutboundType::Progress,
                                    })
                                    .await;
                            });
                        }
                    });

                    match alpha
                        .process_bridge_message(inbound_text, Some(progress_hook))
                        .await
                    {
                        Ok(result) => {
                            let _ = out_tx
                                .send(OutboundMessage {
                                    platform,
                                    chat_id,
                                    text: formatter::format_result(&result),
                                    reply_to_id: Some(msg_id),
                                    msg_type: OutboundType::Result,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = out_tx
                                .send(OutboundMessage {
                                    platform,
                                    chat_id,
                                    text: format!("❌ Error: {e}"),
                                    reply_to_id: Some(msg_id),
                                    msg_type: OutboundType::Error,
                                })
                                .await;
                        }
                    }

                    alpha.set_layer_hook(None).await;
                    let _ = bridge_ctx;
                });
            }
        });
    }
}
