use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::http::{header, HeaderMap};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use tokio::sync::broadcast;
use tokio::sync::oneshot;

use openpup::channels;
use openpup::config;
use openpup::core::gateway::events::{ClientToGateway, GatewayEnvelope, GatewayToClient};
use openpup::core::kernel::{
    self, DefaultAuditSink, DefaultMemoryStore, DefaultPersonaProvider, DefaultToolRegistry,
    KernelEnv, ToolExecutor,
};
use openpup::tools::{ToolCall, ToolKind, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct Hub {
    tx: broadcast::Sender<HubMsg>,
}

#[derive(Debug, Clone)]
struct HubMsg {
    topic: String,
    payload_json: String,
}

impl Hub {
    fn new(buffer: usize) -> Self {
        let (tx, _rx) = broadcast::channel(buffer);
        Self { tx }
    }

    fn publish(&self, topic: impl Into<String>, payload: GatewayEnvelope<GatewayToClient>) {
        let topic = topic.into();
        let payload_json = serde_json::to_string(&payload).unwrap_or_else(|e| {
            serde_json::json!({
                "v": 1,
                "data": { "type": "Error", "data": { "message": format!("failed to serialize outbound: {e}") } }
            })
            .to_string()
        });
        let _ = self.tx.send(HubMsg {
            topic,
            payload_json,
        });
    }
}

#[derive(Clone)]
struct AppState {
    hub: Hub,
    auth_token: Option<String>,
    approvals: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<bool>>>>,
}

async fn health() -> impl IntoResponse {
    "ok"
}

async fn index() -> impl IntoResponse {
    Html(include_str!("../web/index.html"))
}

async fn app_js() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/javascript; charset=utf-8".parse().unwrap(),
    );
    (headers, include_str!("../web/app.js"))
}

/// 飞书回调：URL 验证 + 消息事件（最小实现：只处理文本消息）。
#[derive(Debug, Deserialize)]
struct FeishuCallback {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    challenge: Option<String>,
    #[serde(default)]
    event: Option<FeishuEvent>,
}

#[derive(Debug, Deserialize)]
struct FeishuEvent {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    message: Option<FeishuMessage>,
}

#[derive(Debug, Deserialize)]
struct FeishuMessage {
    #[serde(default)]
    chat_id: String,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
    content: String,
}

/// 飞书事件回调入口：
/// - url_verification: 返回 challenge 完成校验；
/// - event_callback: 异步处理消息事件，立即返回 {"code":0,"msg":"ok"}。
async fn feishu_events(Json(payload): Json<FeishuCallback>) -> axum::Json<serde_json::Value> {
    // 1) URL 验证
    if payload.r#type == "url_verification" {
        let challenge = payload.challenge.unwrap_or_default();
        return axum::Json(serde_json::json!({ "challenge": challenge }));
    }

    // 2) 异步处理消息事件，立即返回 200。
    if payload.r#type == "event_callback" {
        if let Some(ev) = payload.event {
            if ev.r#type == "im.message.receive_v1" {
                if let Some(msg) = ev.message {
                    if msg.message_type == "text" && !msg.content.trim().is_empty() {
                        tokio::spawn(async move {
                            // content 是 JSON 字符串，如 {"text":"..."}
                            let text = serde_json::from_str::<serde_json::Value>(&msg.content)
                                .ok()
                                .and_then(|v| {
                                    v.get("text")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string())
                                })
                                .unwrap_or_else(|| msg.content.clone());

                            let session_id = format!("feishu:{}", msg.chat_id);
                            let cfg = match config::load_or_init() {
                                Ok(c) => c,
                                Err(e) => {
                                    let _ = channels::send_feishu_text_to_chat(
                                        &msg.chat_id,
                                        &format!("openpup: failed to load config: {e:#}"),
                                    )
                                    .await;
                                    return;
                                }
                            };

                            let env = KernelEnv {
                                cfg: cfg.clone(),
                                registry: DefaultToolRegistry,
                                executor: Arc::new(kernel::DefaultToolExecutor::new()),
                                memory: DefaultMemoryStore,
                                persona: DefaultPersonaProvider,
                                audit: DefaultAuditSink,
                            };
                            let kernel = kernel::AgentKernel::new(env);
                            let req = openpup::core::kernel::AgentRequest {
                                session_id: session_id.clone(),
                                input: text.clone(),
                                semantic_kind: Some("feishu".to_string()),
                            };

                            let res = kernel.run_turn(req).await;
                            match res {
                                Ok(turn) => {
                                    let _ = channels::send_feishu_text_to_chat(
                                        &msg.chat_id,
                                        &turn.reply_text,
                                    )
                                    .await;
                                }
                                Err(e) => {
                                    let _ = channels::send_feishu_text_to_chat(
                                        &msg.chat_id,
                                        &format!("openpup: chat error: {e:#}"),
                                    )
                                    .await;
                                }
                            }
                        });
                    }
                }
            }
        }
    }

    axum::Json(serde_json::json!({
        "code": 0,
        "msg": "ok"
    }))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

