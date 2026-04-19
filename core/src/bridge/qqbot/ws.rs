use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::bridge::types::{
    BridgeConnectionState, BridgeStatusEvent, InboundMessage, Platform, QQBotConfig,
};

use super::client::QQBotClient;

// Gateway opcodes
const OP_DISPATCH: u8 = 0;
const OP_HEARTBEAT: u8 = 1;
const OP_IDENTIFY: u8 = 2;
const OP_RESUME: u8 = 6;
const OP_RECONNECT: u8 = 7;
const OP_INVALID_SESSION: u8 = 9;
const OP_HELLO: u8 = 10;
const OP_HEARTBEAT_ACK: u8 = 11;

// Intents: GROUP_AND_C2C_EVENT | INTERACTION | PUBLIC_GUILD_MESSAGES | GUILDS
const INTENTS: u64 = (1 << 25) | (1 << 26) | (1 << 30) | (1 << 0);

/// Reconnection backoff delays in milliseconds.
const BACKOFF_MS: &[u64] = &[1000, 2000, 5000, 10000, 30000, 60000];

pub struct QQBotGateway {
    client: QQBotClient,
    #[allow(dead_code)]
    config: QQBotConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    session_id: Option<String>,
    last_seq: Option<u64>,
    reconnect_count: usize,
}

