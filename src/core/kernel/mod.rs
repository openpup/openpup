//! Agent 内核：统一封装决策循环接口，后续通过 trait 插槽挂接工具/记忆/审计等能力。
//!
//! 事件处理入口在 core::runtime::handle_event。

pub mod node_transport;

use std::sync::Arc;

use anyhow::Result;

use crate::config::OpenpupConfig;
use crate::core::agent_runtime;
use crate::core::llm;
use crate::core::persona;
use crate::core::runtime_audit;
use crate::core::runtime_audit as runtime_audit_core;
use crate::tools::{self, ExposedTool, ToolCall, ToolResult};

/// 对外暴露的统一 Agent 请求结构。
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub session_id: String,
    pub input: String,
    /// 预留：语义记忆分类 / 通道信息等。
    pub semantic_kind: Option<String>,
}

/// 对外暴露的统一 Agent 单轮结果。
#[derive(Debug, Clone)]
pub struct KernelTurnResult {
    pub reply_text: String,
    pub tool_call: Option<(ToolCall, ToolResult)>,
}

/// 工具注册表 trait：负责为 LLM 暴露工具列表，并解析名称到执行句柄。
pub trait ToolRegistry {
    fn list_exposed(&self, cfg: &OpenpupConfig) -> Vec<ExposedTool>;
}

/// 工具执行器 trait：在给定配置下执行某个工具调用。
pub trait ToolExecutor {
    fn execute(&self, cfg: &OpenpupConfig, call: &ToolCall) -> ToolResult;
}

/// 记忆存储 trait：目前仅暴露语义记忆追加，后续可扩展。
pub trait MemoryStore {
    fn add_semantic_item(&self, kind: &str, content: &str, tags: Option<&str>) -> Result<()>;
}

/// Persona 提供者 trait：从 workspace 加载 persona 文本。
pub trait PersonaProvider {
    fn load_persona(&self) -> Result<String>;
}

/// 审计下沉 trait：记录运行时事件。
pub trait AuditSink {
    fn record(&self, event: &runtime_audit::RuntimeAuditEvent) -> Result<()>;
}

/// 默认 ToolRegistry：直接委托给现有 `tools::exposed_tools_from_config`。
pub struct DefaultToolRegistry;

impl ToolRegistry for DefaultToolRegistry {
    fn list_exposed(&self, cfg: &OpenpupConfig) -> Vec<ExposedTool> {
        tools::exposed_tools_from_config(cfg)
    }
}

/// 默认 ToolExecutor：委托给现有 `tools::execute_tool`，并注入 node_transport 供 InvokeNodeTool 使用。
pub struct DefaultToolExecutor(node_transport::HttpNodeTransport);

impl DefaultToolExecutor {
    pub fn new() -> Self {
        DefaultToolExecutor(node_transport::HttpNodeTransport)
    }
}

impl Default for DefaultToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolExecutor for DefaultToolExecutor {
    fn execute(&self, cfg: &OpenpupConfig, call: &ToolCall) -> ToolResult {
        tools::execute_tool(cfg, call, Some(&self.0))
    }
}

/// 默认 MemoryStore：委托给 `crate::core::memory::add_semantic_item`。
pub struct DefaultMemoryStore;

impl MemoryStore for DefaultMemoryStore {
    fn add_semantic_item(&self, kind: &str, content: &str, tags: Option<&str>) -> Result<()> {
        crate::core::memory::add_semantic_item(kind, content, tags)
    }
}

/// 默认 PersonaProvider：委托给 `persona::load_assembled_persona`。
pub struct DefaultPersonaProvider;

impl PersonaProvider for DefaultPersonaProvider {
    fn load_persona(&self) -> Result<String> {
        persona::load_assembled_persona()
    }
}

/// 默认 AuditSink：委托给 `runtime_audit::record`。
pub struct DefaultAuditSink;

impl AuditSink for DefaultAuditSink {
    fn record(&self, event: &runtime_audit::RuntimeAuditEvent) -> Result<()> {
        runtime_audit::record(event)
    }
}

/// AgentKernel 依赖的环境对象，便于在 CLI/守护进程中构建与注入不同实现。
/// executor 使用 Arc 以便在 async run_turn 中 clone 后传入闭包，避免自引用。
pub struct KernelEnv<R, E, M, P, A>
where
    R: ToolRegistry,
    E: ToolExecutor + Send + Sync,
    M: MemoryStore,
    P: PersonaProvider,
    A: AuditSink,
{
    pub cfg: OpenpupConfig,
    pub registry: R,
    pub executor: Arc<E>,
    pub memory: M,
    pub persona: P,
    pub audit: A,
}

impl
    KernelEnv<
        DefaultToolRegistry,
        DefaultToolExecutor,
        DefaultMemoryStore,
        DefaultPersonaProvider,
        DefaultAuditSink,
    >
{
    /// 构建一个使用全部默认实现的内核环境。
    pub fn new_default(cfg: OpenpupConfig) -> Self {
        KernelEnv {
            cfg,
            registry: DefaultToolRegistry,
            executor: Arc::new(DefaultToolExecutor::new()),
            memory: DefaultMemoryStore,
            persona: DefaultPersonaProvider,
            audit: DefaultAuditSink,
        }
    }
}

