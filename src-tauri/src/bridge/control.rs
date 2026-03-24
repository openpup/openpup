use std::sync::Arc;

use anyhow::Result;

use crate::bridge::types::{BridgeConfig, BridgeConnectionStatus};
use crate::bridge::weixin::{StoredWeixinAccount, WeixinQrStartResult, WeixinQrWaitResult, WeixinService};
use crate::bridge::BridgeManager;
use crate::memory::system::MemorySystem;

pub const DEFAULT_WEIXIN_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

pub fn get_bridge_config() -> BridgeConfig {
    crate::config::load().bridge.unwrap_or_default()
}

pub async fn save_bridge_config(
    bridge_manager: Option<&Arc<BridgeManager>>,
    config: BridgeConfig,
) -> Result<()> {
    let mut cfg = crate::config::load();
    cfg.bridge = Some(config.clone());
    crate::config::save(&cfg)?;
    if let Some(manager) = bridge_manager {
        manager.restart(config).await;
    }
    Ok(())
}

pub async fn reload_bridge_config(bridge_manager: &Arc<BridgeManager>) -> Result<BridgeConfig> {
    let config = get_bridge_config();
    bridge_manager.restart(config.clone()).await;
    Ok(config)
}

pub async fn get_bridge_status(memory: &Arc<MemorySystem>) -> Result<Vec<BridgeConnectionStatus>> {
    memory.list_bridge_connections().await
}

pub async fn start_weixin_qr_login(
    weixin_service: &Arc<WeixinService>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    account_id: Option<String>,
    bot_type: Option<String>,
    force: bool,
) -> Result<WeixinQrStartResult> {
    let base_url = normalize_base_url(base_url);
    weixin_service
        .start_qr_login(
            &base_url,
            proxy_url.as_deref(),
            route_tag.as_deref(),
            account_id.as_deref(),
            bot_type.as_deref(),
            force,
        )
        .await
}

pub async fn wait_weixin_qr_login(
    weixin_service: &Arc<WeixinService>,
    bridge_manager: Option<&Arc<BridgeManager>>,
    base_url: String,
    proxy_url: Option<String>,
    route_tag: Option<String>,
    session_key: String,
    bot_type: Option<String>,
    timeout_ms: Option<i64>,
) -> Result<WeixinQrWaitResult> {
    let base_url = normalize_base_url(base_url);
    let result = weixin_service
        .wait_qr_login(
            &base_url,
            proxy_url.as_deref(),
            route_tag.as_deref(),
            &session_key,
            bot_type.as_deref(),
            timeout_ms,
        )
        .await?;

    if result.connected {
        let mut cfg = crate::config::load();
        let next_bridge = weixin_service.apply_login_result(
            cfg.bridge.unwrap_or_default(),
            &result,
            proxy_url,
            route_tag,
        )?;
        cfg.bridge = Some(next_bridge.clone());
        crate::config::save(&cfg)?;
        if let Some(manager) = bridge_manager {
            manager.restart(next_bridge).await;
        }
    }

    Ok(result)
}

pub async fn cancel_weixin_qr_login(weixin_service: &Arc<WeixinService>, session_key: &str) {
    weixin_service.cancel_qr_login(session_key).await;
}

pub fn list_weixin_accounts(weixin_service: &Arc<WeixinService>) -> Vec<StoredWeixinAccount> {
    weixin_service.list_accounts()
}

pub async fn activate_weixin_account(
    weixin_service: &Arc<WeixinService>,
    bridge_manager: Option<&Arc<BridgeManager>>,
    account_id: &str,
) -> Result<BridgeConfig> {
    let mut cfg = crate::config::load();
    let next_bridge = weixin_service.activate_account(cfg.bridge.unwrap_or_default(), account_id)?;
    cfg.bridge = Some(next_bridge.clone());
    crate::config::save(&cfg)?;
    if let Some(manager) = bridge_manager {
        manager.restart(next_bridge.clone()).await;
    }
    Ok(next_bridge)
}

fn normalize_base_url(base_url: String) -> String {
    if base_url.trim().is_empty() {
        DEFAULT_WEIXIN_BASE_URL.to_string()
    } else {
        base_url
    }
}
