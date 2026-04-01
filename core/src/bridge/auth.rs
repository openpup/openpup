use super::types::{BridgeConfig, InboundMessage, Platform};
use anyhow::{bail, Result};

pub struct OwnerAuth {
    config: BridgeConfig,
}

impl OwnerAuth {
    pub fn new(config: BridgeConfig) -> Self {
        Self { config }
    }

    pub fn verify(&self, msg: &InboundMessage) -> Result<()> {
        match msg.platform {
            Platform::Telegram => {
                let cfg = self
                    .config
                    .telegram
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Telegram not configured"))?;
                if msg.user_id != cfg.owner_user_id {
                    bail!("non-owner message rejected");
                }
                if !cfg.allowed_chats.contains(&msg.chat_id) {
                    bail!("chat_id not in allowlist");
                }
                Ok(())
            }
            Platform::Discord => {
                let cfg = self
                    .config
                    .discord
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Discord not configured"))?;
                if msg.user_id != cfg.owner_user_id {
                    bail!("non-owner message rejected");
                }
                if !cfg.allowed_channels.contains(&msg.chat_id) {
                    bail!("channel not in allowlist");
                }
                Ok(())
            }
            Platform::Slack => {
                let cfg = self
                    .config
                    .slack
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Slack not configured"))?;
                if msg.user_id != cfg.owner_user_id {
                    bail!("non-owner message rejected");
                }
                if !cfg.allowed_channels.contains(&msg.chat_id) {
                    bail!("channel not in allowlist");
                }
                Ok(())
            }
            Platform::Weixin => {
                let cfg = self
                    .config
                    .weixin
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Weixin not configured"))?;
                if msg.user_id != cfg.owner_user_id {
                    bail!("non-owner message rejected");
                }
                if !cfg.allowed_users.contains(&msg.chat_id) {
                    bail!("user_id not in allowlist");
                }
                Ok(())
            }
        }
    }
}
