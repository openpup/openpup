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
use crate::runtime::{emit_event, SharedEventSink};

use self::auth::OwnerAuth;
use self::qqbot::QQBotBridge;
use self::telegram::TelegramBridge;
use self::weixin::{WeixinBridge, WeixinService};

pub mod auth;
pub mod control;
pub mod discord;
pub mod formatter;
pub mod qqbot;
pub mod slack;
pub mod telegram;
pub mod types;
pub mod weixin;

pub struct BridgeManager {
    config: RwLock<BridgeConfig>,
    alpha: Arc<AlphaPup>,
    workspace_root: std::path::PathBuf,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    weixin_service: Arc<WeixinService>,
    outbox: BridgeOutbox,
    event_sink: Arc<RwLock<Option<SharedEventSink>>>,
    group_routes: Mutex<HashMap<String, BridgeGroupRoute>>,
    xmtp_helper: Option<Arc<crate::xmtp_helper::XmtpNodeHelper>>,
}

#[derive(Debug, Clone)]
struct BridgeGroupRoute {
    conversation_id: String,
    title: String,
    expires_at: i64,
}

fn is_stop_request(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "stop" | "abort" | "cancel" | "停止" | "停止吧" | "停下" | "停一下" | "取消" | "/stop"
    )
}

fn bridge_route_key(platform: &Platform, chat_id: &str, user_id: &str) -> String {
    format!("{}:{chat_id}:{user_id}", platform.as_str())
}

fn parse_group_command(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let rest = trimmed
        .strip_prefix("/g ")
        .or_else(|| trimmed.strip_prefix("/group "))?
        .trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let target = parts.next()?.trim();
    let content = parts.next()?.trim();
    if target.is_empty() || content.is_empty() {
        return None;
    }
    Some((target.to_string(), content.to_string()))
}

fn parse_use_command(text: &str) -> Option<String> {
    let target = text.trim().strip_prefix("/use ")?.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

fn mentions_alpha(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | '，' | '.' | '。' | ':' | '：' | ';' | '；' | ')' | '）' | '(' | '（'
            )
        });
        token
            .strip_prefix('@')
            .or_else(|| token.strip_prefix('＠'))
            .map(|mention| mention.eq_ignore_ascii_case("alpha"))
            .unwrap_or(false)
    })
}

fn bridge_member_profile(platform: &Platform) -> (String, &'static str) {
    match platform {
        Platform::QQBot => ("QQBot 用户".to_string(), "qqbot-user"),
        Platform::Weixin => ("微信用户".to_string(), "weixin-user"),
        Platform::Telegram => ("Telegram 用户".to_string(), "telegram-user"),
        _ => ("Bridge 用户".to_string(), "bridge-user"),
    }
}

fn is_enabled_config(config: &BridgeConfig) -> bool {
    config.telegram.is_some()
        || config.discord.is_some()
        || config.slack.is_some()
        || config.weixin.is_some()
        || config.qqbot.is_some()
}

