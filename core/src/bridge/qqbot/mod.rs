mod client;
mod ws;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use super::types::{
    BridgeConnectionState, BridgeStatusEvent, InboundMessage, OutboundMessage, Platform,
    QQBotConfig,
};
use client::QQBotClient;
use ws::QQBotGateway;

#[derive(Clone)]
pub struct QQBotBridge {
    config: QQBotConfig,
    client: QQBotClient,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
}

impl QQBotBridge {
    pub fn new(
        config: QQBotConfig,
        inbound_tx: mpsc::Sender<InboundMessage>,
        status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    ) -> Self {
        let client = QQBotClient::new(
            config.app_id.clone(),
            config.client_secret.clone(),
        );
        Self {
            config,
            client,
            inbound_tx,
            status_tx,
        }
    }

    pub async fn start_polling(self) -> Result<()> {
        info!("[qqbot] starting websocket gateway");

        // Obtain initial access token
        if let Err(e) = self.client.ensure_token().await {
            self.emit_status(BridgeConnectionState::Error, false, Some(e.to_string()))
                .await;
            return Err(e);
        }

        let gateway = QQBotGateway::new(
            self.client.clone(),
            self.config.clone(),
            self.inbound_tx.clone(),
            self.status_tx.clone(),
        );
        gateway.run().await
    }

    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        self.client.ensure_token().await?;
        self.client
            .send_message(&msg.chat_id, &msg.text, msg.reply_to_id.as_deref())
            .await
    }

    async fn emit_status(
        &self,
        state: BridgeConnectionState,
        connected: bool,
        error: Option<String>,
    ) {
        if let Some(tx) = &self.status_tx {
            let _ = tx
                .send(BridgeStatusEvent {
                    platform: Platform::QQBot,
                    status: state,
                    connected,
                    last_seen: if connected {
                        Some(chrono::Utc::now().timestamp())
                    } else {
                        None
                    },
                    error_msg: error,
                })
                .await;
        }
    }
}