fn is_high_risk(call: &ToolCall) -> bool {
    matches!(
        call.kind,
        ToolKind::RegisterSubAgent
            | ToolKind::RegisterNode
            | ToolKind::InvokeNodeTool
            | ToolKind::L3LogDecision
            | ToolKind::L3UpdateProgress
            | ToolKind::L3AddTodo
            | ToolKind::L3UpdateTodoStatus
    )
}

struct ApprovalToolExecutor {
    inner: kernel::DefaultToolExecutor,
    hub: Hub,
    approvals: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<bool>>>>,
    session_topic: String,
}

impl ApprovalToolExecutor {
    fn new(
        hub: Hub,
        approvals: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<bool>>>>,
        session_topic: String,
    ) -> Self {
        Self {
            inner: kernel::DefaultToolExecutor::new(),
            hub,
            approvals,
            session_topic,
        }
    }

    fn wait_for_approval(&self, approval_id: &str) -> bool {
        let (tx, rx) = oneshot::channel::<bool>();
        {
            let mut map = self.approvals.lock().unwrap();
            map.insert(approval_id.to_string(), tx);
        }
        // 在 tokio runtime 内允许阻塞等待（不建议长时间；默认 5 分钟超时）。
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async move {
                match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                    Ok(Ok(v)) => v,
                    _ => false,
                }
            })
        })
    }
}

