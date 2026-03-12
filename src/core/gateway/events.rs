use serde::{Deserialize, Serialize};
use serde_json::Value;

/// WebSocket/HTTP 网关消息统一包裹（带 version，便于未来演进）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayEnvelope<T> {
    pub v: u32,
    pub data: T,
}

impl<T> GatewayEnvelope<T> {
    pub fn v1(data: T) -> Self {
        Self { v: 1, data }
    }
}

/// 客户端 -> 网关。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClientToGateway {
    /// WS 鉴权（建议作为 WS 首帧）。
    Auth { token: String },

    /// 订阅 topics：例如 `session/<id>`、`audit`、`orchestration/<run_id>`。
    Subscribe { topics: Vec<String> },

    /// 发送一条用户输入给内核（单轮）。
    SendMessage {
        session_id: String,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        semantic_kind: Option<String>,
    },

    /// 触发一次 Loop。
    RunLoop { loop_id: String },

    /// 触发一次 Planner-Executor 编排。
    Orchestrate {
        session_id: String,
        goal: String,
        #[serde(default)]
        agents: Vec<String>,
    },

    /// 对审批请求的响应。
    ApprovalResponse { approval_id: String, approve: bool },
}

/// 网关 -> 客户端（事件流）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum GatewayToClient {
    /// 连接已鉴权。
    Authed,

    /// 通用错误（可用于协议/鉴权/执行失败等）。
    Error { message: String },

    /// 内核单轮回复（非 streaming）。
    KernelReply {
        session_id: String,
        reply_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call: Option<Value>,
    },

    /// 编排：Planner 产出的 plan。
    OrchestrationPlan {
        run_id: String,
        goal: String,
        plan: Value,
    },

    /// 编排：单步开始。
    OrchestrationStepStarted {
        run_id: String,
        step_idx: usize,
        agent: String,
        input: String,
    },

    /// 编排：单步完成。
    OrchestrationStepFinished {
        run_id: String,
        step_idx: usize,
        agent: String,
        ok: bool,
        output: Value,
    },

    /// 编排：最终汇总输出。
    OrchestrationFinished {
        run_id: String,
        ok: bool,
        summary: String,
    },

    /// 审计事件（JSON 原样透传，便于 UI 展示）。
    AuditEvent { event: Value },

    /// 需要人类审批才能继续（用于高风险工具/动作）。
    NeedsApproval {
        approval_id: String,
        /// 建议 UI 展示的标题/动作摘要。
        summary: String,
        /// 可选：结构化上下文（脱敏）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<Value>,
    },
}
