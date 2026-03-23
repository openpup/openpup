use super::client::base_info;
use super::types::{
    MessageItem, SendMessageReq, TextItem, WeixinMessage, MESSAGE_ITEM_TYPE_TEXT,
    MESSAGE_STATE_FINISH, MESSAGE_TYPE_BOT,
};

pub fn build_text_message(
    to_user_id: &str,
    text: &str,
    context_token: Option<&str>,
    client_id: String,
) -> SendMessageReq {
    SendMessageReq {
        msg: Some(WeixinMessage {
            from_user_id: Some(String::new()),
            to_user_id: Some(to_user_id.to_string()),
            client_id: Some(client_id),
            message_type: Some(MESSAGE_TYPE_BOT),
            message_state: Some(MESSAGE_STATE_FINISH),
            item_list: Some(vec![MessageItem {
                item_type: Some(MESSAGE_ITEM_TYPE_TEXT),
                text_item: Some(TextItem {
                    text: Some(normalize_text(text)),
                }),
                ..Default::default()
            }]),
            context_token: context_token.map(ToString::to_string),
            ..Default::default()
        }),
        base_info: Some(base_info()),
    }
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}
