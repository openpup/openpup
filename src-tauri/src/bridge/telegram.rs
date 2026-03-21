use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::types::{InboundMessage, OutboundMessage, Platform, TelegramConfig};

const TELEGRAM_API: &str = "https://api.telegram.org/bot";

pub struct TelegramBridge {
    config:     TelegramConfig,
    client:     Client,
    inbound_tx: mpsc::Sender<InboundMessage>,
}

impl TelegramBridge {
    pub fn new(config: TelegramConfig, inbound_tx: mpsc::Sender<InboundMessage>) -> Self {
        Self { config, client: Client::new(), inbound_tx }
    }

    pub async fn start_polling(self) -> Result<()> {
        let mut offset: i64 = 0;
        loop {
            match self.get_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        offset = update["update_id"].as_i64().unwrap_or(0) + 1;
                        if let Some(message) = update.get("message") {
                            let text = message["text"].as_str().unwrap_or("").to_string();
                            if text.is_empty() { continue; }
                            let inbound = InboundMessage {
                                platform:   Platform::Telegram,
                                chat_id:    message["chat"]["id"].to_string(),
                                user_id:    message["from"]["id"].to_string(),
                                text,
                                message_id: message["message_id"].to_string(),
                                timestamp:  chrono::Utc::now().timestamp(),
                            };
                            let _ = self.inbound_tx.send(inbound).await;
                        }
                    }
                }
                Err(e) => warn!("[telegram] polling error: {e}"),
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let url = format!("{}{}/sendMessage", TELEGRAM_API, self.config.bot_token);
        let mut body = json!({
            "chat_id":    msg.chat_id,
            "text":       msg.text,
            "parse_mode": "Markdown",
        });
        if let Some(ref reply_id) = msg.reply_to_id {
            body["reply_to_message_id"] = json!(reply_id.parse::<i64>().unwrap_or(0));
        }
        debug!("[telegram] send to {}: {} chars", msg.chat_id, msg.text.len());
        self.client.post(&url).json(&body).send().await?;
        Ok(())
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<Value>> {
        let url = format!("{}{}/getUpdates", TELEGRAM_API, self.config.bot_token);
        let resp = self.client
            .get(&url)
            .query(&[("offset", &offset.to_string()), ("timeout", &"30".to_string())])
            .timeout(std::time::Duration::from_secs(35))
            .send().await?
            .json::<Value>().await?;
        Ok(resp["result"].as_array().cloned().unwrap_or_default())
    }
}