/// Agent 内核：统一封装单轮决策循环。
pub struct AgentKernel<R, E, M, P, A>
where
    R: ToolRegistry,
    E: ToolExecutor + Send + Sync,
    M: MemoryStore,
    P: PersonaProvider,
    A: AuditSink,
{
    env: KernelEnv<R, E, M, P, A>,
}

impl<R, E, M, P, A> AgentKernel<R, E, M, P, A>
where
    R: ToolRegistry,
    E: ToolExecutor + Send + Sync,
    M: MemoryStore,
    P: PersonaProvider,
    A: AuditSink,
{
    pub fn new(env: KernelEnv<R, E, M, P, A>) -> Self {
        AgentKernel { env }
    }

    /// 统一的单轮执行入口。
    pub async fn run_turn(&self, req: AgentRequest) -> Result<KernelTurnResult> {
        // 1. 准备 system prompt：Persona + 工具描述。
        let mut system = match self.env.persona.load_persona() {
            Ok(s) => s,
            Err(_) => String::from("# Persona\n\n(未找到 workspace/persona，使用空 persona。)"),
        };

        let exposed = self.env.registry.list_exposed(&self.env.cfg);
        if !exposed.is_empty() {
            let mut tools_section = String::new();
            tools_section.push_str(
                "You can optionally request calling local tools by replying with a JSON object on a single line:\n\
{\"tool\": \"name\", \"args\": {...}}\n\
Do not add explanation text around it when you want a tool call.\n\
Available tools:\n",
            );
            for t in &exposed {
                tools_section.push_str(&format!(
                    "- {} (level {}): {} args: {}\n",
                    t.name, t.level, t.description, t.args
                ));
            }
            let mode = self.env.cfg.autonomy.execution_mode.as_str();
            if mode != "readonly" {
                tools_section.push_str(
                    "- save_composite_tool (management, L2): {\"spec_toml\": string containing CompositeToolFile TOML}\n\
CompositeToolFile TOML must follow this schema: top-level `id` (string, required) as composite tool ID, used as `{id}.toml` filename and registry key; also include `name` / `description` / `steps`. When you create any composite tool TOML, you MUST include `id`, usually equal to `name` or a slug of it.\n",
                );
                tools_section.push_str(
                    "- register_sub_agent (management, L2): {\"name\": string, \"model\": optional, \"persona\": optional}\n",
                );
                tools_section
                    .push_str("- register_node (management, L2): {\"name\": string, \"host\": optional}\n");
                tools_section.push_str(
                    "- invoke_sub_agent (multi-agent, L2): {\"name\": string, \"input\": string}\n",
                );
                tools_section.push_str(
                    "- invoke_node_tool (multi-node, L2): {\"node\": string, \"tool\": string, \"args\": object}\n",
                );
            }
            tools_section.push('\n');
            system = format!(
                "You are openpup, a local agent.\n\n{}{}",
                tools_section, system
            );
        } else {
            system = format!("You are openpup, a local agent.\n\n{}", system);
        }

        // 2. LLM 配置与会话封装，工具执行经 self.env.executor 插槽注入，不直接依赖具体 tools。
        let llm_cfg = llm::load_openai_from_config(&self.env.cfg)?;
        let session = agent_runtime::AgentSession {
            session_id: req.session_id.clone(),
            system_prompt: system,
            exposed_tools: exposed,
            llm_cfg,
            cfg: self.env.cfg.clone(),
        };

        let semantic_kind = req.semantic_kind.as_deref();
        let exec = Arc::clone(&self.env.executor);
        let result = agent_runtime::run_single_turn(
            &session,
            &req.input,
            semantic_kind,
            5,
            move |c, call| exec.execute(c, call),
        )
        .await?;

        // 3. 记忆写入与审计经 trait 完成，再返回结果。
        let kind = req.semantic_kind.as_deref().unwrap_or("loop_log");
        let _ = self
            .env
            .memory
            .add_semantic_item(kind, &result.reply_text, Some(&req.session_id));
        let mut audit_ev = runtime_audit_core::new_event(
            runtime_audit_core::REALM_DEFAULT,
            runtime_audit_core::AGENT_CORE,
            "event",
            "agent_request",
            format!(
                "session={} reply_len={}",
                req.session_id,
                result.reply_text.len()
            ),
        );
        audit_ev.decision_summary = if result.reply_text.len() > 300 {
            format!(
                "{}...",
                result.reply_text.chars().take(300).collect::<String>()
            )
        } else {
            result.reply_text.clone()
        };
        audit_ev.result = runtime_audit_core::RuntimeAuditResult {
            status: "success".to_string(),
            error: None,
        };
        let _ = self.env.audit.record(&audit_ev);

        Ok(KernelTurnResult {
            reply_text: result.reply_text,
            tool_call: result.tool_call,
        })
    }
}

/// 便捷类型别名：全部使用默认实现的内核。
pub type DefaultKernel = AgentKernel<
    DefaultToolRegistry,
    DefaultToolExecutor,
    DefaultMemoryStore,
    DefaultPersonaProvider,
    DefaultAuditSink,
>;

impl DefaultKernel {
    /// 构建一个使用默认实现的内核实例。
    pub fn from_config(cfg: OpenpupConfig) -> Self {
        let env = KernelEnv::new_default(cfg);
        AgentKernel::new(env)
    }
}

// 同步 helper 已移除；请在上层创建 Tokio runtime，并使用 DefaultKernel::run_turn 进行异步调用。
