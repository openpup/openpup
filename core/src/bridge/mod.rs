use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::RwLock;

use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{info, warn};
use uuid::Uuid;

use crate::agents::alpha::AlphaPup;
use crate::bridge::types::{
    BridgeConfig, BridgeConnectionState, BridgeContext, BridgeOutbox, BridgeStatusEvent,
    InboundMessage, OutboundMessage, OutboundType, Platform,
};

use self::auth::OwnerAuth;
use self::telegram::TelegramBridge;
use self::weixin::{WeixinBridge, WeixinService};

pub mod auth;
pub mod control;
pub mod discord;
pub mod formatter;
pub mod slack;
pub mod telegram;
pub mod types;
pub mod weixin;

pub struct BridgeManager {
    config: RwLock<BridgeConfig>,
    alpha: Arc<AlphaPup>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    weixin_service: Arc<WeixinService>,
    outbox: BridgeOutbox,
}

fn is_stop_request(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "stop" | "abort" | "cancel" | "停止" | "停止吧" | "停下" | "停一下" | "取消" | "/stop"
    )
}

fn is_enabled_config(config: &BridgeConfig) -> bool {
    config.telegram.is_some()
        || config.discord.is_some()
        || config.slack.is_some()
        || config.weixin.is_some()
}

impl BridgeManager {
    pub fn new(
        config: BridgeConfig,
        alpha: Arc<AlphaPup>,
        weixin_service: Arc<WeixinService>,
        outbox: BridgeOutbox,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            alpha,
            tasks: Mutex::new(Vec::new()),
            weixin_service,
            outbox,
        }
    }

    pub fn is_enabled(&self) -> bool {
        is_enabled_config(&self.current_config())
    }

    pub fn current_config(&self) -> BridgeConfig {
        self.config
            .read()
            .expect("bridge config lock poisoned")
            .clone()
    }

    pub fn weixin_service(&self) -> Arc<WeixinService> {
        self.weixin_service.clone()
    }

    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            let config = self.current_config();
            self.apply_config_state(&config).await;
            self.run_with_config(config).await;
        });
    }

    pub async fn restart(self: &Arc<Self>, config: BridgeConfig) {
        {
            let mut guard = self.config.write().expect("bridge config lock poisoned");
            *guard = config.clone();
        }
        self.stop_tasks().await;
        self.apply_config_state(&config).await;
        self.run_with_config(config).await;
    }

    pub async fn stop(self: &Arc<Self>) {
        let config = BridgeConfig::default();
        {
            let mut guard = self.config.write().expect("bridge config lock poisoned");
            *guard = config.clone();
        }
        self.stop_tasks().await;
        self.apply_config_state(&config).await;
    }

    async fn stop_tasks(&self) {
        let mut tasks = self.tasks.lock().await;
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    async fn apply_config_state(&self, config: &BridgeConfig) {
        for platform in [
            Platform::Telegram,
            Platform::Discord,
            Platform::Slack,
            Platform::Weixin,
        ] {
            let enabled = match platform {
                Platform::Telegram => config.telegram.is_some(),
                Platform::Discord => config.discord.is_some(),
                Platform::Slack => config.slack.is_some(),
                Platform::Weixin => config.weixin.is_some(),
            };
            let status = if enabled {
                BridgeConnectionState::Connecting
            } else {
                BridgeConnectionState::Unconfigured
            };
            let _ = self
                .alpha
                .memory
                .update_bridge_connection(platform.as_str(), &status, false, None, None)
                .await;
        }
    }

    async fn run_with_config(self: &Arc<Self>, config: BridgeConfig) {
        if !is_enabled_config(&config) {
            return;
        }
        info!("[bridge] starting configured platform bridges");

        let (inbound_tx, mut inbound_rx) = mpsc::channel::<InboundMessage>(128);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundMessage>(256);
        let (status_tx, mut status_rx) = mpsc::channel::<BridgeStatusEvent>(32);

        // Publish the outbound sender so tools (bridge_send) can use it.
        *self.outbox.lock().await = Some(outbound_tx.clone());
        let mut outbound_routes: HashMap<Platform, mpsc::Sender<OutboundMessage>> = HashMap::new();
        let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        if let Some(tg_cfg) = config.telegram.clone() {
            let tx = inbound_tx.clone();
            let status_sender = status_tx.clone();
            let bridge = TelegramBridge::new(tg_cfg, tx, Some(status_sender.clone()));
            let alpha = self.alpha.clone();

            let (tg_out_tx, mut tg_out_rx) = mpsc::channel::<OutboundMessage>(64);
            outbound_routes.insert(Platform::Telegram, tg_out_tx);

            let alpha_for_sender = alpha.clone();
            let sender = bridge.clone();
            handles.push(tokio::spawn(async move {
                while let Some(msg) = tg_out_rx.recv().await {
                    if let Err(e) = sender.send(&msg).await {
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Telegram.as_str(),
                                &BridgeConnectionState::Error,
                                false,
                                None,
                                Some(&e.to_string()),
                            )
                            .await;
                        warn!("[telegram] send error: {e}");
                    } else {
                        let _ = alpha_for_sender
                            .memory
                            .record_external_outbound(&Uuid::new_v4().to_string(), &msg)
                            .await;
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Telegram.as_str(),
                                &BridgeConnectionState::Connected,
                                true,
                                Some(chrono::Utc::now().timestamp()),
                                None,
                            )
                            .await;
                    }
                }
            }));

            handles.push(tokio::spawn(async move {
                if let Err(e) = bridge.start_polling().await {
                    let _ = status_sender
                        .send(BridgeStatusEvent {
                            platform: Platform::Telegram,
                            status: BridgeConnectionState::Error,
                            connected: false,
                            last_seen: None,
                            error_msg: Some(e.to_string()),
                        })
                        .await;
                    warn!("[telegram] polling stopped: {e}");
                }
            }));
        }

        if let Some(wx_cfg) = config.weixin.clone() {
            let tx = inbound_tx.clone();
            let status_sender = status_tx.clone();
            let bridge = WeixinBridge::new(wx_cfg, tx, Some(status_sender.clone()));
            let alpha = self.alpha.clone();

            let (wx_out_tx, mut wx_out_rx) = mpsc::channel::<OutboundMessage>(64);
            outbound_routes.insert(Platform::Weixin, wx_out_tx);

            let alpha_for_sender = alpha.clone();
            let sender = bridge.clone();
            handles.push(tokio::spawn(async move {
                while let Some(msg) = wx_out_rx.recv().await {
                    if let Err(e) = sender.send(&msg).await {
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Weixin.as_str(),
                                &BridgeConnectionState::Error,
                                false,
                                None,
                                Some(&e.to_string()),
                            )
                            .await;
                        warn!("[weixin] send error: {e}");
                    } else {
                        let _ = alpha_for_sender
                            .memory
                            .record_external_outbound(&Uuid::new_v4().to_string(), &msg)
                            .await;
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::Weixin.as_str(),
                                &BridgeConnectionState::Connected,
                                true,
                                Some(chrono::Utc::now().timestamp()),
                                None,
                            )
                            .await;
                    }
                }
            }));

            handles.push(tokio::spawn(async move {
                if let Err(e) = bridge.start_polling().await {
                    let _ = status_sender
                        .send(BridgeStatusEvent {
                            platform: Platform::Weixin,
                            status: BridgeConnectionState::Error,
                            connected: false,
                            last_seen: None,
                            error_msg: Some(e.to_string()),
                        })
                        .await;
                    warn!("[weixin] polling stopped: {e}");
                }
            }));
        }

        handles.push(tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                if let Some(route_tx) = outbound_routes.get(&msg.platform) {
                    let _ = route_tx.send(msg).await;
                }
            }
        }));

        let status_alpha = self.alpha.clone();
        handles.push(tokio::spawn(async move {
            while let Some(event) = status_rx.recv().await {
                let _ = status_alpha
                    .memory
                    .update_bridge_connection(
                        event.platform.as_str(),
                        &event.status,
                        event.connected,
                        event.last_seen,
                        event.error_msg.as_deref(),
                    )
                    .await;
            }
        }));

        let manager = self.clone();
        handles.push(tokio::spawn(async move {
            while let Some(inbound) = inbound_rx.recv().await {
                let _ = manager
                    .alpha
                    .memory
                    .record_external_inbound(&Uuid::new_v4().to_string(), &inbound)
                    .await;
                let _ = manager
                    .alpha
                    .memory
                    .update_bridge_connection(
                        inbound.platform.as_str(),
                        &BridgeConnectionState::Connected,
                        true,
                        Some(inbound.timestamp),
                        None,
                    )
                    .await;

                let auth = OwnerAuth::new(manager.current_config());
                if let Err(e) = auth.verify(&inbound) {
                    warn!("[bridge] rejected message: {e}");
                    continue;
                }

                let alpha = manager.alpha.clone();
                let out_tx = outbound_tx.clone();
                let chat_id = inbound.chat_id.clone();
                let msg_id = inbound.message_id.clone();
                let platform = inbound.platform.clone();
                let inbound_text = inbound.text.clone();

                if is_stop_request(&inbound_text) {
                    manager.alpha.abort_flag.store(true, Ordering::Relaxed);
                    let _ = outbound_tx
                        .send(OutboundMessage {
                            platform,
                            chat_id,
                            text: "⏹ 已发送停止信号，当前处理会尽快停止。".to_string(),
                            reply_to_id: Some(msg_id),
                            msg_type: OutboundType::Result,
                        })
                        .await;
                    continue;
                }

                tokio::spawn(async move {
                    let _ = out_tx
                        .send(OutboundMessage {
                            platform: platform.clone(),
                            chat_id: chat_id.clone(),
                            text: "🐾 收到，处理中…".to_string(),
                            reply_to_id: Some(msg_id.clone()),
                            msg_type: OutboundType::Ack,
                        })
                        .await;

                    let (reply_tx, reply_rx) = oneshot::channel::<String>();
                    let _ = reply_tx;
                    let bridge_ctx = BridgeContext {
                        platform: platform.clone(),
                        chat_id: chat_id.clone(),
                        out_tx: out_tx.clone(),
                        reply_rx: tokio::sync::Mutex::new(Some(reply_rx)),
                    };

                    alpha
                        .set_layer_hook(Some(Arc::new({
                            let out_tx = out_tx.clone();
                            let platform = platform.clone();
                            let chat_id = chat_id.clone();
                            move |layer_idx, done_pups| {
                                let out_tx = out_tx.clone();
                                let platform = platform.clone();
                                let chat_id = chat_id.clone();
                                tokio::spawn(async move {
                                    let _ = out_tx
                                        .send(OutboundMessage {
                                            platform,
                                            chat_id,
                                            text: formatter::format_progress(&done_pups, layer_idx),
                                            reply_to_id: None,
                                            msg_type: OutboundType::Progress,
                                        })
                                        .await;
                                });
                            }
                        })))
                        .await;

                    let progress_hook = Arc::new({
                        let out_tx = out_tx.clone();
                        let platform = platform.clone();
                        let chat_id = chat_id.clone();
                        move |text: String| {
                            let out_tx = out_tx.clone();
                            let platform = platform.clone();
                            let chat_id = chat_id.clone();
                            tokio::spawn(async move {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text,
                                        reply_to_id: None,
                                        msg_type: OutboundType::Progress,
                                    })
                                    .await;
                            });
                        }
                    });

                    match alpha
                        .process_bridge_message(inbound_text, Some(progress_hook))
                        .await
                    {
                        Ok(result) => {
                            let _ = out_tx
                                .send(OutboundMessage {
                                    platform,
                                    chat_id,
                                    text: formatter::format_result(&result),
                                    reply_to_id: Some(msg_id),
                                    msg_type: OutboundType::Result,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = out_tx
                                .send(OutboundMessage {
                                    platform,
                                    chat_id,
                                    text: format!("❌ Error: {e}"),
                                    reply_to_id: Some(msg_id),
                                    msg_type: OutboundType::Error,
                                })
                                .await;
                        }
                    }

                    alpha.set_layer_hook(None).await;
                    let _ = bridge_ctx;
                });
            }
        }));

        self.tasks.lock().await.extend(handles);
    }
}
