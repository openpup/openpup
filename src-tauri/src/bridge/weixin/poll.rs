use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{mpsc, RwLock};
use tracing::warn;

use crate::bridge::types::{BridgeConnectionState, BridgeStatusEvent, InboundMessage, Platform};

use super::client::{WeixinApiClient, DEFAULT_LONG_POLL_TIMEOUT_MS};
use super::inbound::parse_inbound_message;
use super::store::{load_get_updates_buf, save_get_updates_buf};
use super::types::WeixinMessage;

const MAX_CONSECUTIVE_FAILURES: usize = 3;
const BACKOFF_DELAY_MS: u64 = 30_000;
const RETRY_DELAY_MS: u64 = 2_000;

pub async fn start_poll_loop(
    client: WeixinApiClient,
    inbound_tx: mpsc::Sender<InboundMessage>,
    status_tx: Option<mpsc::Sender<BridgeStatusEvent>>,
    contexts: Arc<RwLock<HashMap<String, String>>>,
) -> Result<()> {
    let account_id = client.account_id().to_string();
    let mut cursor = load_get_updates_buf(&account_id).unwrap_or_default();
    let mut timeout_ms = DEFAULT_LONG_POLL_TIMEOUT_MS;
    let mut consecutive_failures = 0usize;

    loop {
        match client.get_updates(&cursor, timeout_ms).await {
            Ok(resp) => {
                consecutive_failures = 0;
                if let Some(next_cursor) = resp.get_updates_buf.as_deref() {
                    if next_cursor != cursor {
                        cursor = next_cursor.to_string();
                        let _ = save_get_updates_buf(&account_id, &cursor);
                    }
                }
                if let Some(next_timeout) = resp.longpolling_timeout_ms {
                    timeout_ms = next_timeout.max(5_000);
                }
                if let Some(status_tx) = &status_tx {
                    let _ = status_tx
                        .send(BridgeStatusEvent {
                            platform: Platform::Weixin,
                            status: BridgeConnectionState::Connected,
                            connected: true,
                            last_seen: Some(chrono::Utc::now().timestamp()),
                            error_msg: None,
                        })
                        .await;
                }
                for message in resp.msgs.unwrap_or_default() {
                    handle_inbound_message(message, &contexts, &inbound_tx).await;
                }
            }
            Err(error) => {
                consecutive_failures += 1;
                if let Some(status_tx) = &status_tx {
                    let _ = status_tx
                        .send(BridgeStatusEvent {
                            platform: Platform::Weixin,
                            status: BridgeConnectionState::Error,
                            connected: false,
                            last_seen: None,
                            error_msg: Some(error.to_string()),
                        })
                        .await;
                }
                warn!("[weixin] polling error: {error}");
                let delay = if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    consecutive_failures = 0;
                    BACKOFF_DELAY_MS
                } else {
                    RETRY_DELAY_MS
                };
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                continue;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn handle_inbound_message(
    message: WeixinMessage,
    contexts: &Arc<RwLock<HashMap<String, String>>>,
    inbound_tx: &mpsc::Sender<InboundMessage>,
) {
    if let Some(parsed) = parse_inbound_message(&message) {
        if let Some(context_token) = parsed.context_token {
            contexts
                .write()
                .await
                .insert(parsed.inbound.chat_id.clone(), context_token);
        }
        let _ = inbound_tx.send(parsed.inbound).await;
    }
}
