use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::types::{
    BridgeConnectionState, BridgeStatusEvent, InboundMessage, OutboundMessage, Platform,
    TelegramConfig,
};

const TELEGRAM_API: &str = "https://api.telegram.org/bot";

#[derive(Clone)]
pub struct TelegramBridge {
    config: TelegramConfig,
    client: Client,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
}

impl TelegramBridge {
    pub fn new(
        config: TelegramConfig,
        inbound_tx: mpsc::Sender<InboundMessage>,
        status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    ) -> Self {
        let client = {
            let mut builder = Client::builder();
            if let Some(proxy_url) = config
                .proxy_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                match reqwest::Proxy::all(proxy_url) {
                    Ok(proxy) => {
                        builder = builder.proxy(proxy);
                    }
                    Err(error) => {
                        warn!("[telegram] invalid proxy `{proxy_url}`: {error}");
                    }
                }
            }
            builder.build().unwrap_or_else(|error| {
                warn!("[telegram] failed to build http client: {error}");
                Client::new()
            })
        };
        Self {
            config,
            client,
            inbound_tx,
            status_tx,
        }
    }

    pub async fn start_polling(self) -> Result<()> {
        let mut offset: i64 = 0;
        loop {
            match self.get_updates(offset).await {
                Ok(updates) => {
                    if let Some(status_tx) = &self.status_tx {
                        let _ = status_tx
                            .send(BridgeStatusEvent {
                                platform: Platform::Telegram,
                                status: BridgeConnectionState::Connected,
                                connected: true,
                                last_seen: Some(chrono::Utc::now().timestamp()),
                                error_msg: None,
                            })
                            .await;
                    }
                    for update in updates {
                        offset = update["update_id"].as_i64().unwrap_or(0) + 1;
                        if let Some(message) = update.get("message") {
                            let text = message["text"].as_str().unwrap_or("").to_string();
                            if text.is_empty() {
                                continue;
                            }
                            let inbound = InboundMessage {
                                platform: Platform::Telegram,
                                chat_id: message["chat"]["id"].to_string(),
                                user_id: message["from"]["id"].to_string(),
                                text,
                                message_id: message["message_id"].to_string(),
                                timestamp: chrono::Utc::now().timestamp(),
                            };
                            let _ = self.inbound_tx.send(inbound).await;
                        }
                    }
                }
                Err(e) => {
                    let is_timeout = e
                        .downcast_ref::<reqwest::Error>()
                        .map(|error| error.is_timeout())
                        .unwrap_or(false);
                    if !is_timeout {
                        if let Some(status_tx) = &self.status_tx {
                            let _ = status_tx
                                .send(BridgeStatusEvent {
                                    platform: Platform::Telegram,
                                    status: BridgeConnectionState::Error,
                                    connected: false,
                                    last_seen: None,
                                    error_msg: Some(e.to_string()),
                                })
                                .await;
                        }
                        warn!("[telegram] polling error: {e}");
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let url = format!("{}{}/sendMessage", TELEGRAM_API, self.config.bot_token);
        let mut body = json!({
            "chat_id":    msg.chat_id,
            "text":       msg.text,
        });
        if let Some(ref reply_id) = msg.reply_to_id {
            body["reply_to_message_id"] = json!(reply_id.parse::<i64>().unwrap_or(0));
        }
        debug!(
            "[telegram] send to {}: {} chars",
            msg.chat_id,
            msg.text.len()
        );
        let response = self.client.post(&url).json(&body).send().await?;
        let status = response.status();
        let payload = response
            .json::<Value>()
            .await
            .unwrap_or_else(|_| json!({ "ok": false }));
        if !status.is_success() {
            return Err(anyhow!("telegram send HTTP {status}: {payload}"));
        }
        if !payload["ok"].as_bool().unwrap_or(false) {
            return Err(anyhow!("telegram send rejected: {payload}"));
        }
        Ok(())
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<Value>> {
        let url = format!("{}{}/getUpdates", TELEGRAM_API, self.config.bot_token);
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("offset", &offset.to_string()),
                ("timeout", &"30".to_string()),
            ])
            .timeout(std::time::Duration::from_secs(45))
            .send()
            .await?
            .json::<Value>()
            .await?;
        Ok(resp["result"].as_array().cloned().unwrap_or_default())
    }
}
