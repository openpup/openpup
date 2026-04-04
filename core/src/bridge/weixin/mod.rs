use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::bridge::types::{
    BridgeConfig, BridgeStatusEvent, InboundMessage, OutboundMessage, WeixinConfig,
};

pub mod client;
pub mod config_cache;
pub mod inbound;
pub mod login;
pub mod outbound;
pub mod poll;
pub mod session_guard;
pub mod store;
pub mod types;

use client::WeixinApiClient;
use config_cache::WeixinConfigManager;
use login::LoginManager;
use outbound::build_text_message;
use poll::start_poll_loop;
use store::{list_accounts, load_account, WeixinAccountData};
pub use types::{StoredWeixinAccount, WeixinQrStartResult, WeixinQrWaitResult};

#[derive(Clone, Default)]
pub struct WeixinService {
    login_manager: LoginManager,
}

impl WeixinService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start_qr_login(
        &self,
        base_url: &str,
        proxy_url: Option<&str>,
        route_tag: Option<&str>,
        account_id: Option<&str>,
        bot_type: Option<&str>,
        force: bool,
    ) -> Result<WeixinQrStartResult> {
        self.login_manager
            .start_login(base_url, proxy_url, route_tag, account_id, bot_type, force)
            .await
    }

    pub async fn wait_qr_login(
        &self,
        base_url: &str,
        proxy_url: Option<&str>,
        route_tag: Option<&str>,
        session_key: &str,
        bot_type: Option<&str>,
        timeout_ms: Option<i64>,
    ) -> Result<WeixinQrWaitResult> {
        self.login_manager
            .wait_login(
                session_key,
                base_url,
                proxy_url,
                route_tag,
                bot_type,
                timeout_ms,
            )
            .await
    }

    pub async fn cancel_qr_login(&self, session_key: &str) {
        self.login_manager.cancel_login(session_key).await;
    }

    pub fn list_accounts(&self) -> Vec<StoredWeixinAccount> {
        list_accounts()
    }

    pub fn activate_account(
        &self,
        current_config: BridgeConfig,
        account_id: &str,
    ) -> Result<BridgeConfig> {
        let existing = current_config.weixin.clone().unwrap_or_default();
        let stored = load_account(account_id)
            .ok_or_else(|| anyhow!("Weixin account `{account_id}` not found"))?;
        let owner_user_id = stored
            .user_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| existing.owner_user_id.clone());
        let mut allowed_users = if existing.allowed_users.is_empty() {
            owner_user_id
                .trim()
                .is_empty()
                .then(Vec::new)
                .unwrap_or_else(|| vec![owner_user_id.clone()])
        } else {
            existing.allowed_users.clone()
        };
        if !owner_user_id.trim().is_empty()
            && !allowed_users.iter().any(|value| value == &owner_user_id)
        {
            allowed_users.push(owner_user_id.clone());
        }
        let weixin = WeixinConfig {
            base_url: stored.base_url.unwrap_or(existing.base_url),
            access_token: stored.token.unwrap_or(existing.access_token),
            owner_user_id,
            allowed_users,
            proxy_url: existing.proxy_url,
            account_id: Some(account_id.to_string()),
            route_tag: existing.route_tag,
        };
        let mut next = current_config;
        next.weixin = Some(weixin);
        Ok(next)
    }

    pub fn apply_login_result(
        &self,
        current_config: BridgeConfig,
        result: &WeixinQrWaitResult,
        proxy_url: Option<String>,
        route_tag: Option<String>,
    ) -> Result<BridgeConfig> {
        let account_id = result
            .account_id
            .as_deref()
            .ok_or_else(|| anyhow!("missing account id from login result"))?;
        let mut next = self.activate_account(current_config, account_id)?;
        if let Some(weixin) = next.weixin.as_mut() {
            if let Some(base_url) = result
                .base_url
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                weixin.base_url = base_url.to_string();
            }
            if let Some(user_id) = result
                .user_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                weixin.owner_user_id = user_id.to_string();
                if !weixin.allowed_users.iter().any(|value| value == user_id) {
                    weixin.allowed_users.push(user_id.to_string());
                }
            }
            if proxy_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
            {
                weixin.proxy_url = proxy_url;
            }
            if route_tag
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
            {
                weixin.route_tag = route_tag;
            }
        }
        Ok(next)
    }
}

#[derive(Clone)]
pub struct WeixinBridge {
    client: WeixinApiClient,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    contexts: Arc<RwLock<HashMap<String, String>>>,
    config_cache: WeixinConfigManager,
}

impl WeixinBridge {
    pub fn new(
        config: WeixinConfig,
        inbound_tx: mpsc::Sender<InboundMessage>,
        status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    ) -> Self {
        let client = WeixinApiClient::from_config(&config);
        Self {
            config_cache: WeixinConfigManager::new(client.clone()),
            client,
            inbound_tx,
            status_tx,
            contexts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_polling(&self) -> Result<()> {
        start_poll_loop(
            self.client.clone(),
            self.inbound_tx.clone(),
            self.status_tx.clone(),
            self.contexts.clone(),
        )
        .await
    }

    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let context_token = self.contexts.read().await.get(&msg.chat_id).cloned();
        let Some(context_token) = context_token else {
            warn!(
                "[weixin] skipping send to `{}`: no context_token yet (peer must send a message first)",
                msg.chat_id
            );
            return Ok(());
        };
        let body = build_text_message(
            &msg.chat_id,
            &msg.text,
            Some(&context_token),
            Uuid::new_v4().to_string(),
        );
        debug!("[weixin] send to {}: {} chars", msg.chat_id, msg.text.len());
        self.client.send_message(&body).await
    }

    pub async fn send_typing_start(&self, user_id: &str) -> Result<()> {
        self.send_typing(user_id, types::TYPING_STATUS_TYPING).await
    }

    pub async fn send_typing_cancel(&self, user_id: &str) -> Result<()> {
        self.send_typing(user_id, types::TYPING_STATUS_CANCEL).await
    }

    async fn send_typing(&self, user_id: &str, status: i32) -> Result<()> {
        let context_token = self.contexts.read().await.get(user_id).cloned();
        let cached = self
            .config_cache
            .get_for_user(user_id, context_token.as_deref())
            .await;
        if cached.typing_ticket.trim().is_empty() {
            return Ok(());
        }
        self.client
            .send_typing(&types::SendTypingReq {
                ilink_user_id: Some(user_id.to_string()),
                typing_ticket: Some(cached.typing_ticket),
                status: Some(status),
                base_info: Some(client::base_info()),
            })
            .await
    }

    pub fn store_login_result(
        &self,
        account_id: &str,
        token: Option<String>,
        base_url: Option<String>,
        user_id: Option<String>,
    ) -> Result<()> {
        store::save_account(
            account_id,
            WeixinAccountData {
                token,
                base_url,
                user_id,
                saved_at: Some(chrono::Utc::now().to_rfc3339()),
            },
        )
    }
}
