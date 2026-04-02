//! Tauri command handlers, split by domain.
//!
//! Each sub-module contains a focused set of commands. `AppState` lives here
//! so every sub-module can import it via `super::AppState`.

use std::ops::Deref;
use std::sync::Arc;

use openpup_core::app::OpenPupApp;

#[derive(Clone)]
pub struct AppState {
    pub app: Arc<OpenPupApp>,
    pub bridge_manager: Arc<crate::bridge::BridgeManager>,
}

impl Deref for AppState {
    type Target = OpenPupApp;

    fn deref(&self) -> &Self::Target {
        self.app.as_ref()
    }
}

impl AppState {
    pub fn is_mobile_runtime(&self) -> bool {
        self.app.is_mobile_runtime()
    }

    pub async fn save_bridge_config(
        &self,
        config: crate::bridge::types::BridgeConfig,
    ) -> anyhow::Result<()> {
        crate::bridge::control::save_bridge_config(Some(&self.bridge_manager), config).await
    }

    pub async fn start_weixin_qr_login(
        &self,
        base_url: String,
        proxy_url: Option<String>,
        route_tag: Option<String>,
        account_id: Option<String>,
        bot_type: Option<String>,
        force: bool,
    ) -> anyhow::Result<crate::bridge::weixin::WeixinQrStartResult> {
        crate::bridge::control::start_weixin_qr_login(
            &self.bridge_manager.weixin_service(),
            base_url,
            proxy_url,
            route_tag,
            account_id,
            bot_type,
            force,
        )
        .await
    }

    pub async fn wait_weixin_qr_login(
        &self,
        base_url: String,
        proxy_url: Option<String>,
        route_tag: Option<String>,
        session_key: String,
        bot_type: Option<String>,
        timeout_ms: Option<i64>,
    ) -> anyhow::Result<crate::bridge::weixin::WeixinQrWaitResult> {
        crate::bridge::control::wait_weixin_qr_login(
            &self.bridge_manager.weixin_service(),
            Some(&self.bridge_manager),
            base_url,
            proxy_url,
            route_tag,
            session_key,
            bot_type,
            timeout_ms,
        )
        .await
    }

    pub async fn cancel_weixin_qr_login(&self, session_key: &str) {
        crate::bridge::control::cancel_weixin_qr_login(
            &self.bridge_manager.weixin_service(),
            session_key,
        )
        .await;
    }

    pub fn list_weixin_accounts(&self) -> Vec<crate::bridge::weixin::StoredWeixinAccount> {
        crate::bridge::control::list_weixin_accounts(&self.bridge_manager.weixin_service())
    }

    pub async fn activate_weixin_account(
        &self,
        account_id: &str,
    ) -> anyhow::Result<crate::bridge::types::BridgeConfig> {
        crate::bridge::control::activate_weixin_account(
            &self.bridge_manager.weixin_service(),
            Some(&self.bridge_manager),
            account_id,
        )
        .await
    }
}

pub mod bridge;
pub mod channel;
pub mod chat;
pub mod config;
pub mod context;
pub mod feedback;
pub mod knowledge;
pub mod mcp;
pub mod memory;
pub mod onboarding;
pub mod permissions;
pub mod pups;
pub mod skills;
pub mod tasks;
pub mod timeline;
pub mod workspace;

// Re-export all command functions so main.rs can reference them directly.
pub use bridge::*;
pub use channel::*;
pub use chat::*;
pub use config::*;
pub use context::*;
pub use feedback::*;
pub use knowledge::*;
pub use mcp::*;
pub use memory::*;
pub use onboarding::*;
pub use permissions::*;
pub use pups::*;
pub use skills::*;
pub use tasks::*;
pub use timeline::*;
pub use workspace::*;
