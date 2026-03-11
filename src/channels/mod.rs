//! 通道适配层：将 Telegram/Slack/企业微信/飞书等外部消息源适配为 Runtime 事件。
//!
//! 当前阶段：
//! - Telegram：提供完整的长轮询 bot 通道（收/发消息 + 网关编排）。
//! - Feishu：提供最小可用的「通知通道」，用于向指定 chat 推送文本消息。

use anyhow::Result;

mod telegram;
use self::telegram as telegram_mod;
mod feishu;
use self::feishu as feishu_mod;

/// 运行 Telegram 通道事件循环（长轮询），通常由 `openpup up` 在 Tokio runtime 中以后台任务形式启动。
pub async fn run_telegram_channel() -> Result<()> {
    println!("openpup telegram: starting polling loop (managed by `openpup up`). Use Ctrl+C to stop.");
    telegram_mod::run_bot_loop().await
}

/// 向 Feishu 默认群聊发送一条纯文本通知。
pub async fn notify_feishu_default(text: &str) -> Result<()> {
    feishu_mod::notify_default_chat(text).await
}

/// 向指定 Feishu chat_id 发送一条纯文本消息。
pub async fn send_feishu_text_to_chat(chat_id: &str, text: &str) -> Result<()> {
    feishu_mod::send_text_to_chat(chat_id, text).await
}