impl QQBotGateway {
    pub fn new(
        client: QQBotClient,
        config: QQBotConfig,
        inbound_tx: mpsc::Sender<InboundMessage>,
        status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    ) -> Self {
        Self {
            client,
            config,
            inbound_tx,
            status_tx,
            session_id: None,
            last_seq: None,
            reconnect_count: 0,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        loop {
            match self.connect_and_listen().await {
                Ok(action) => match action {
                    LoopAction::Reconnect => {
                        let delay = self.backoff_delay();
                        warn!(
                            "[qqbot] reconnecting in {delay}ms (attempt {})",
                            self.reconnect_count + 1
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                        self.reconnect_count += 1;
                    }
                    LoopAction::ReconnectFresh => {
                        self.session_id = None;
                        self.last_seq = None;
                        let delay = self.backoff_delay();
                        warn!("[qqbot] fresh reconnect in {delay}ms");
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                        self.reconnect_count += 1;
                    }
                    LoopAction::RefreshTokenAndReconnect => {
                        warn!("[qqbot] refreshing token before reconnect");
                        if let Err(e) = self.client.refresh_token().await {
                            warn!("[qqbot] token refresh failed: {e}");
                        }
                        self.session_id = None;
                        self.last_seq = None;
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        self.reconnect_count += 1;
                    }
                },
                Err(e) => {
                    self.emit_status(BridgeConnectionState::Error, false, Some(e.to_string()))
                        .await;
                    let delay = self.backoff_delay();
                    warn!("[qqbot] connection error: {e}, retrying in {delay}ms");
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    self.reconnect_count += 1;
                }
            }
        }
    }

    async fn connect_and_listen(&mut self) -> Result<LoopAction> {
        self.client.ensure_token().await?;
        let gateway_url = self.client.get_gateway().await?;
        info!("[qqbot] connecting to gateway: {gateway_url}");

        let (ws_stream, _) = tokio_tungstenite::connect_async(&gateway_url).await?;
        let (mut sink, mut stream) = ws_stream.split();

        // Wait for Hello (op 10)
        let heartbeat_interval = match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                let payload: Value = serde_json::from_str(&text)?;
                if payload["op"].as_u64() != Some(OP_HELLO as u64) {
                    return Err(anyhow!("expected Hello (op 10), got: {payload}"));
                }
                payload["d"]["heartbeat_interval"].as_u64().unwrap_or(45000)
            }
            other => {
                return Err(anyhow!("unexpected first message: {other:?}"));
            }
        };

        debug!("[qqbot] received Hello, heartbeat_interval={heartbeat_interval}ms");

        // Send Identify or Resume
        let token = self.client.get_token().await?;
        if let (Some(session_id), Some(seq)) = (&self.session_id, self.last_seq) {
            debug!("[qqbot] sending Resume (session={session_id}, seq={seq})");
            let resume = json!({
                "op": OP_RESUME,
                "d": {
                    "token": format!("QQBot {token}"),
                    "session_id": session_id,
                    "seq": seq,
                }
            });
            sink.send(Message::Text(resume.to_string())).await?;
        } else {
            debug!("[qqbot] sending Identify");
            let identify = json!({
                "op": OP_IDENTIFY,
                "d": {
                    "token": format!("QQBot {token}"),
                    "intents": INTENTS,
                    "shard": [0, 1],
                    "properties": {
                        "$os": std::env::consts::OS,
                        "$browser": "openpup",
                        "$device": "openpup",
                    }
                }
            });
            sink.send(Message::Text(identify.to_string())).await?;
        }

        self.emit_status(BridgeConnectionState::Connected, true, None)
            .await;
        self.reconnect_count = 0;

        // Heartbeat + message loop
        let mut heartbeat_interval_timer =
            tokio::time::interval(tokio::time::Duration::from_millis(heartbeat_interval));
        // Skip the first immediate tick
        heartbeat_interval_timer.tick().await;

        loop {
            tokio::select! {
                _ = heartbeat_interval_timer.tick() => {
                    let hb = json!({
                        "op": OP_HEARTBEAT,
                        "d": self.last_seq,
                    });
                    if let Err(e) = sink.send(Message::Text(hb.to_string())).await {
                        warn!("[qqbot] heartbeat send error: {e}");
                        return Ok(LoopAction::Reconnect);
                    }
                }
                msg = stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            match self.handle_message(&text).await {
                                Ok(Some(action)) => return Ok(action),
                                Ok(None) => {} // continue
                                Err(e) => {
                                    warn!("[qqbot] message handling error: {e}");
                                }
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            let code = frame.as_ref().map(|f| f.code);
                            warn!("[qqbot] connection closed: {code:?}");
                            return Ok(self.handle_close_code(code.map(|c| c.into())));
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = sink.send(Message::Pong(data)).await;
                        }
                        Some(Err(e)) => {
                            warn!("[qqbot] websocket error: {e}");
                            return Ok(LoopAction::Reconnect);
                        }
                        None => {
                            warn!("[qqbot] websocket stream ended");
                            return Ok(LoopAction::Reconnect);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn handle_message(&mut self, text: &str) -> Result<Option<LoopAction>> {
        let payload: Value = serde_json::from_str(text)?;
        let op = payload["op"].as_u64().unwrap_or(99) as u8;

        match op {
            OP_DISPATCH => {
                // Update sequence number
                if let Some(s) = payload["s"].as_u64() {
                    self.last_seq = Some(s);
                }
                let event_type = payload["t"].as_str().unwrap_or("");
                let data = &payload["d"];

                // Capture session_id from READY event
                if event_type == "READY" {
                    if let Some(sid) = data["session_id"].as_str() {
                        self.session_id = Some(sid.to_string());
                        info!("[qqbot] session established: {sid}");
                    }
                    return Ok(None);
                }

                if event_type == "RESUMED" {
                    info!("[qqbot] session resumed successfully");
                    return Ok(None);
                }

                self.handle_dispatch(event_type, data).await;
                Ok(None)
            }
            OP_HEARTBEAT_ACK => {
                debug!("[qqbot] heartbeat ack");
                Ok(None)
            }
            OP_HEARTBEAT => {
                // Server requested heartbeat — respond immediately
                // This is handled in the main loop via the interval,
                // but we won't error here.
                Ok(None)
            }
            OP_RECONNECT => {
                info!("[qqbot] server requested reconnect");
                Ok(Some(LoopAction::Reconnect))
            }
            OP_INVALID_SESSION => {
                let resumable = payload["d"].as_bool().unwrap_or(false);
                if resumable {
                    info!("[qqbot] invalid session (resumable), reconnecting");
                    Ok(Some(LoopAction::Reconnect))
                } else {
                    info!("[qqbot] invalid session (not resumable), fresh connect");
                    Ok(Some(LoopAction::ReconnectFresh))
                }
            }
            _ => {
                debug!("[qqbot] unhandled op {op}: {text}");
                Ok(None)
            }
        }
    }

    async fn handle_dispatch(&self, event_type: &str, data: &Value) {
        match event_type {
            // C2C private message
            "C2C_MESSAGE_CREATE" => {
                let text = data["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return;
                }
                let user_openid = data["author"]["user_openid"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let msg_id = data["id"].as_str().unwrap_or("").to_string();
                let timestamp = self.parse_timestamp(data);

                let inbound = InboundMessage {
                    platform: Platform::QQBot,
                    chat_id: format!("c2c:{user_openid}"),
                    user_id: user_openid,
                    text,
                    message_id: msg_id,
                    timestamp,
                };
                let _ = self.inbound_tx.send(inbound).await;
            }
            // Group @bot message
            "GROUP_AT_MESSAGE_CREATE" | "GROUP_MESSAGE_CREATE" => {
                let text = data["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return;
                }
                let group_openid = data["group_openid"].as_str().unwrap_or("").to_string();
                let user_openid = data["author"]["member_openid"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let msg_id = data["id"].as_str().unwrap_or("").to_string();
                let timestamp = self.parse_timestamp(data);

                let inbound = InboundMessage {
                    platform: Platform::QQBot,
                    chat_id: format!("group:{group_openid}"),
                    user_id: user_openid,
                    text,
                    message_id: msg_id,
                    timestamp,
                };
                let _ = self.inbound_tx.send(inbound).await;
            }
            // Guild channel @bot message
            "AT_MESSAGE_CREATE" => {
                let text = data["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return;
                }
                let channel_id = data["channel_id"].as_str().unwrap_or("").to_string();
                let user_id = data["author"]["id"].as_str().unwrap_or("").to_string();
                let msg_id = data["id"].as_str().unwrap_or("").to_string();
                let timestamp = self.parse_timestamp(data);

                let inbound = InboundMessage {
                    platform: Platform::QQBot,
                    chat_id: format!("channel:{channel_id}"),
                    user_id,
                    text,
                    message_id: msg_id,
                    timestamp,
                };
                let _ = self.inbound_tx.send(inbound).await;
            }
            // Guild direct message
            "DIRECT_MESSAGE_CREATE" => {
                let text = data["content"].as_str().unwrap_or("").trim().to_string();
                if text.is_empty() {
                    return;
                }
                let user_id = data["author"]["id"].as_str().unwrap_or("").to_string();
                let msg_id = data["id"].as_str().unwrap_or("").to_string();
                let timestamp = self.parse_timestamp(data);

                let inbound = InboundMessage {
                    platform: Platform::QQBot,
                    chat_id: format!("c2c:{user_id}"),
                    user_id,
                    text,
                    message_id: msg_id,
                    timestamp,
                };
                let _ = self.inbound_tx.send(inbound).await;
            }
            _ => {
                debug!("[qqbot] ignoring event: {event_type}");
            }
        }
    }

    fn parse_timestamp(&self, data: &Value) -> i64 {
        // QQ Bot timestamps are ISO 8601 strings
        if let Some(ts_str) = data["timestamp"].as_str() {
            chrono::DateTime::parse_from_rfc3339(ts_str)
                .map(|dt| dt.timestamp())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp())
        } else {
            chrono::Utc::now().timestamp()
        }
    }

    fn handle_close_code(&self, code: Option<u16>) -> LoopAction {
        match code {
            Some(4004) => LoopAction::RefreshTokenAndReconnect,
            Some(4008) => {
                warn!("[qqbot] rate limited (4008), waiting 60s");
                LoopAction::Reconnect
            }
            Some(c) if matches!(c, 4006 | 4007 | 4009 | 4900..=4913) => LoopAction::ReconnectFresh,
            _ => LoopAction::Reconnect,
        }
    }

    fn backoff_delay(&self) -> u64 {
        let idx = self.reconnect_count.min(BACKOFF_MS.len() - 1);
        BACKOFF_MS[idx]
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

enum LoopAction {
    Reconnect,
    ReconnectFresh,
    RefreshTokenAndReconnect,
}
