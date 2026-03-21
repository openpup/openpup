use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::agents::alpha::AlphaPup;

use self::auth::OwnerAuth;
use self::telegram::TelegramBridge;
use self::types::{BridgeConfig, InboundMessage, OutboundMessage, OutboundType, Platform};

pub mod auth;
pub mod discord;
pub mod formatter;
pub mod slack;
pub mod telegram;
pub mod types;

pub struct BridgeManager {
    config: BridgeConfig,
    alpha:  Arc<AlphaPup>,
}

impl BridgeManager {
    pub fn new(config: BridgeConfig, alpha: Arc<AlphaPup>) -> Self {
        Self { config, alpha }
    }

    /// Returns true if any platform is configured.
    pub fn is_enabled(&self) -> bool {
        self.config.telegram.is_some()
            || self.config.discord.is_some()
            || self.config.slack.is_some()
    }

    /// Start all configured platform bridges. Should be called after AppHandle
    /// is available (inside Tauri setup closure).
    pub fn start(self: Arc<Self>) {
        if !self.is_enabled() {
            return;
        }
        info!("[bridge] starting configured platform bridges");

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<InboundMessage>(128);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundMessage>(256);

        // ── Telegram ─────────────────────────────────────────────────────────
        if let Some(tg_cfg) = self.config.telegram.clone() {
            let tx = inbound_tx.clone();
            let bridge = TelegramBridge::new(tg_cfg.clone(), tx);

            // Outbound sender for Telegram
            let tg_cfg_out = tg_cfg.clone();
            let (tg_out_tx, mut tg_out_rx) = mpsc::channel::<OutboundMessage>(64);

            // Clone outbound_tx to route Telegram messages
            let tg_route_tx = tg_out_tx.clone();
            tokio::spawn(async move {
                while let Some(msg) = outbound_rx.recv().await {
                    if msg.platform == Platform::Telegram {
                        let _ = tg_route_tx.send(msg).await;
                    }
                }
            });

            tokio::spawn(async move {
                let sender = TelegramBridge::new(tg_cfg_out, mpsc::channel(1).0);
                while let Some(msg) = tg_out_rx.recv().await {
                    if let Err(e) = sender.send(&msg).await {
                        warn!("[telegram] send error: {e}");
                    }
                }
            });

            tokio::spawn(async move {
                if let Err(e) = bridge.start_polling().await {
                    warn!("[telegram] polling stopped: {e}");
                }
            });
        }

        // ── Core processing loop ──────────────────────────────────────────────
        let manager = self.clone();
        tokio::spawn(async move {
            while let Some(inbound) = inbound_rx.recv().await {
                let auth = OwnerAuth::new(manager.config.clone());
                if let Err(e) = auth.verify(&inbound) {
                    warn!("[bridge] rejected message: {e}");
                    continue;
                }

                let alpha     = manager.alpha.clone();
                let out_tx    = outbound_tx.clone();
                let chat_id   = inbound.chat_id.clone();
                let msg_id    = inbound.message_id.clone();
                let platform  = inbound.platform.clone();

                tokio::spawn(async move {
                    // Acknowledge immediately
                    let _ = out_tx.send(OutboundMessage {
                        platform:    platform.clone(),
                        chat_id:     chat_id.clone(),
                        text:        "\u{1f43e} Got it, working on it\u{2026}".to_string(),
                        reply_to_id: Some(msg_id.clone()),
                        msg_type:    OutboundType::Ack,
                    }).await;

                    // Execute via Alpha
                    match alpha.process_bridge_message(inbound.text.clone()).await {
                        Ok(result) => {
                            let _ = out_tx.send(OutboundMessage {
                                platform,
                                chat_id,
                                text:        formatter::format_result(&result),
                                reply_to_id: Some(msg_id),
                                msg_type:    OutboundType::Result,
                            }).await;
                        }
                        Err(e) => {
                            let _ = out_tx.send(OutboundMessage {
                                platform,
                                chat_id,
                                text:        format!("\u{274c} Error: {e}"),
                                reply_to_id: Some(msg_id),
                                msg_type:    OutboundType::Error,
                            }).await;
                        }
                    }
                });
            }
        });
    }
}