impl BridgeManager {
    pub fn new(
        config: BridgeConfig,
        alpha: Arc<AlphaPup>,
        workspace_root: std::path::PathBuf,
        weixin_service: Arc<WeixinService>,
        outbox: BridgeOutbox,
        xmtp_helper: Option<Arc<crate::xmtp_helper::XmtpNodeHelper>>,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            alpha,
            workspace_root,
            tasks: Mutex::new(Vec::new()),
            weixin_service,
            outbox,
            event_sink: Arc::new(RwLock::new(None)),
            group_routes: Mutex::new(HashMap::new()),
            xmtp_helper,
        }
    }

    pub fn set_event_sink(&self, sink: SharedEventSink) {
        let mut guard = self
            .event_sink
            .write()
            .expect("bridge event sink lock poisoned");
        *guard = Some(sink);
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

    fn emit_conversation_message(
        &self,
        message: crate::conversation::types::ConversationMessageRecord,
    ) {
        let sink = self
            .event_sink
            .read()
            .expect("bridge event sink lock poisoned")
            .clone();
        if let Some(sink) = sink {
            emit_event(
                sink.as_ref(),
                "conversation_message_created",
                crate::conversation::types::ConversationMessageCreatedPayload {
                    conversation_id: message.conversation_id.clone(),
                    message,
                },
            );
        }
    }

    async fn emit_conversation_members_changed(&self, conversation_id: &str) {
        let sink = self
            .event_sink
            .read()
            .expect("bridge event sink lock poisoned")
            .clone();
        let Some(sink) = sink else {
            return;
        };
        if let Ok(members) = self
            .alpha
            .memory
            .list_conversation_members(conversation_id)
            .await
        {
            emit_event(
                sink.as_ref(),
                "conversation_members_changed",
                crate::conversation::types::ConversationMembersChangedPayload {
                    conversation_id: conversation_id.to_string(),
                    member_count: members
                        .iter()
                        .filter(|member| member.status == "active")
                        .count() as i64,
                    members,
                },
            );
        }
    }

    async fn emit_conversation_spaces_changed(&self) {
        let sink = self
            .event_sink
            .read()
            .expect("bridge event sink lock poisoned")
            .clone();
        let Some(sink) = sink else {
            return;
        };
        if let Ok(spaces) = self.alpha.memory.list_conversation_spaces().await {
            emit_event(
                sink.as_ref(),
                "conversation_spaces_changed",
                crate::conversation::types::ConversationSpacesChangedPayload { spaces },
            );
        }
    }

    async fn set_group_route(&self, key: String, route: BridgeGroupRoute) {
        self.group_routes.lock().await.insert(key, route);
    }

    async fn clear_group_route(&self, key: &str) {
        self.group_routes.lock().await.remove(key);
    }

    async fn active_group_route(&self, key: &str) -> Option<BridgeGroupRoute> {
        let now = chrono::Utc::now().timestamp();
        let mut routes = self.group_routes.lock().await;
        let route = routes.get(key).cloned()?;
        if route.expires_at < now {
            routes.remove(key);
            None
        } else {
            Some(route)
        }
    }

    async fn handle_group_inbound(
        self: Arc<Self>,
        alpha: Arc<AlphaPup>,
        out_tx: mpsc::Sender<OutboundMessage>,
        platform: Platform,
        chat_id: String,
        msg_id: String,
        user_id: String,
        space: crate::conversation::types::ConversationSpaceRecord,
        group_text: String,
    ) {
        let route_label = format!("{} · bridge", platform.as_str());
        let identity_id = format!("bridge:{}:{user_id}", platform.as_str());
        let (display_name, mention_key) = bridge_member_profile(&platform);

        if let Err(e) = alpha
            .memory
            .ensure_conversation_member(
                &space.id,
                &identity_id,
                &display_name,
                Some(mention_key),
                &route_label,
                "member",
            )
            .await
        {
            let _ = out_tx
                .send(OutboundMessage {
                    platform,
                    chat_id,
                    text: format!("❌ 加入群成员失败：{e}"),
                    reply_to_id: Some(msg_id),
                    msg_type: OutboundType::Error,
                })
                .await;
            return;
        }
        self.emit_conversation_members_changed(&space.id).await;
        self.emit_conversation_spaces_changed().await;

        let _ = out_tx
            .send(OutboundMessage {
                platform: platform.clone(),
                chat_id: chat_id.clone(),
                text: format!("🐾 收到，已发到 #{}，处理中…", space.title),
                reply_to_id: Some(msg_id.clone()),
                msg_type: OutboundType::Ack,
            })
            .await;

        let sender_route_id = format!("{}:{}:{}", platform.as_str(), chat_id, msg_id);
        if let Ok(message) = alpha
            .memory
            .post_conversation_message(
                &space.id,
                &identity_id,
                Some(&sender_route_id),
                &display_name,
                "human",
                Some(&route_label),
                &group_text,
            )
            .await
        {
            if let Some(xmtp_helper) = &self.xmtp_helper {
                if let Err(e) = xmtp_helper
                    .publish_message(&alpha.memory, &self.workspace_root, &space, &message)
                    .await
                {
                    warn!("[bridge] failed to publish group message to XMTP: {e}");
                }
            }
            self.emit_conversation_message(message);
            self.emit_conversation_spaces_changed().await;
        }

        let mention_required = space
            .transports
            .iter()
            .any(|transport| transport.kind == "xmtp" && transport.status == "active");
        if mention_required && !mentions_alpha(&group_text) {
            return;
        }

        match alpha
            .process_group_message(&space.id, &space.title, &group_text)
            .await
        {
            Ok(reply) => {
                if !reply.is_empty() && !alpha.abort_flag.load(Ordering::Relaxed) {
                    if let Ok(message) = alpha
                        .memory
                        .post_conversation_message(
                            &space.id,
                            "agent_alpha",
                            None,
                            "Alpha",
                            "agent",
                            Some("openpup.alpha"),
                            &reply,
                        )
                        .await
                    {
                        if let Some(xmtp_helper) = &self.xmtp_helper {
                            if let Err(e) = xmtp_helper
                                .publish_message(
                                    &alpha.memory,
                                    &self.workspace_root,
                                    &space,
                                    &message,
                                )
                                .await
                            {
                                warn!("[bridge] failed to publish Alpha group reply to XMTP: {e}");
                            }
                        }
                        self.emit_conversation_message(message);
                        self.emit_conversation_spaces_changed().await;
                    }
                }
                let segments = formatter::format_result(&reply);
                for (i, seg) in segments.into_iter().enumerate() {
                    let _ = out_tx
                        .send(OutboundMessage {
                            platform: platform.clone(),
                            chat_id: chat_id.clone(),
                            text: seg,
                            reply_to_id: if i == 0 { Some(msg_id.clone()) } else { None },
                            msg_type: OutboundType::Result,
                        })
                        .await;
                }
            }
            Err(e) => {
                let _ = out_tx
                    .send(OutboundMessage {
                        platform,
                        chat_id,
                        text: format!("❌ 群聊处理失败：{e}"),
                        reply_to_id: Some(msg_id),
                        msg_type: OutboundType::Error,
                    })
                    .await;
            }
        }
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
            Platform::QQBot,
        ] {
            let enabled = match platform {
                Platform::Telegram => config.telegram.is_some(),
                Platform::Discord => config.discord.is_some(),
                Platform::Slack => config.slack.is_some(),
                Platform::Weixin => config.weixin.is_some(),
                Platform::QQBot => config.qqbot.is_some(),
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

        if let Some(qq_cfg) = config.qqbot.clone() {
            let tx = inbound_tx.clone();
            let status_sender = status_tx.clone();
            let bridge = QQBotBridge::new(qq_cfg, tx, Some(status_sender.clone()));
            let alpha = self.alpha.clone();

            let (qq_out_tx, mut qq_out_rx) = mpsc::channel::<OutboundMessage>(64);
            outbound_routes.insert(Platform::QQBot, qq_out_tx);

            let alpha_for_sender = alpha.clone();
            let sender = bridge.clone();
            handles.push(tokio::spawn(async move {
                while let Some(msg) = qq_out_rx.recv().await {
                    if let Err(e) = sender.send(&msg).await {
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::QQBot.as_str(),
                                &BridgeConnectionState::Error,
                                false,
                                None,
                                Some(&e.to_string()),
                            )
                            .await;
                        warn!("[qqbot] send error: {e}");
                    } else {
                        let _ = alpha_for_sender
                            .memory
                            .record_external_outbound(&Uuid::new_v4().to_string(), &msg)
                            .await;
                        let _ = alpha_for_sender
                            .memory
                            .update_bridge_connection(
                                Platform::QQBot.as_str(),
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
                            platform: Platform::QQBot,
                            status: BridgeConnectionState::Error,
                            connected: false,
                            last_seen: None,
                            error_msg: Some(e.to_string()),
                        })
                        .await;
                    warn!("[qqbot] polling stopped: {e}");
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
                    warn!(
                        "[bridge] rejected message from {}:{} (user_id={}, chat_id={}): {e}",
                        inbound.platform.as_str(),
                        inbound.message_id,
                        inbound.user_id,
                        inbound.chat_id,
                    );
                    continue;
                }

                let alpha = manager.alpha.clone();
                let out_tx = outbound_tx.clone();
                let chat_id = inbound.chat_id.clone();
                let msg_id = inbound.message_id.clone();
                let platform = inbound.platform.clone();
                let inbound_text = inbound.text.clone();
                let route_key = bridge_route_key(&platform, &chat_id, &inbound.user_id);

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

                if let Some(group_target) = parse_use_command(&inbound_text) {
                    let out_tx = outbound_tx.clone();
                    let route_key = route_key.clone();
                    let manager_for_route = manager.clone();
                    tokio::spawn(async move {
                        if matches!(group_target.as_str(), "personal" | "个人" | "私聊") {
                            manager_for_route.clear_group_route(&route_key).await;
                            let _ = out_tx
                                .send(OutboundMessage {
                                    platform,
                                    chat_id,
                                    text: "已切回个人工作区。".to_string(),
                                    reply_to_id: Some(msg_id),
                                    msg_type: OutboundType::Ack,
                                })
                                .await;
                            return;
                        }

                        match alpha.memory.find_conversation_space(&group_target).await {
                            Ok(Some(space)) => {
                                manager_for_route
                                    .set_group_route(
                                        route_key,
                                        BridgeGroupRoute {
                                            conversation_id: space.id.clone(),
                                            title: space.title.clone(),
                                            expires_at: chrono::Utc::now().timestamp() + 30 * 60,
                                        },
                                    )
                                    .await;
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!(
                                            "已切到 #{}，30 分钟内普通消息会进入这个群。发送 /use personal 可返回个人工作区。",
                                            space.title
                                        ),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Ack,
                                    })
                                    .await;
                            }
                            Ok(None) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 未找到群：{group_target}"),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 查找群失败：{e}"),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                        }
                    });
                    continue;
                }

                if let Some((group_target, group_text)) = parse_group_command(&inbound_text) {
                    let user_id = inbound.user_id.clone();
                    let manager_for_events = manager.clone();

                    tokio::spawn(async move {
                        match alpha.memory.find_conversation_space(&group_target).await {
                            Ok(Some(space)) => {
                                manager_for_events
                                    .handle_group_inbound(
                                        alpha,
                                        out_tx,
                                        platform,
                                        chat_id,
                                        msg_id,
                                        user_id,
                                        space,
                                        group_text,
                                    )
                                    .await;
                            }
                            Ok(None) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 未找到群：{group_target}"),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 查找群失败：{e}"),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                        }
                    });
                    continue;
                }

                if let Some(route) = manager.active_group_route(&route_key).await {
                    let user_id = inbound.user_id.clone();
                    let group_text = inbound_text.clone();
                    let manager_for_events = manager.clone();
                    tokio::spawn(async move {
                        match alpha.memory.get_conversation_space(&route.conversation_id).await {
                            Ok(Some(space)) => {
                                manager_for_events
                                    .handle_group_inbound(
                                        alpha,
                                        out_tx,
                                        platform,
                                        chat_id,
                                        msg_id,
                                        user_id,
                                        space,
                                        group_text,
                                    )
                                    .await;
                            }
                            Ok(None) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 群已不存在：{}", route.title),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform,
                                        chat_id,
                                        text: format!("❌ 查找群失败：{e}"),
                                        reply_to_id: Some(msg_id),
                                        msg_type: OutboundType::Error,
                                    })
                                    .await;
                            }
                        }
                    });
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
                            let segments = formatter::format_result(&result);
                            for (i, seg) in segments.into_iter().enumerate() {
                                let _ = out_tx
                                    .send(OutboundMessage {
                                        platform: platform.clone(),
                                        chat_id: chat_id.clone(),
                                        text: seg,
                                        reply_to_id: if i == 0 {
                                            Some(msg_id.clone())
                                        } else {
                                            None
                                        },
                                        msg_type: OutboundType::Result,
                                    })
                                    .await;
                            }
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
