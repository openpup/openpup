//! Core 层：运行时事件类型与触发源 + 事件处理入口 handle_event。

/// 触发来源类型。
#[derive(Debug, Clone)]
pub enum TriggerKind {
    /// 定时任务（scheduler）
    Time,
    /// 手动触发（CLI）
    Manual,
    /// 消息/外部事件（预留，用于 Telegram/Discord/WhatsApp 等）
    Message,
}

/// 触发来源的结构化描述，便于在多通道、多账号场景下做精确控制。
#[derive(Debug, Clone)]
pub struct TriggerSource {
    /// 通道标识，例如 "local" / "telegram" / "discord" / "whatsapp"。
    pub channel: String,
    /// 账号或会话主体标识，例如 "cli" / "scheduler" / 具体 user id。
    pub account: String,
    /// 额外上下文，例如 room/channel id、webhook id 等（可选）。
    pub context: Option<String>,
}

impl TriggerSource {
    /// 本地 CLI 触发。
    pub fn local_cli() -> Self {
        TriggerSource {
            channel: "local".to_string(),
            account: "cli".to_string(),
            context: None,
        }
    }

    /// 本地调度器触发。
    pub fn local_scheduler() -> Self {
        TriggerSource {
            channel: "local".to_string(),
            account: "scheduler".to_string(),
            context: None,
        }
    }

    /// 通用构造，供未来通道适配器使用。
    pub fn new(channel: &str, account: &str, context: Option<String>) -> Self {
        TriggerSource {
            channel: channel.to_string(),
            account: account.to_string(),
            context,
        }
    }
}

/// 统一的运行时事件。
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// 基于文件的只读/草稿 Loop 事件。
    Loop {
        /// 要执行的 Loop 标识，例如 "work_morning"。
        loop_id: String,
        /// 触发类型：时间 / 手动 / 消息。
        trigger: TriggerKind,
        /// 触发来源的结构化信息（通道/账号/上下文）。
        source: TriggerSource,
    },
    /// 单次 Agent 请求（CLI/HTTP 等上层封装使用）。
    AgentRequest {
        session_id: String,
        input: String,
    },
    /// Planner-Executor 编排请求（供网关/通道/未来调度触发）。
    Orchestrate {
        session_id: String,
        goal: String,
        agents: Vec<String>,
    },
    /// 声明或更新一个子 Agent（spawn）。
    SpawnRequest {
        name: String,
        model: Option<String>,
        persona: Option<String>,
    },
    /// 节点心跳或状态上报（multi-node 预留）。
    NodeHeartbeat {
        node_id: String,
        status: String,
    },
}

impl RuntimeEvent {
    /// CLI 手动触发某个 Loop。
    pub fn manual(loop_id: &str) -> Self {
        RuntimeEvent::Loop {
            loop_id: loop_id.to_string(),
            trigger: TriggerKind::Manual,
            source: TriggerSource::local_cli(),
        }
    }

    /// 调度器按时间触发某个 Loop。
    pub fn time(loop_id: &str) -> Self {
        RuntimeEvent::Loop {
            loop_id: loop_id.to_string(),
            trigger: TriggerKind::Time,
            source: TriggerSource::local_scheduler(),
        }
    }
}

// -----------------------------------------------------------------------------
// 事件处理入口（由 kernel 迁入）
// -----------------------------------------------------------------------------

use crate::config;
use crate::core::kernel::{self, AgentRequest};
use crate::core::orchestrator;
use crate::core::registry;
use crate::loops;

/// 运行时事件处理入口：根据事件类型路由到 Loop / Agent / Spawn / Node 等逻辑。
/// 统一改为异步接口，由上层 runtime 负责创建 Tokio runtime 并 .await。
pub async fn handle_event(ev: &RuntimeEvent) -> anyhow::Result<()> {
    match ev {
        RuntimeEvent::Loop { loop_id, .. } => match loop_id.as_str() {
            "work_morning"
            | "work_plan_draft"
            | "invest_morning"
            | "invest_close"
            | "life_morning"
            | "life_evening" => loops::run(loop_id),
            _ => Ok(()),
        },
        RuntimeEvent::AgentRequest { session_id, input } => {
            let cfg = config::load_or_init()?;
            let req = AgentRequest {
                session_id: session_id.clone(),
                input: input.clone(),
                semantic_kind: Some("loop_log".to_string()),
            };
            let kernel = kernel::DefaultKernel::from_config(cfg.clone());
            let _result = kernel.run_turn(req).await?;
            // 审计已在内核 run_turn 内经 AuditSink trait 完成，此处不再直写。
            Ok(())
        }
        RuntimeEvent::Orchestrate { session_id, goal, agents } => {
            let cfg = config::load_or_init()?;
            // runtime 入口不负责事件流输出；如需事件流由网关层注入 emitter。
            orchestrator::run_planner_executor(
                &cfg,
                session_id,
                goal,
                agents.clone(),
                |_| {},
            ).await?;
            Ok(())
        }
        RuntimeEvent::SpawnRequest { name, model, persona } => {
            let spec = registry::SubAgentSpec {
                name: name.clone(),
                model: model.clone(),
                persona: persona.clone(),
            };
            registry::register_sub_agent(spec)
        }
        RuntimeEvent::NodeHeartbeat { node_id, status } => {
            registry::update_node_heartbeat(node_id, status)
        }
    }
}
