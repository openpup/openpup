use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use super::types::{
    BridgeConnectionState, BridgeStatusEvent, DiscordConfig, InboundMessage, OutboundMessage,
    Platform,
};

const DISCORD_API: &str = "https://discord.com/api/v10";
const DISCORD_GATEWAY_INTENTS: u64 = 1 | 512 | 4096 | 32768;
const DISCORD_THREAD_PUBLIC: i64 = 11;

#[derive(Clone)]
pub struct DiscordBridge {
    config: DiscordConfig,
    client: Client,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
}

impl DiscordBridge {
    pub fn new(
        config: DiscordConfig,
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
                    Ok(proxy) => builder = builder.proxy(proxy),
                    Err(error) => warn!("[discord] invalid proxy `{proxy_url}`: {error}"),
                }
            }
            builder.build().unwrap_or_else(|error| {
                warn!("[discord] failed to build http client: {error}");
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

    pub async fn start_gateway(self) -> Result<()> {
        loop {
            if let Err(error) = self.run_gateway_once().await {
                self.emit_status(BridgeConnectionState::Error, false, Some(error.to_string()))
                    .await;
                warn!("[discord] gateway stopped: {error}");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }

    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let url = format!("{DISCORD_API}/channels/{}/messages", msg.chat_id);
        let mut body = json!({
            "content": truncate_discord_message(&msg.text),
            "allowed_mentions": { "parse": [] },
        });
        if let Some(reply_id) = msg.reply_to_id.as_deref().filter(|value| !value.is_empty()) {
            body["message_reference"] = json!({
                "message_id": reply_id,
                "channel_id": msg.chat_id,
                "fail_if_not_exists": false,
            });
        }
        debug!(
            "[discord] send to {}: {} chars",
            msg.chat_id,
            msg.text.len()
        );
        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                discord_authorization(&self.config.bot_token),
            )
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let payload = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(format_discord_http_error(
                "send message",
                status.as_u16(),
                &payload,
            )));
        }
        Ok(())
    }

    pub async fn create_thread(&self, parent_channel_id: &str, name: &str) -> Result<String> {
        let url = format!("{DISCORD_API}/channels/{parent_channel_id}/threads");
        let body = json!({
            "name": truncate_thread_name(name),
            "type": DISCORD_THREAD_PUBLIC,
            "auto_archive_duration": 1440,
        });
        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                discord_authorization(&self.config.bot_token),
            )
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let payload = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(format_discord_http_error(
                "create thread",
                status.as_u16(),
                &payload,
            )));
        }
        let value: Value = serde_json::from_str(&payload)?;
        value["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("discord create thread response missing id"))
    }

    async fn run_gateway_once(&self) -> Result<()> {
        self.emit_status(BridgeConnectionState::Connecting, false, None)
            .await;
        let gateway_url = self.gateway_url().await?;
        let url = discord_gateway_ws_url(&gateway_url)?;
        let (ws, _) = connect_async(&url).await?;
        let (sink, mut stream) = ws.split();
        let sink = Arc::new(Mutex::new(sink));
        let last_sequence = Arc::new(Mutex::new(None::<i64>));

        while let Some(frame) = stream.next().await {
            let frame = frame?;
            if !frame.is_text() {
                continue;
            }
            let payload: Value = serde_json::from_str(frame.to_text()?)?;
            if let Some(sequence) = payload["s"].as_i64() {
                *last_sequence.lock().await = Some(sequence);
            }

            match payload["op"].as_i64().unwrap_or(-1) {
                0 => self.handle_dispatch(&payload).await?,
                10 => {
                    let interval = payload["d"]["heartbeat_interval"]
                        .as_u64()
                        .unwrap_or(45_000);
                    self.start_heartbeat(sink.clone(), last_sequence.clone(), interval);
                    self.identify(sink.clone()).await?;
                    self.emit_status(BridgeConnectionState::Connected, true, None)
                        .await;
                    info!("[discord] gateway connected");
                }
                1 => {
                    self.send_heartbeat(sink.clone(), last_sequence.clone())
                        .await?;
                }
                7 | 9 => {
                    return Err(anyhow!("discord gateway requested reconnect"));
                }
                11 => {}
                op => debug!("[discord] ignored gateway op {op}"),
            }
        }

        Err(anyhow!("discord gateway stream ended"))
    }

    async fn gateway_url(&self) -> Result<String> {
        let response = self
            .client
            .get(format!("{DISCORD_API}/gateway/bot"))
            .header(
                "Authorization",
                discord_authorization(&self.config.bot_token),
            )
            .send()
            .await?;
        let status = response.status();
        let payload = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(format_discord_http_error(
                "fetch gateway",
                status.as_u16(),
                &payload,
            )));
        }
        let value: Value = serde_json::from_str(&payload)?;
        value["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("discord gateway response missing url"))
    }

    async fn identify<S>(&self, sink: Arc<Mutex<S>>) -> Result<()>
    where
        S: SinkExt<Message> + Unpin,
        <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
    {
        let identify = json!({
            "op": 2,
            "d": {
                "token": self.config.bot_token,
                "intents": DISCORD_GATEWAY_INTENTS,
                "properties": {
                    "os": std::env::consts::OS,
                    "browser": "openpup",
                    "device": "openpup"
                }
            }
        });
        sink.lock()
            .await
            .send(Message::Text(identify.to_string()))
            .await?;
        Ok(())
    }

    fn start_heartbeat<S>(
        &self,
        sink: Arc<Mutex<S>>,
        last_sequence: Arc<Mutex<Option<i64>>>,
        interval_ms: u64,
    ) where
        S: SinkExt<Message> + Unpin + Send + 'static,
        <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
    {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            loop {
                interval.tick().await;
                let seq = *last_sequence.lock().await;
                let payload = json!({ "op": 1, "d": seq });
                if sink
                    .lock()
                    .await
                    .send(Message::Text(payload.to_string()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    async fn send_heartbeat<S>(
        &self,
        sink: Arc<Mutex<S>>,
        last_sequence: Arc<Mutex<Option<i64>>>,
    ) -> Result<()>
    where
        S: SinkExt<Message> + Unpin,
        <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
    {
        let seq = *last_sequence.lock().await;
        let payload = json!({ "op": 1, "d": seq });
        sink.lock()
            .await
            .send(Message::Text(payload.to_string()))
            .await?;
        Ok(())
    }

    async fn handle_dispatch(&self, payload: &Value) -> Result<()> {
        if payload["t"].as_str() != Some("MESSAGE_CREATE") {
            return Ok(());
        }
        let message = &payload["d"];
        if message["author"]["bot"].as_bool().unwrap_or(false) {
            return Ok(());
        }
        let text = message["content"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let inbound = InboundMessage {
            platform: Platform::Discord,
            chat_id: message["channel_id"].as_str().unwrap_or("").to_string(),
            user_id: message["author"]["id"].as_str().unwrap_or("").to_string(),
            text,
            message_id: message["id"].as_str().unwrap_or("").to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        };
        let _ = self.inbound_tx.send(inbound).await;
        Ok(())
    }

    async fn emit_status(
        &self,
        status: BridgeConnectionState,
        connected: bool,
        error_msg: Option<String>,
    ) {
        if let Some(status_tx) = &self.status_tx {
            let _ = status_tx
                .send(BridgeStatusEvent {
                    platform: Platform::Discord,
                    status,
                    connected,
                    last_seen: connected.then(|| chrono::Utc::now().timestamp()),
                    error_msg,
                })
                .await;
        }
    }
}

fn truncate_discord_message(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        if result.len() + ch.len_utf8() > 1900 {
            result.push_str("\n…");
            break;
        }
        result.push(ch);
    }
    result
}

fn truncate_thread_name(name: &str) -> String {
    let trimmed = name.trim();
    let source = if trimmed.is_empty() {
        "OpenPup Channel"
    } else {
        trimmed
    };
    source.chars().take(90).collect()
}

fn discord_authorization(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("bot ") {
        trimmed.to_string()
    } else {
        format!("Bot {trimmed}")
    }
}

fn discord_gateway_ws_url(base_url: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(base_url)?;
    if url.path().is_empty() {
        url.set_path("/");
    }
    url.query_pairs_mut()
        .clear()
        .append_pair("v", "10")
        .append_pair("encoding", "json");
    Ok(url.to_string())
}

fn format_discord_http_error(action: &str, status: u16, payload: &str) -> String {
    let message = serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|item| item.as_str())
                .map(str::to_string)
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| payload.trim().to_string());
    let hint = match status {
        400 if action == "create thread" => {
            " Check that the hub channel is a guild text/forum channel and the bot can create public threads."
        }
        400 => " Check the configured channel IDs and request payload.",
        401 => " Check that the Bot Token is valid and has not been reset.",
        403 => " Check bot permissions for the target channel and thread.",
        404 => " Check that the target channel ID still exists.",
        _ => "",
    };
    if message.is_empty() {
        format!("discord {action} HTTP {status}.{hint}")
    } else {
        format!("discord {action} HTTP {status}: {message}.{hint}")
    }
}