impl ToolExecutor for ApprovalToolExecutor {
    fn execute(&self, cfg: &openpup::config::OpenpupConfig, call: &ToolCall) -> ToolResult {
        if is_high_risk(call) && cfg.autonomy.execution_mode != "full" {
            let approval_id = uuid::Uuid::new_v4().to_string();
            let summary = format!("Approve high-risk tool: {:?}", call.kind);
            self.hub.publish(
                self.session_topic.clone(),
                GatewayEnvelope::v1(GatewayToClient::NeedsApproval {
                    approval_id: approval_id.clone(),
                    summary,
                    context: Some(serde_json::json!({
                        "tool_kind": format!("{:?}", call.kind),
                        "args": call.args
                    })),
                }),
            );
            let approved = self.wait_for_approval(&approval_id);
            if !approved {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("approval denied or timed out".to_string()),
                };
            }
        }
        self.inner.execute(cfg, call)
    }
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut authed = state.auth_token.is_none();
    let mut topics: HashSet<String> = HashSet::new();

    // 默认订阅该连接的 session 由客户端自行 Subscribe 决定。
    let mut rx = state.hub.tx.subscribe();

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                let Some(Ok(msg)) = inbound else { break };
                if !handle_inbound_message(&mut socket, &state, &mut authed, &mut topics, msg).await {
                    break;
                }
            }
            outbound = rx.recv() => {
                let Ok(outbound) = outbound else { continue };
                if !topics.contains(&outbound.topic) {
                    continue;
                }
                if socket.send(Message::Text(outbound.payload_json)).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn handle_inbound_message(
    socket: &mut WebSocket,
    state: &AppState,
    authed: &mut bool,
    topics: &mut HashSet<String>,
    msg: Message,
) -> bool {
    let Message::Text(text) = msg else {
        return true;
    };

    let env: GatewayEnvelope<ClientToGateway> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                        message: format!("invalid message JSON: {e}"),
                    }))
                    .unwrap(),
                ))
                .await;
            return true;
        }
    };

    if env.v != 1 {
        let _ = socket
            .send(Message::Text(
                serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                    message: format!("unsupported protocol version {}", env.v),
                }))
                .unwrap(),
            ))
            .await;
        return true;
    }

    match env.data {
        ClientToGateway::Auth { token } => {
            if *authed {
                return true;
            }
            if state.auth_token.as_deref() == Some(token.as_str()) {
                *authed = true;
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Authed))
                            .unwrap(),
                    ))
                    .await;
            } else {
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                            message: "unauthorized".to_string(),
                        }))
                        .unwrap(),
                    ))
                    .await;
                return false;
            }
        }
        _ if !*authed => {
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                        message: "unauthorized: send Auth first".to_string(),
                    }))
                    .unwrap(),
                ))
                .await;
        }
        ClientToGateway::Subscribe { topics: t } => {
            for x in t {
                topics.insert(x);
            }
        }
        ClientToGateway::ApprovalResponse {
            approval_id,
            approve,
        } => {
            let tx = {
                let mut map = state.approvals.lock().unwrap();
                map.remove(&approval_id)
            };
            if let Some(tx) = tx {
                let _ = tx.send(approve);
            }
        }
        ClientToGateway::SendMessage {
            session_id,
            input,
            semantic_kind,
        } => {
            let cfg = match config::load_or_init() {
                Ok(c) => c,
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                                message: format!("failed to load config: {e:#}"),
                            }))
                            .unwrap(),
                        ))
                        .await;
                    return true;
                }
            };
            // 使用带审批的 executor 运行内核（仅网关路径）。
            let executor = ApprovalToolExecutor::new(
                state.hub.clone(),
                Arc::clone(&state.approvals),
                format!("session/{}", session_id.clone()),
            );
            let env = KernelEnv {
                cfg: cfg.clone(),
                registry: DefaultToolRegistry,
                executor: Arc::new(executor),
                memory: DefaultMemoryStore,
                persona: DefaultPersonaProvider,
                audit: DefaultAuditSink,
            };
            let kernel = kernel::AgentKernel::new(env);
            let req = openpup::core::kernel::AgentRequest {
                session_id: session_id.clone(),
                input,
                semantic_kind,
            };

            let result = tokio::task::block_in_place(|| {
                let h = tokio::runtime::Handle::current();
                h.block_on(kernel.run_turn(req))
            });
            match result {
                Ok(result) => {
                    let tool_call = result.tool_call.as_ref().map(|(call, res)| {
                        serde_json::json!({
                            "call": call,
                            "result": res
                        })
                    });
                    state.hub.publish(
                        format!("session/{}", session_id),
                        GatewayEnvelope::v1(GatewayToClient::KernelReply {
                            session_id,
                            reply_text: result.reply_text,
                            tool_call,
                        }),
                    );
                }
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                                message: format!("kernel error: {e:#}"),
                            }))
                            .unwrap(),
                        ))
                        .await;
                }
            }
        }
        ClientToGateway::RunLoop { loop_id } => {
            let ev = openpup::core::runtime::RuntimeEvent::manual(&loop_id);
            if let Err(e) = openpup::core::runtime::handle_event(&ev).await {
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                            message: format!("loop error: {e:#}"),
                        }))
                        .unwrap(),
                    ))
                    .await;
            }
        }
        ClientToGateway::Orchestrate {
            session_id,
            goal,
            agents,
        } => {
            let cfg = match config::load_or_init() {
                Ok(c) => c,
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::to_string(&GatewayEnvelope::v1(GatewayToClient::Error {
                                message: format!("failed to load config: {e:#}"),
                            }))
                            .unwrap(),
                        ))
                        .await;
                    return true;
                }
            };

            let hub = state.hub.clone();
            let topic_session = format!("session/{}", session_id.clone());
            tokio::spawn(async move {
                let publish = |evt: GatewayToClient| {
                    // 编排事件默认走 orchestration/<run_id>，同时也推送到 session/<id> 便于 UI 一处订阅。
                    let env = GatewayEnvelope::v1(evt.clone());
                    match &evt {
                        GatewayToClient::OrchestrationPlan { run_id, .. }
                        | GatewayToClient::OrchestrationStepStarted { run_id, .. }
                        | GatewayToClient::OrchestrationStepFinished { run_id, .. }
                        | GatewayToClient::OrchestrationFinished { run_id, .. } => {
                            hub.publish(format!("orchestration/{}", run_id), env.clone());
                        }
                        _ => {}
                    }
                    hub.publish(topic_session.clone(), env);
                };

                let res = openpup::core::orchestrator::run_planner_executor(
                    &cfg,
                    &session_id,
                    &goal,
                    agents,
                    |evt| publish(evt),
                )
                .await;

                if let Err(e) = res {
                    hub.publish(
                        topic_session,
                        GatewayEnvelope::v1(GatewayToClient::Error {
                            message: format!("orchestrate error: {e:#}"),
                        }),
                    );
                }
            });
        }
    }

    true
}

fn load_auth_token() -> Option<String> {
    std::env::var("OPENPUP_GATEWAY_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::load_or_init().context("load config")?;
    let gateway_cfg = cfg.gateway.clone().unwrap_or_default();

    let addr_s = if !gateway_cfg.bind.trim().is_empty() {
        gateway_cfg.bind.clone()
    } else {
        std::env::var("OPENPUP_GATEWAY_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string())
    };
    let addr: SocketAddr = addr_s
        .parse()
        .context("invalid gateway bind (expected host:port)")?;

    let token = gateway_cfg
        .token_env
        .as_deref()
        .and_then(|k| std::env::var(k).ok())
        .or_else(load_auth_token)
        .filter(|s| !s.trim().is_empty());

    let hub = Hub::new(512);
    let state = AppState {
        hub,
        auth_token: if gateway_cfg.require_auth {
            token
        } else {
            None
        },
        approvals: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/feishu/events", post(feishu_events))
        .with_state(state);

    println!("openpup gateway listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind TCP listener")?;
    axum::serve(listener, app)
        .await
        .context("gateway server error")?;
    Ok(())
}
