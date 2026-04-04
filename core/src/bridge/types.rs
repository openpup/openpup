use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Shared slot for the bridge outbound sender.
/// Created empty, filled by BridgeManager when it starts.
pub type BridgeOutbox = Arc<Mutex<Option<mpsc::Sender<OutboundMessage>>>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Telegram,
    Discord,
    Slack,
    Weixin,
    QQBot,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Telegram => "telegram",
            Platform::Discord => "discord",
            Platform::Slack => "slack",
            Platform::Weixin => "weixin",
            Platform::QQBot => "qqbot",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub platform: Platform,
    pub chat_id: String,
    pub user_id: String,
    pub text: String,
    pub message_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub platform: Platform,
    pub chat_id: String,
    pub text: String,
    pub reply_to_id: Option<String>,
    pub msg_type: OutboundType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutboundType {
    Ack,
    Progress,
    Result,
    PermRequest,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeConnectionState {
    Unconfigured,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConnectionStatus {
    pub platform: String,
    pub status: BridgeConnectionState,
    pub connected: bool,
    pub last_seen: Option<i64>,
    pub error_msg: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeStatusEvent {
    pub platform: Platform,
    pub status: BridgeConnectionState,
    pub connected: bool,
    pub last_seen: Option<i64>,
    pub error_msg: Option<String>,
}

pub struct BridgeContext {
    pub platform: Platform,
    pub chat_id: String,
    pub out_tx: mpsc::Sender<OutboundMessage>,
    pub reply_rx: Mutex<Option<oneshot::Receiver<String>>>,
}

impl BridgeContext {
    pub async fn wait_for_reply(&self) -> Option<String> {
        let rx = self.reply_rx.lock().await.take();
        match rx {
            Some(receiver) => receiver.await.ok(),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub weixin: Option<WeixinConfig>,
    pub qqbot: Option<QQBotConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub owner_user_id: String,
    pub allowed_chats: Vec<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token: String,
    pub owner_user_id: String,
    pub allowed_channels: Vec<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: String,
    pub owner_user_id: String,
    pub allowed_channels: Vec<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinConfig {
    pub base_url: String,
    pub access_token: String,
    pub owner_user_id: String,
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub route_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QQBotConfig {
    pub app_id: String,
    pub client_secret: String,
    pub owner_user_id: String,
    pub allowed_chats: Vec<String>,
}
