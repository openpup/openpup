use crate::bridge::types::{InboundMessage, Platform};

use super::types::{WeixinMessage, MESSAGE_ITEM_TYPE_TEXT, MESSAGE_TYPE_USER};

#[derive(Debug, Clone)]
pub struct ParsedInbound {
    pub inbound: InboundMessage,
    pub context_token: Option<String>,
}

pub fn parse_inbound_message(message: &WeixinMessage) -> Option<ParsedInbound> {
    if message.message_type.unwrap_or_default() != MESSAGE_TYPE_USER {
        return None;
    }

    let user_id = message.from_user_id.clone()?;
    let text = extract_text(message);
    if text.is_empty() {
        return None;
    }

    let timestamp = message
        .create_time_ms
        .map(normalize_timestamp_ms)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    Some(ParsedInbound {
        context_token: message.context_token.clone(),
        inbound: InboundMessage {
            platform: Platform::Weixin,
            chat_id: user_id.clone(),
            user_id,
            text,
            message_id: message_id_string(message),
            timestamp,
        },
    })
}

pub fn extract_text(message: &WeixinMessage) -> String {
    message
        .item_list
        .as_ref()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.item_type.unwrap_or_default() == MESSAGE_ITEM_TYPE_TEXT)
                .filter_map(|item| item.text_item.as_ref())
                .filter_map(|item| item.text.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn message_id_string(message: &WeixinMessage) -> String {
    if let Some(id) = message.message_id {
        return id.to_string();
    }
    if let Some(id) = message.client_id.as_deref() {
        return id.to_string();
    }
    chrono::Utc::now().timestamp_millis().to_string()
}

fn normalize_timestamp_ms(value: i64) -> i64 {
    if value > 1_000_000_000_000 {
        value / 1000
    } else {
        value
    }
}
