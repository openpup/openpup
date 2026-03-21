use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform { Telegram, Discord, Slack }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub platform:   Platform,
    pub chat_id:    String,
    pub user_id:    String,
    pub text:       String,
    pub message_id: String,
    pub timestamp:  i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub platform:    Platform,
    pub chat_id:     String,
    pub text:        String,
    pub reply_to_id: Option<String>,
    pub msg_type:    OutboundType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutboundType { Ack, Progress, Result, PermRequest, Error }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord:  Option<DiscordConfig>,
    pub slack:    Option<SlackConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token:     String,
    pub owner_user_id: String,
    pub allowed_chats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token:        String,
    pub owner_user_id:    String,
    pub allowed_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token:        String,
    pub app_token:        String,
    pub owner_user_id:    String,
    pub allowed_channels: Vec<String>,
}
