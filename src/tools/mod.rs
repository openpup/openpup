pub mod integrations;
pub mod net;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::OpenpupConfig;
use crate::core::llm;
use crate::core::memory;
use crate::core::registry;
use crate::core::runtime_audit;
use crate::core::workspace;
use crate::tools::integrations::{caldav, email_imap, home_assistant, market};
use anyhow::{Context, Result};
use std::fs::{self, create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use uuid::Uuid;

/// 统一的工具种类定义（只包含当前实现的只读 L1 工具 + 少量管理工具 + 组合工具）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolKind {
    /// 读取 Home Assistant 某个 entity 的状态。
    HomeAssistantGetState,
    /// 读取单个标的的日线行情（stooq）。
    MarketQuote,
    /// 从已配置的 RSS 源抓取新闻标题。
    NewsRssHeadlines,
    /// IMAP 未读邮件标题列表。
    EmailUnreadSubjects,
    /// CalDAV 事件列表（简要）。
    CaldavEventsToday,
    /// CalDAV 任务列表（简要）。
    CaldavTasks,
    /// 原语：执行 shell 命令（沙箱内，L2）。
    ShellExec,
    /// 原语：读取文件内容（workspace 内相对路径，L1）。
    FileRead,
    /// 原语：写入文件（workspace 内相对路径，L2）。
    FileWrite,
    /// 原语：发起 HTTP 请求（L2）。
    HttpRequest,
    /// 原语：联网搜索（L1，当前用 DuckDuckGo 摘要）。
    WebSearch,
    /// 原语：安全数学表达式计算（L1）。
    Calculator,
    /// 原语：存储一条长期记忆 k/v（L2）。
    MemoryStore,
    /// 原语：按查询检索长期记忆（L1）。
    MemoryRecall,
    /// 管理工具：保存一个组合工具 TOML 到 workspace/tools 目录。
    SaveCompositeTool,
    /// 管理工具：注册一个子 Agent（受 spawn.mode 约束）。
    RegisterSubAgent,
    /// 管理工具：注册一个 Worker 节点（受 spawn.mode 约束）。
    RegisterNode,
    /// 多 Agent：调用已注册的子 Agent 执行一轮对话，返回其回复（由 agent_runtime 执行）。
    InvokeSubAgent,
    /// 多 Node：在指定节点上执行某工具，通过 HTTP POST 到节点 /tool 接口。
    InvokeNodeTool,
    /// L3：将某条决策/总结写入本地 L3 日志（低风险，仅本机 JSONL）。
    L3LogDecision,
    /// L3：更新一个本地进度状态（小型 JSON 状态机，低风险）。
    L3UpdateProgress,
    /// L3：在本地维护一个简单 TODO 列表（追加条目）。
    L3AddTodo,
    /// L3：更新本地 TODO 条目的状态。
    L3UpdateTodoStatus,
    /// 组合工具：在 workspace/tools 下以 TOML 声明的一串步骤。
    Composite(String),
}

/// 统一的工具调用描述，供 planner/agent 使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub kind: ToolKind,
    /// 参数统一通过 JSON 承载，便于与 LLM/外部 planner 对接。
    pub args: Value,
}

/// 工具执行结果的统一包装。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub value: Option<Value>,
    pub error: Option<String>,
}

/// 对 Worker 节点的工具调用与具体传输协议解耦，由 kernel 实现，tools 仅通过此 trait 调用。
pub trait NodeTransport: Send + Sync {
    fn invoke_tool(
        &self,
        node: &crate::core::registry::NodeInfo,
        tool: &str,
        args: &Value,
    ) -> anyhow::Result<ToolResult>;
}

/// 将字符串名称映射为 ToolKind，供 LLM / planner 使用。
pub fn tool_kind_from_str(name: &str) -> Option<ToolKind> {
    match name {
        "home_assistant_get_state" | "ha_get_state" => Some(ToolKind::HomeAssistantGetState),
        "market_quote" | "market:quote" => Some(ToolKind::MarketQuote),
        "news_rss_headlines" | "news:rss-headlines" => Some(ToolKind::NewsRssHeadlines),
        "email_unread_subjects" | "email:unread-subjects" => Some(ToolKind::EmailUnreadSubjects),
        "caldav_events_today" | "caldav:events-today" => Some(ToolKind::CaldavEventsToday),
        "caldav_tasks" | "caldav:tasks" => Some(ToolKind::CaldavTasks),
        "shell_exec" | "shell" | "exec" => Some(ToolKind::ShellExec),
        "file_read" => Some(ToolKind::FileRead),
        "file_write" => Some(ToolKind::FileWrite),
        "http_request" | "http" => Some(ToolKind::HttpRequest),
        "web_search" => Some(ToolKind::WebSearch),
        "calculator" => Some(ToolKind::Calculator),
        "memory_store" => Some(ToolKind::MemoryStore),
        "memory_recall" => Some(ToolKind::MemoryRecall),
        "save_composite_tool" => Some(ToolKind::SaveCompositeTool),
        "register_sub_agent" | "spawn" => Some(ToolKind::RegisterSubAgent),
        "register_node" | "node_register" => Some(ToolKind::RegisterNode),
        "invoke_sub_agent" | "call_sub_agent" => Some(ToolKind::InvokeSubAgent),
        "invoke_node_tool" | "node_tool" => Some(ToolKind::InvokeNodeTool),
        "l3_log_decision" | "l3:log-decision" => Some(ToolKind::L3LogDecision),
        "l3_update_progress" | "l3:update-progress" => Some(ToolKind::L3UpdateProgress),
        "l3_add_todo" | "l3:add-todo" => Some(ToolKind::L3AddTodo),
        "l3_update_todo_status" | "l3:update-todo-status" => Some(ToolKind::L3UpdateTodoStatus),
        other => {
            // 尝试将未知名称解释为组合工具 id。
            if registry_get_composite(other).is_some() {
                Some(ToolKind::Composite(other.to_string()))
            } else {
                None
            }
        }
    }
}

/// 对外暴露给 agent/LLM 的工具描述（基于配置文件）。
#[derive(Debug, Clone)]
pub struct ExposedTool {
    pub name: String,
    pub description: String,
    pub level: String,
    pub args: String,
    pub kind: ToolKind,
}

fn map_id_to_kind(id: &str) -> Option<ToolKind> {
    match id {
        "home_assistant_get_state" => Some(ToolKind::HomeAssistantGetState),
        "market_quote" => Some(ToolKind::MarketQuote),
        "news_rss_headlines" => Some(ToolKind::NewsRssHeadlines),
        "email_unread_subjects" => Some(ToolKind::EmailUnreadSubjects),
        "caldav_events_today" => Some(ToolKind::CaldavEventsToday),
        "caldav_tasks" => Some(ToolKind::CaldavTasks),
        "shell_exec" => Some(ToolKind::ShellExec),
        "file_read" => Some(ToolKind::FileRead),
        "file_write" => Some(ToolKind::FileWrite),
        "http_request" => Some(ToolKind::HttpRequest),
        "web_search" => Some(ToolKind::WebSearch),
        "calculator" => Some(ToolKind::Calculator),
        "memory_store" => Some(ToolKind::MemoryStore),
        "memory_recall" => Some(ToolKind::MemoryRecall),
        "l3_log_decision" => Some(ToolKind::L3LogDecision),
        "l3_update_progress" => Some(ToolKind::L3UpdateProgress),
        "l3_add_todo" => Some(ToolKind::L3AddTodo),
        "l3_update_todo_status" => Some(ToolKind::L3UpdateTodoStatus),
        // 其他 id 可能对应组合工具，由组合工具加载逻辑处理。
        _ => None,
    }
}

/// 内建工具（硬编码）：所有已实现的工具及等级，不依赖 config。暴露时再按 execution_mode 过滤。
fn builtin_tools() -> Vec<ExposedTool> {
    vec![
        ExposedTool {
            name: "home_assistant_get_state".to_string(),
            description: "Read Home Assistant entity state".to_string(),
            level: "L1".to_string(),
            args: "{\"entity_id\": string}".to_string(),
            kind: ToolKind::HomeAssistantGetState,
        },
        ExposedTool {
            name: "market_quote".to_string(),
            description: "Get daily quote for a symbol".to_string(),
            level: "L1".to_string(),
            args: "{\"symbol\": string}".to_string(),
            kind: ToolKind::MarketQuote,
        },
        ExposedTool {
            name: "news_rss_headlines".to_string(),
            description: "Fetch RSS headlines from configured feeds".to_string(),
            level: "L1".to_string(),
            args: "{\"limit\": optional number}".to_string(),
            kind: ToolKind::NewsRssHeadlines,
        },
        ExposedTool {
            name: "email_unread_subjects".to_string(),
            description: "List unread email subjects".to_string(),
            level: "L1".to_string(),
            args: "{\"mailbox\": optional string, \"limit\": optional number}".to_string(),
            kind: ToolKind::EmailUnreadSubjects,
        },
        ExposedTool {
            name: "caldav_events_today".to_string(),
            description: "Get today's calendar events".to_string(),
            level: "L1".to_string(),
            args: "{\"limit\": optional number}".to_string(),
            kind: ToolKind::CaldavEventsToday,
        },
        ExposedTool {
            name: "caldav_tasks".to_string(),
            description: "Get calendar tasks".to_string(),
            level: "L1".to_string(),
            args: "{\"limit\": optional number}".to_string(),
            kind: ToolKind::CaldavTasks,
        },
        ExposedTool {
            name: "shell_exec".to_string(),
            description: "Execute shell command in workspace (sandbox)".to_string(),
            level: "L2".to_string(),
            args: "{\"command\": string, \"timeout_sec\": optional number, \"working_dir\": optional string}".to_string(),
            kind: ToolKind::ShellExec,
        },
        ExposedTool {
            name: "file_read".to_string(),
            description: "Read file contents (path relative to workspace)".to_string(),
            level: "L1".to_string(),
            args: "{\"path\": string, \"encoding\": optional string}".to_string(),
            kind: ToolKind::FileRead,
        },
        ExposedTool {
            name: "file_write".to_string(),
            description: "Write content to file (path relative to workspace)".to_string(),
            level: "L2".to_string(),
            args: "{\"path\": string, \"content\": string, \"mode\": optional \"overwrite\"|\"append\"}".to_string(),
            kind: ToolKind::FileWrite,
        },
        ExposedTool {
            name: "http_request".to_string(),
            description: "Make HTTP request (GET/POST/PUT/DELETE)".to_string(),
            level: "L2".to_string(),
            args: "{\"method\": string, \"url\": string, \"headers\": optional object, \"body\": optional string, \"timeout\": optional number}".to_string(),
            kind: ToolKind::HttpRequest,
        },
        ExposedTool {
            name: "web_search".to_string(),
            description: "Search the web and return summary (DuckDuckGo)".to_string(),
            level: "L1".to_string(),
            args: "{\"query\": string, \"num_results\": optional number}".to_string(),
            kind: ToolKind::WebSearch,
        },
        ExposedTool {
            name: "calculator".to_string(),
            description: "Evaluate math expression safely".to_string(),
            level: "L1".to_string(),
            args: "{\"expression\": string}".to_string(),
            kind: ToolKind::Calculator,
        },
        ExposedTool {
            name: "memory_store".to_string(),
            description: "Store a key-value in long-term memory".to_string(),
            level: "L2".to_string(),
            args: "{\"key\": string, \"value\": string, \"tags\": optional string}".to_string(),
            kind: ToolKind::MemoryStore,
        },
        ExposedTool {
            name: "memory_recall".to_string(),
            description: "Recall memories by query".to_string(),
            level: "L1".to_string(),
            args: "{\"query\": string, \"limit\": optional number}".to_string(),
            kind: ToolKind::MemoryRecall,
        },
        ExposedTool {
            name: "l3_log_decision".to_string(),
            description: "Append a decision/summary to local L3 log".to_string(),
            level: "L3".to_string(),
            args: "{\"summary\": string, \"details\": optional}".to_string(),
            kind: ToolKind::L3LogDecision,
        },
        ExposedTool {
            name: "l3_update_progress".to_string(),
            description: "Update a local progress key".to_string(),
            level: "L3".to_string(),
            args: "{\"key\": string, \"status\": string, \"meta\": optional}".to_string(),
            kind: ToolKind::L3UpdateProgress,
        },
        ExposedTool {
            name: "l3_add_todo".to_string(),
            description: "Add a local TODO item".to_string(),
            level: "L3".to_string(),
            args: "{\"title\": string, \"id\": optional, \"status\": optional, \"tags\": optional}".to_string(),
            kind: ToolKind::L3AddTodo,
        },
        ExposedTool {
            name: "l3_update_todo_status".to_string(),
            description: "Update local TODO status".to_string(),
            level: "L3".to_string(),
            args: "{\"id\": string, \"status\": string}".to_string(),
            kind: ToolKind::L3UpdateTodoStatus,
        },
    ]
}

/// 四个 execution_mode：readonly、draft-only、full、approval。
/// 按 execution_mode 决定某等级是否可暴露：readonly 仅 L1，draft-only 允许 L1/L2，full 允许 L1–L3，approval 允许 L1–L4。
fn level_exposed_for_mode(mode: &str, level: &str) -> bool {
    match mode {
        "readonly" => level == "L1",
        "draft-only" => level == "L1" || level == "L2",
        "full" => level == "L1" || level == "L2" || level == "L3",
        "approval" => level == "L1" || level == "L2" || level == "L3" || level == "L4",
        _ => level == "L1",
    }
}

/// 从配置构建一份对 LLM 可见的工具列表。
///
/// - **内建工具**（硬编码）：L1 只读 + L3 本地工具，始终作为底表；config [tools] 可覆盖同名项。
/// - **execution_mode**：只暴露当前模式允许的等级（readonly→L1，draft-only→L1/L2，full→L1–L3）。
/// - 管理/多节点工具（save_composite_tool、register_*、invoke_*）为 L2，在 draft-only/full 下暴露，由 kernel 在 system prompt 中统一描述。
/// - 最后叠加 workspace 组合工具（组合工具等级以声明为准，同样按 mode 过滤）。
pub fn exposed_tools_from_config(cfg: &OpenpupConfig) -> Vec<ExposedTool> {
    let mode = cfg.autonomy.execution_mode.as_str();
    let mut out: Vec<ExposedTool> = builtin_tools()
        .into_iter()
        .filter(|t| level_exposed_for_mode(mode, &t.level))
        .collect();

    if let Some(list) = cfg.tools.as_ref() {
        for t in list {
            if let Some(kind) = map_id_to_kind(t.id.as_str()) {
                let level = if t.level.is_empty() {
                    tool_level_for_kind(&kind).to_string()
                } else {
                    t.level.clone()
                };
                if !level_exposed_for_mode(mode, &level) {
                    continue;
                }
                let name = if t.name.is_empty() { t.id.clone() } else { t.name.clone() };
                if let Some(pos) = out.iter().position(|e| e.name == name || e.name == t.id) {
                    out[pos] = ExposedTool {
                        name,
                        description: t.description.clone(),
                        level,
                        args: t.args.clone(),
                        kind,
                    };
                } else {
                    out.push(ExposedTool {
                        name,
                        description: t.description.clone(),
                        level,
                        args: t.args.clone(),
                        kind,
                    });
                }
            }
        }
    }

    if let Ok(extra) = load_composite_tools() {
        for t in extra {
            if level_exposed_for_mode(mode, &t.level) {
                out.push(t);
            }
        }
    }

    out
}

/// 组合工具在 workspace 下的声明格式。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompositeToolFile {
    id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    steps: Vec<CompositeStep>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompositeStep {
    tool: String,
    #[serde(default)]
    args: Value,
}

use std::collections::HashMap;

/// 全局组合工具 registry：id -> CompositeToolFile。
/// 仅在进程内使用，不做跨进程共享。
static mut COMPOSITE_REGISTRY: Option<HashMap<String, CompositeToolFile>> = None;

#[allow(static_mut_refs)]
fn with_composite_registry<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, CompositeToolFile>) -> R,
{
    // 简单的全局可变状态，当前仅用于单进程 CLI/daemon 场景。
    unsafe {
        let reg = COMPOSITE_REGISTRY.get_or_insert_with(HashMap::new);
        f(reg)
    }
}

fn registry_get_composite(id: &str) -> Option<CompositeToolFile> {
    with_composite_registry(|reg| reg.get(id).cloned())
}

fn registry_insert_composite(ct: CompositeToolFile) {
    with_composite_registry(|reg| {
        reg.insert(ct.id.clone(), ct);
    });
}

/// 从 workspace/tools 目录中加载组合工具定义，映射到 ExposedTool 并写入 registry。
fn load_composite_tools() -> Result<Vec<ExposedTool>> {
    let mut out = Vec::new();
    let root = workspace::workspace_root()?;
    let dir = root.join("tools");
    if !dir.exists() {
        return Ok(out);
    }

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ext) = path.extension() {
            if ext != "toml" {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(ct) = load_single_composite(&path) {
            let id = ct.id.clone();
            registry_insert_composite(ct);
            out.push(ExposedTool {
                name: id.clone(),
                description: String::new(),
                level: "L1".to_string(),
                args: String::new(),
                kind: ToolKind::Composite(id),
            });
        }
    }

    Ok(out)
}

fn load_single_composite(path: &PathBuf) -> Result<CompositeToolFile> {
    let s = fs::read_to_string(path)?;
    let cf: CompositeToolFile = toml::from_str(&s)?;
    Ok(cf)
}

/// 写入或更新一个组合工具定义文件。
///
/// - `raw_toml` 必须能解析为 CompositeToolFile；
/// - 文件名采用 `{id}.toml`，其中 id 来自 TOML 内容；
/// - 若同名文件已存在，将被覆盖。
pub fn save_composite_tool_raw(raw_toml: &str) -> Result<PathBuf> {
    let cf: CompositeToolFile = toml::from_str(raw_toml)?;
    let root = workspace::workspace_root()?;
    let dir = root.join("tools");
    fs::create_dir_all(&dir)?;
    let filename = format!("{}.toml", cf.id);
    let path = dir.join(filename);
    fs::write(&path, raw_toml)?;
    // 热加载：在当前进程内立即更新组合工具 registry，方便后续同一轮调用。
    registry_insert_composite(cf);
    Ok(path)
}

fn tool_level_for_kind(kind: &ToolKind) -> &'static str {
    match kind {
        ToolKind::ShellExec
        | ToolKind::FileWrite
        | ToolKind::HttpRequest
        | ToolKind::MemoryStore
        | ToolKind::SaveCompositeTool
        | ToolKind::RegisterSubAgent
        | ToolKind::RegisterNode
        | ToolKind::InvokeSubAgent
        | ToolKind::InvokeNodeTool => "L2",
        ToolKind::L3LogDecision
        | ToolKind::L3UpdateProgress
        | ToolKind::L3AddTodo
        | ToolKind::L3UpdateTodoStatus => "L3",
        _ => "L1",
    }
}

fn tool_name_for_kind(kind: &ToolKind) -> String {
    match kind {
        ToolKind::HomeAssistantGetState => "home_assistant_get_state".to_string(),
        ToolKind::MarketQuote => "market_quote".to_string(),
        ToolKind::NewsRssHeadlines => "news_rss_headlines".to_string(),
        ToolKind::EmailUnreadSubjects => "email_unread_subjects".to_string(),
        ToolKind::CaldavEventsToday => "caldav_events_today".to_string(),
        ToolKind::CaldavTasks => "caldav_tasks".to_string(),
        ToolKind::ShellExec => "shell_exec".to_string(),
        ToolKind::FileRead => "file_read".to_string(),
        ToolKind::FileWrite => "file_write".to_string(),
        ToolKind::HttpRequest => "http_request".to_string(),
        ToolKind::WebSearch => "web_search".to_string(),
        ToolKind::Calculator => "calculator".to_string(),
        ToolKind::MemoryStore => "memory_store".to_string(),
        ToolKind::MemoryRecall => "memory_recall".to_string(),
        ToolKind::SaveCompositeTool => "save_composite_tool".to_string(),
        ToolKind::RegisterSubAgent => "register_sub_agent".to_string(),
        ToolKind::RegisterNode => "register_node".to_string(),
        ToolKind::InvokeSubAgent => "invoke_sub_agent".to_string(),
        ToolKind::InvokeNodeTool => "invoke_node_tool".to_string(),
        ToolKind::L3LogDecision => "l3_log_decision".to_string(),
        ToolKind::L3UpdateProgress => "l3_update_progress".to_string(),
        ToolKind::L3AddTodo => "l3_add_todo".to_string(),
        ToolKind::L3UpdateTodoStatus => "l3_update_todo_status".to_string(),
        ToolKind::Composite(id) => format!("composite:{}", id),
    }
}

fn level_allowed(mode: &str, level: &str) -> bool {
    // L4 仅在 execution_mode = "approval" 时允许执行。
    if level == "L4" {
        return mode == "approval";
    }
    // L3 仅在 full / approval 时允许执行。
    if level == "L3" {
        return mode == "full" || mode == "approval";
    }
    match mode {
        "readonly" => level == "L1",
        "draft-only" => level == "L1" || level == "L2",
        "full" | "approval" => true,
        _ => level == "L1",
    }
}

/// 错误信息前缀，用于 CLI 判断是否为「安全矩阵拒绝」。
const SAFETY_DENIAL_ERROR_PREFIX: &str = "execution_mode ";

/// 判断工具执行失败是否因 execution_mode × 工具等级矩阵拒绝。
pub fn is_safety_denial_error(err: Option<&str>) -> bool {
    err.map(|e| e.contains(SAFETY_DENIAL_ERROR_PREFIX) && e.contains("does not allow"))
        .unwrap_or(false)
}

// ============== 安全审查官：按单次调用的参数评估「真实等级」，再与 execution_mode 矩阵比对 ==============

/// 安全审查官：根据本次调用的参数得出该次调用的**真实风险等级**（effective level）。
/// 工具声明等级（如 shell_exec 声明为 L2）仅表示默认；审查官可依据 args 将本次调用判定为更高等级（L3/L4），
/// 再与 execution_mode 矩阵决定是否放行。
pub fn effective_tool_level(call: &ToolCall) -> &'static str {
    match &call.kind {
        ToolKind::ShellExec => {
            let cmd = call
                .args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            effective_level_for_shell_command(cmd)
        }
        _ => tool_level_for_kind(&call.kind),
    }
}

/// 对 shell_exec 的 command 做风险分级：返回本次执行应视为的等级（L2/L3/L4）。
fn effective_level_for_shell_command(cmd: &str) -> &'static str {
    println!("effective_level_for_shell_command: {}", cmd);
    if cmd.is_empty() {
        return "L2";
    }
    let lower = cmd.to_lowercase();

    // L4：明显高危，仅 approval 模式可执行
    let l4_patterns: &[&str] = &[
        "rm -rf /",
        "rm -rf /*",
        "sudo ",
        " > /etc",
        ">>/etc",
        "|/bin/sh",
        "| sh ",
        "|bash ",
        ":(){",
        "mkfs.",
        "dd if=",
        "chmod 777 /",
        "chmod 4755",
        "> /dev/sd",
        ">/dev/sd",
    ];
    if l4_patterns.iter().any(|p| lower.contains(p)) {
        return "L4";
    }

    // L3：中危（网络拉取并执行、后台常驻、写系统路径等），full 可执行
    let l3_patterns: &[&str] = &[
        "curl ",
        "wget ",
        " | sh",
        " | bash",
        "nohup ",
        " &",
        ">/tmp/",
        ">>/tmp/",
        ">/var/",
        ">/usr/",
    ];
    if l3_patterns.iter().any(|p| lower.contains(p)) {
        return "L3";
    }

    // L2：低危（只读或仅写 workspace 内、简单命令）
    "L2"
}

/// 执行时使用的有效等级：若开启 LLM 增强且为 shell_exec，且当前**未**在 tokio runtime 内，则用内建 security_reviewer 判定；否则用规则（避免在 async 上下文中创建 runtime 导致 panic）。
fn effective_tool_level_for_execution(cfg: &OpenpupConfig, call: &ToolCall) -> String {
    if cfg.autonomy.use_llm_security_review != Some(true) {
        return effective_tool_level(call).to_string();
    }
    if let ToolKind::ShellExec = &call.kind {
        if tokio::runtime::Handle::try_current().is_ok() {
            return effective_tool_level(call).to_string();
        }
        let cmd = call
            .args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if let Ok(llm_cfg) = llm::load_openai_from_config(cfg) {
            if let Ok(reply) =
                llm::complete_as_builtin_role_blocking(&llm_cfg, llm::ROLE_SECURITY_REVIEWER, cmd)
            {
                if let Some(level) = llm::parse_security_level_from_review(&reply) {
                    return level;
                }
            }
        }
    }
    effective_tool_level(call).to_string()
}

// ============== 以上为安全审查官 ==============

/// 统一的同步执行器：在给定配置下执行某个工具。
///
/// - 不打印到 stdout，由上层决定如何渲染；
/// - 按 execution_mode 与工具 level 做最小权限控制：readonly 仅 L1，draft-only 允许 L1/L2，full 不限制。
/// - 被拒绝时写入 runtime-audit.log 的 safety_denied 事件。
/// - `node_transport`：调用 Worker 节点工具时使用，由 kernel 注入；若为 None 且请求为 InvokeNodeTool 则返回错误。
fn workspace_relative_path(rel: &str) -> Result<PathBuf> {
    let root = workspace::workspace_root()?;
    let root = root.canonicalize().unwrap_or_else(|_| root.clone());
    let path = root.join(rel);
    let normalized = path.components().fold(PathBuf::new(), |mut acc, c| {
        match c {
            std::path::Component::ParentDir => {
                if acc.as_os_str().is_empty() {
                    acc.push("..");
                } else {
                    acc.pop();
                }
            }
            std::path::Component::CurDir => {}
            _ => acc.push(c),
        }
        acc
    });
    let resolved = root.join(&normalized);
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        anyhow::bail!("path escapes workspace")
    }
}

pub fn execute_tool(
    cfg: &OpenpupConfig,
    call: &ToolCall,
    node_transport: Option<&dyn NodeTransport>,
) -> ToolResult {
    let mode = cfg.autonomy.execution_mode.as_str();
    let declared_level = tool_level_for_kind(&call.kind);
    let effective_level = effective_tool_level_for_execution(cfg, call);
    if !level_allowed(mode, &effective_level) {
        let tool_id = tool_name_for_kind(&call.kind);
        let err_msg = format!(
            "execution_mode {:?} does not allow effective tool level {} (declared {}); tool_id={}",
            mode, effective_level, declared_level, tool_id
        );
        let mut ev = runtime_audit::new_event(
            runtime_audit::REALM_DEFAULT,
            runtime_audit::AGENT_CORE,
            "tool",
            "safety_denied",
            format!(
                "Tool execution denied by security reviewer: mode={} effective_level={} tool_id={}",
                mode, effective_level, tool_id
            ),
        );
        ev.tools.push(runtime_audit::RuntimeAuditToolCall {
            name: tool_id.clone(),
            level: effective_level.clone(),
            args_digest: Some(format!("declared={},effective={}", declared_level, effective_level)),
        });
        ev.result = runtime_audit::RuntimeAuditResult {
            status: "denied".to_string(),
            error: Some(err_msg.clone()),
        };
        ev.risk = runtime_audit::RuntimeAuditRisk::default();
        let _ = runtime_audit::record(&ev);

        return ToolResult {
            ok: false,
            value: None,
            error: Some(err_msg),
        };
    }

    match &call.kind {
        ToolKind::SaveCompositeTool => {
            let spec = call
                .args
                .get("spec_toml")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if spec.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("spec_toml is required".to_string()),
                };
            }
            match save_composite_tool_raw(spec) {
                Ok(path) => ToolResult {
                    ok: true,
                    value: Some(serde_json::json!({ "path": path })),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::RegisterSubAgent => {
            if cfg.autonomy.spawn.mode == "disabled" {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("spawn.mode is disabled; cannot register sub-agent from agent. Set autonomy.spawn.mode in config to allow.".to_string()),
                };
            }
            let name = call
                .args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.name is required".to_string()),
                };
            }
            let spec = registry::SubAgentSpec {
                name,
                model: call
                    .args
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                persona: call
                    .args
                    .get("persona")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };
            match registry::register_sub_agent(spec) {
                Ok(()) => {
                    let mut ev = runtime_audit::new_event(
                        runtime_audit::REALM_DEFAULT,
                        runtime_audit::AGENT_CORE,
                        "agent",
                        "register_sub_agent",
                        "Register sub-agent via tool",
                    );
                    ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                        name: "register_sub_agent".to_string(),
                        level: "L2".to_string(),
                        args_digest: Some("name-only".to_string()),
                    });
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    ev.risk = runtime_audit::RuntimeAuditRisk {
                        spawn: Some(runtime_audit::SpawnRisk {
                            attempted: true,
                            host_digest: None,
                        }),
                        ..Default::default()
                    };
                    let _ = runtime_audit::record(&ev);

                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "registered": true })),
                        error: None,
                    }
                }
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::RegisterNode => {
            if cfg.autonomy.spawn.mode == "disabled" {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("spawn.mode is disabled; cannot register node from agent. Set autonomy.spawn.mode in config to allow.".to_string()),
                };
            }
            let name = call
                .args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.name is required".to_string()),
                };
            }
            let info = registry::NodeInfo {
                name,
                host: call
                    .args
                    .get("host")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                tags: Vec::new(),
                last_seen_ts: memory::now_unix_ts(),
                status: "registered".to_string(),
            };
            match registry::register_node(info) {
                Ok(()) => {
                    let mut ev = runtime_audit::new_event(
                        runtime_audit::REALM_DEFAULT,
                        runtime_audit::AGENT_CORE,
                        "agent",
                        "register_node",
                        "Register worker node via tool",
                    );
                    ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                        name: "register_node".to_string(),
                        level: "L2".to_string(),
                        args_digest: Some("name+host".to_string()),
                    });
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    ev.risk = runtime_audit::RuntimeAuditRisk {
                        spawn: Some(runtime_audit::SpawnRisk {
                            attempted: true,
                            host_digest: None,
                        }),
                        ..Default::default()
                    };
                    let _ = runtime_audit::record(&ev);

                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "registered": true })),
                        error: None,
                    }
                }
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::InvokeSubAgent => {
            // 实际执行在 agent_runtime 中完成，此处不应被调用；若被调用则返回明确错误。
            ToolResult {
                ok: false,
                value: None,
                error: Some(
                    "invoke_sub_agent must be dispatched by agent runtime (internal)".to_string(),
                ),
            }
        }
        ToolKind::InvokeNodeTool => {
            let node_transport = match node_transport {
                Some(t) => t,
                None => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(
                            "invoke_node_tool requires node transport (kernel); not available in this context".to_string(),
                        ),
                    };
                }
            };
            let node_name = call
                .args
                .get("node")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let tool_name = call
                .args
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let tool_args = call
                .args
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if node_name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.node is required (node name)".to_string()),
                };
            }
            if tool_name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.tool is required (tool name to run on node)".to_string()),
                };
            }
            let nodes_file = match registry::load_nodes() {
                Ok(f) => f,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let node = match nodes_file.nodes.get(node_name) {
                Some(n) => n.clone(),
                None => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(format!("node {:?} not found in registry", node_name)),
                    };
                }
            };
            let res = match node_transport.invoke_tool(&node, tool_name, &tool_args) {
                Ok(res) => res,
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "invoke_node_tool",
                format!("Invoke tool {} on node {}", tool_name, node_name),
            );
            ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                name: "invoke_node_tool".to_string(),
                level: "L2".to_string(),
                args_digest: Some(format!("node={},tool={}", node_name, tool_name)),
            });
            ev.result = runtime_audit::RuntimeAuditResult {
                status: if res.ok { "success" } else { "error" }.to_string(),
                error: res.error.clone(),
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::L3LogDecision => {
            let summary = call
                .args
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let details = call.args.get("details").cloned();
            if summary.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.summary is required for l3_log_decision".to_string()),
                };
            }

            let res = append_l3_decision_log(&summary, details.clone());
            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "l3_auto_exec",
                format!("L3 auto decision logged: {}", summary),
            );
            ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                name: "l3_log_decision".to_string(),
                level: "L3".to_string(),
                args_digest: Some(format!(
                    "summary_len={},details={}",
                    summary.len(),
                    details.is_some()
                )),
            });
            ev.risk = runtime_audit::RuntimeAuditRisk::default();

            match res {
                Ok(path) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "logged": true, "path": path })),
                        error: None,
                    }
                }
                Err(e) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    }
                }
            }
        }
        ToolKind::L3UpdateProgress => {
            let key = call
                .args
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let status = call
                .args
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let meta = call.args.get("meta").cloned();
            if key.is_empty() || status.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some(
                        "args.key and args.status are required for l3_update_progress".to_string(),
                    ),
                };
            }

            let res = update_l3_progress(&key, &status, meta.clone());
            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "l3_auto_exec",
                format!("L3 auto progress update: {} => {}", key, status),
            );
            ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                name: "l3_update_progress".to_string(),
                level: "L3".to_string(),
                args_digest: Some(format!(
                    "key={},status_len={},meta={}",
                    key,
                    status.len(),
                    meta.is_some()
                )),
            });
            ev.risk = runtime_audit::RuntimeAuditRisk::default();

            match res {
                Ok(path) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "updated": true, "path": path })),
                        error: None,
                    }
                }
                Err(e) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    }
                }
            }
        }
        ToolKind::L3AddTodo => {
            let title = call
                .args
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if title.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.title is required for l3_add_todo".to_string()),
                };
            }
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let status = call
                .args
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .trim()
                .to_string();
            let tags: Vec<String> = call
                .args
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            let res = add_l3_todo(&id, &title, &status, tags.clone());
            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "l3_auto_exec",
                format!("L3 add local todo: {} ({})", title, id),
            );
            ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                name: "l3_add_todo".to_string(),
                level: "L3".to_string(),
                args_digest: Some(format!("id={},status={},tags={}", id, status, tags.len())),
            });
            ev.risk = runtime_audit::RuntimeAuditRisk::default();

            match res {
                Ok(path) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "id": id, "path": path })),
                        error: None,
                    }
                }
                Err(e) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    }
                }
            }
        }
        ToolKind::L3UpdateTodoStatus => {
            let id = call
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let status = call
                .args
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() || status.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some(
                        "args.id and args.status are required for l3_update_todo_status"
                            .to_string(),
                    ),
                };
            }

            let res = update_l3_todo_status(&id, &status);
            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "l3_auto_exec",
                format!("L3 update local todo status: {} => {}", id, status),
            );
            ev.tools.push(runtime_audit::RuntimeAuditToolCall {
                name: "l3_update_todo_status".to_string(),
                level: "L3".to_string(),
                args_digest: Some(format!("id={},status={}", id, status)),
            });
            ev.risk = runtime_audit::RuntimeAuditRisk::default();

            match res {
                Ok(path) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "success".to_string(),
                        error: None,
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "id": id, "path": path })),
                        error: None,
                    }
                }
                Err(e) => {
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    let _ = runtime_audit::record(&ev);
                    ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    }
                }
            }
        }
        ToolKind::ShellExec => {
            let command = call
                .args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if command.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.command is required for shell_exec".to_string()),
                };
            }
            let _timeout_sec = call
                .args
                .get("timeout_sec")
                .and_then(|v| v.as_u64())
                .unwrap_or(60);
            let cwd = match workspace::workspace_root() {
                Ok(r) => r,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let output = Command::new("sh")
                .arg("-c")
                .arg(&command)
                .current_dir(&cwd)
                .output();
            match output {
                Ok(o) => ToolResult {
                    ok: true,
                    value: Some(serde_json::json!({
                        "stdout": String::from_utf8_lossy(&o.stdout),
                        "stderr": String::from_utf8_lossy(&o.stderr),
                        "exit_code": o.status.code().unwrap_or(-1)
                    })),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::FileRead => {
            let path_arg = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path_arg.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.path is required for file_read".to_string()),
                };
            }
            match workspace_relative_path(path_arg) {
                Ok(p) => match fs::read_to_string(&p) {
                    Ok(s) => ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "content": s })),
                        error: None,
                    },
                    Err(e) => ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    },
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::FileWrite => {
            let path_arg = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = call
                .args
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path_arg.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.path is required for file_write".to_string()),
                };
            }
            let mode = call
                .args
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("overwrite");
            match workspace_relative_path(path_arg) {
                Ok(p) => {
                    if let Some(parent) = p.parent() {
                        let _ = create_dir_all(parent);
                    }
                    let open_result = if mode == "append" {
                        OpenOptions::new().write(true).append(true).create(true).open(&p)
                    } else {
                        OpenOptions::new()
                            .write(true)
                            .truncate(true)
                            .create(true)
                            .open(&p)
                    };
                    match open_result.and_then(|mut f| f.write_all(content.as_bytes())) {
                        Ok(()) => ToolResult {
                            ok: true,
                            value: Some(serde_json::json!({ "path": p.to_string_lossy() })),
                            error: None,
                        },
                        Err(e) => ToolResult {
                            ok: false,
                            value: None,
                            error: Some(e.to_string()),
                        },
                    }
                }
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::HttpRequest => {
            let method = call
                .args
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET")
                .to_uppercase();
            let url = call
                .args
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.url is required for http_request".to_string()),
                };
            }
            let timeout_sec = call
                .args
                .get("timeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);
            let res: Result<serde_json::Value> = crate::tools::net::block_on_async(async {
                let proxy = std::env::var("OPENPUP_PROXY").ok();
                let mut builder = reqwest::Client::builder()
                    .timeout(Duration::from_secs(timeout_sec));
                if let Some(p) = proxy {
                    builder = builder.proxy(reqwest::Proxy::all(p)?);
                }
                let client = builder.build()?;

                let mut req = match method.as_str() {
                    "GET" => client.get(url),
                    "POST" => client.post(url),
                    "PUT" => client.put(url),
                    "DELETE" => client.delete(url),
                    _ => anyhow::bail!("unsupported method: {}", method),
                };

                if let Some(h) = call.args.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in h {
                        if let Some(s) = v.as_str() {
                            req = req.header(k.as_str(), s);
                        }
                    }
                }
                if let Some(body) = call.args.get("body").and_then(|v| v.as_str()) {
                    req = req.body(body.to_string());
                }

                let resp = req.send().await?;
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                Ok(serde_json::json!({ "status": status, "body": body }))
            });

            match res {
                Ok(v) => ToolResult {
                    ok: true,
                    value: Some(v),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::WebSearch => {
            let query = call
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if query.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.query is required for web_search".to_string()),
                };
            }
            let url = format!(
                "https://api.duckduckgo.com/?q={}&format=json",
                urlencoding::encode(query)
            );
            let res: Result<serde_json::Value> = crate::tools::net::block_on_async(async {
                let proxy = std::env::var("OPENPUP_PROXY").ok();
                let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(30));
                if let Some(p) = proxy {
                    builder = builder.proxy(reqwest::Proxy::all(p)?);
                }
                let client = builder.build()?;
                let resp = client.get(&url).send().await?;
                let text = resp.text().await.unwrap_or_default();
                let v: Option<serde_json::Value> = serde_json::from_str(&text).ok();
                let abstract_text = v
                    .as_ref()
                    .and_then(|o| o.get("AbstractText"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let abstract_url = v
                    .as_ref()
                    .and_then(|o| o.get("AbstractURL"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(serde_json::json!({
                    "summary": abstract_text,
                    "url": abstract_url,
                    "raw": if abstract_text.is_empty() { text } else { String::new() }
                }))
            });

            match res {
                Ok(v) => ToolResult {
                    ok: true,
                    value: Some(v),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::Calculator => {
            let expression = call
                .args
                .get("expression")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if expression.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.expression is required for calculator".to_string()),
                };
            }
            match evalexpr::eval_number(expression) {
                Ok(n) => ToolResult {
                    ok: true,
                    value: Some(serde_json::json!({ "result": n })),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::MemoryStore => {
            let key = call
                .args
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let value = call
                .args
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if key.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.key is required for memory_store".to_string()),
                };
            }
            let _tags = call
                .args
                .get("tags")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match memory::add_semantic_item("memory", &value, Some(&key)) {
                Ok(()) => ToolResult {
                    ok: true,
                    value: Some(serde_json::json!({ "stored": true, "key": key })),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::MemoryRecall => {
            let query = call
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = call
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as usize;
            match memory::search_semantic_items(Some("memory"), query, limit) {
                Ok(items) => {
                    let arr: Vec<serde_json::Value> = items
                        .into_iter()
                        .map(|it| {
                            serde_json::json!({
                                "key": it.tags.unwrap_or_default(),
                                "content": it.content,
                                "created_ts": it.created_ts
                            })
                        })
                        .collect();
                    ToolResult {
                        ok: true,
                        value: Some(serde_json::json!({ "items": arr })),
                        error: None,
                    }
                }
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            }
        }
        ToolKind::Composite(id) => {
            // 展开组合工具：按 steps 顺序依次调用底层工具。
            let ct = match registry_get_composite(id) {
                Some(c) => c,
                None => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(format!("composite tool {} not found", id)),
                    };
                }
            };
            let mut results: Vec<Value> = Vec::new();
            // 简单防御：限制最大步骤数，避免过长或递归爆炸。
            let max_steps = 16usize;
            for (idx, step) in ct.steps.into_iter().enumerate() {
                if idx >= max_steps {
                    break;
                }
                let kind = match tool_kind_from_str(step.tool.as_str()) {
                    Some(k) => k,
                    None => {
                        return ToolResult {
                            ok: false,
                            value: None,
                            error: Some(format!("unknown tool in composite step: {}", step.tool)),
                        };
                    }
                };
                // 防止组合工具自我引用导致的简单无限递归。
                if matches!(kind, ToolKind::Composite(ref other_id) if other_id == id) {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(format!("composite tool {} cannot reference itself", id)),
                    };
                }
                let sub_call = ToolCall {
                    kind,
                    args: step.args,
                };
                let sub_res = execute_tool(cfg, &sub_call, node_transport);
                if !sub_res.ok {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(format!(
                            "composite tool {} step {} failed: {:?}",
                            id,
                            idx + 1,
                            sub_res.error
                        )),
                    };
                }
                if let Some(v) = sub_res.value {
                    results.push(v);
                }
            }

            ToolResult {
                ok: true,
                value: Some(Value::Array(results)),
                error: None,
            }
        }
        ToolKind::HomeAssistantGetState => {
            let entity_id = call
                .args
                .get("entity_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if entity_id.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("entity_id is required".to_string()),
                };
            }
            let ha_cfg = match home_assistant::get_home_assistant_config(cfg) {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let res = match home_assistant::get_state(entity_id, &ha_cfg) {
                Ok(v) => ToolResult {
                    ok: true,
                    value: Some(v),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_home_assistant_get_state",
                format!("Read Home Assistant state for {}.", entity_id),
            );
            ev.tools.push(runtime_audit::tool_call(
                "home_assistant_get_state",
                Some(entity_id.to_string()),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: if res.ok {
                    "success".to_string()
                } else {
                    "error".to_string()
                },
                error: res.error.clone(),
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::MarketQuote => {
            let symbol = call
                .args
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if symbol.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("symbol is required".to_string()),
                };
            }
            let provider = cfg
                .integrations
                .as_ref()
                .and_then(|i| i.market.as_ref())
                .map(|m| m.provider.as_str())
                .unwrap_or("stooq");
            if provider != "stooq" {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some(format!(
                        "unsupported provider {} (only stooq is implemented)",
                        provider
                    )),
                };
            }
            let res = match market::stooq_quote_daily(symbol) {
                Ok(v) => ToolResult {
                    ok: true,
                    value: Some(v),
                    error: None,
                },
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_market_quote",
                format!("Read market quote for {}.", symbol),
            );
            ev.tools.push(runtime_audit::tool_call(
                "market_quote",
                Some(symbol.to_string()),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: if res.ok {
                    "success".to_string()
                } else {
                    "error".to_string()
                },
                error: res.error.clone(),
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::NewsRssHeadlines => {
            let limit = call
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(5)
                .min(20) as usize;
            let feeds = cfg
                .integrations
                .as_ref()
                .and_then(|i| i.news_rss.as_ref())
                .map(|n| n.feeds.clone());
            let feeds = match feeds {
                Some(f) if !f.is_empty() => f,
                _ => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(
                            "news_rss is not configured. Run `openpup add-tool news-rss`."
                                .to_string(),
                        ),
                    };
                }
            };
            let mut all = Vec::new();
            for feed in &feeds {
                match market::rss_headlines(feed, limit) {
                    Ok(items) => {
                        all.push(Value::Array(items));
                    }
                    Err(e) => {
                        let mut ev = runtime_audit::new_event(
                            runtime_audit::REALM_DEFAULT,
                            runtime_audit::AGENT_CORE,
                            "agent",
                            "tool_news_rss_headlines",
                            format!("Fetch RSS headlines failed for {}", feed),
                        );
                        ev.tools.push(runtime_audit::tool_call(
                            "news_rss_headlines",
                            Some(format!("feed={},limit={}", feed, limit)),
                        ));
                        ev.result = runtime_audit::RuntimeAuditResult {
                            status: "error".to_string(),
                            error: Some(e.to_string()),
                        };
                        ev.risk = runtime_audit::RuntimeAuditRisk::default();
                        let _ = runtime_audit::record(&ev);

                        return ToolResult {
                            ok: false,
                            value: None,
                            error: Some(format!("RSS error for {}: {}", feed, e)),
                        };
                    }
                }
            }

            let res = ToolResult {
                ok: true,
                value: Some(Value::Array(all)),
                error: None,
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_news_rss_headlines",
                format!("Fetch RSS headlines ({} feeds).", feeds.len()),
            );
            ev.tools.push(runtime_audit::tool_call(
                "news_rss_headlines",
                Some(format!("feeds={},limit={}", feeds.len(), limit)),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: "success".to_string(),
                error: None,
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::EmailUnreadSubjects => {
            let mailbox = call
                .args
                .get("mailbox")
                .and_then(|v| v.as_str())
                .unwrap_or("INBOX");
            let limit = call
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .min(50) as usize;
            let imap_cfg = match email_imap::get_imap_config(cfg) {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let res = match email_imap::unread_envelopes(&imap_cfg, mailbox, limit) {
                Ok(items) => {
                    let mapped: Vec<Value> = items
                        .into_iter()
                        .map(|(subj, from, date)| {
                            serde_json::json!({
                                "subject": subj,
                                "from": from,
                                "date": date,
                            })
                        })
                        .collect();
                    ToolResult {
                        ok: true,
                        value: Some(Value::Array(mapped)),
                        error: None,
                    }
                }
                Err(e) => ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                },
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_email_unread_subjects",
                format!("List unread email subjects in {}.", mailbox),
            );
            ev.tools.push(runtime_audit::tool_call(
                "email_imap_unseen_envelope",
                Some(format!("mailbox={},limit={}", mailbox, limit)),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: if res.ok {
                    "success".to_string()
                } else {
                    "error".to_string()
                },
                error: res.error.clone(),
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::CaldavEventsToday => {
            let limit = call
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .min(50) as usize;
            let cal_cfg = match caldav::get_caldav_config(cfg) {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let blobs = match caldav::fetch_ics_blobs(&cal_cfg, 3) {
                Ok(b) => b,
                Err(e) => {
                    let mut ev = runtime_audit::new_event(
                        runtime_audit::REALM_DEFAULT,
                        runtime_audit::AGENT_CORE,
                        "agent",
                        "tool_caldav_events_today",
                        "Fetch CalDAV events failed (fetch_ics_blobs).",
                    );
                    ev.tools.push(runtime_audit::tool_call(
                        "caldav_get_ics_events",
                        Some(format!("limit={}", limit)),
                    ));
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    ev.risk = runtime_audit::RuntimeAuditRisk::default();
                    let _ = runtime_audit::record(&ev);

                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let mut all = Vec::new();
            for b in &blobs {
                match caldav::parse_events(b, limit) {
                    Ok(events) => {
                        all.extend(events);
                    }
                    Err(e) => {
                        let mut ev = runtime_audit::new_event(
                            runtime_audit::REALM_DEFAULT,
                            runtime_audit::AGENT_CORE,
                            "agent",
                            "tool_caldav_events_today",
                            "Fetch CalDAV events failed (parse_events).",
                        );
                        ev.tools.push(runtime_audit::tool_call(
                            "caldav_get_ics_events",
                            Some(format!("limit={}", limit)),
                        ));
                        ev.result = runtime_audit::RuntimeAuditResult {
                            status: "error".to_string(),
                            error: Some(e.to_string()),
                        };
                        ev.risk = runtime_audit::RuntimeAuditRisk::default();
                        let _ = runtime_audit::record(&ev);

                        return ToolResult {
                            ok: false,
                            value: None,
                            error: Some(e.to_string()),
                        };
                    }
                }
            }

            let res = ToolResult {
                ok: true,
                value: Some(Value::Array(all)),
                error: None,
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_caldav_events_today",
                "Fetch CalDAV events (basic GET+parse).",
            );
            ev.tools.push(runtime_audit::tool_call(
                "caldav_get_ics_events",
                Some(format!("limit={}", limit)),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: "success".to_string(),
                error: None,
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
        ToolKind::CaldavTasks => {
            let limit = call
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .min(50) as usize;
            let cal_cfg = match caldav::get_caldav_config(cfg) {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let blobs = match caldav::fetch_ics_blobs(&cal_cfg, 3) {
                Ok(b) => b,
                Err(e) => {
                    let mut ev = runtime_audit::new_event(
                        runtime_audit::REALM_DEFAULT,
                        runtime_audit::AGENT_CORE,
                        "agent",
                        "tool_caldav_tasks",
                        "Fetch CalDAV tasks failed (fetch_ics_blobs).",
                    );
                    ev.tools.push(runtime_audit::tool_call(
                        "caldav_get_ics_tasks",
                        Some(format!("limit={}", limit)),
                    ));
                    ev.result = runtime_audit::RuntimeAuditResult {
                        status: "error".to_string(),
                        error: Some(e.to_string()),
                    };
                    ev.risk = runtime_audit::RuntimeAuditRisk::default();
                    let _ = runtime_audit::record(&ev);

                    return ToolResult {
                        ok: false,
                        value: None,
                        error: Some(e.to_string()),
                    };
                }
            };
            let mut all = Vec::new();
            for b in &blobs {
                match caldav::parse_tasks(b, limit) {
                    Ok(tasks) => {
                        all.extend(tasks);
                    }
                    Err(e) => {
                        let mut ev = runtime_audit::new_event(
                            runtime_audit::REALM_DEFAULT,
                            runtime_audit::AGENT_CORE,
                            "agent",
                            "tool_caldav_tasks",
                            "Fetch CalDAV tasks failed (parse_tasks).",
                        );
                        ev.tools.push(runtime_audit::tool_call(
                            "caldav_get_ics_tasks",
                            Some(format!("limit={}", limit)),
                        ));
                        ev.result = runtime_audit::RuntimeAuditResult {
                            status: "error".to_string(),
                            error: Some(e.to_string()),
                        };
                        ev.risk = runtime_audit::RuntimeAuditRisk::default();
                        let _ = runtime_audit::record(&ev);

                        return ToolResult {
                            ok: false,
                            value: None,
                            error: Some(e.to_string()),
                        };
                    }
                }
            }

            let res = ToolResult {
                ok: true,
                value: Some(Value::Array(all)),
                error: None,
            };

            let mut ev = runtime_audit::new_event(
                runtime_audit::REALM_DEFAULT,
                runtime_audit::AGENT_CORE,
                "agent",
                "tool_caldav_tasks",
                "Fetch CalDAV tasks (basic GET+parse).",
            );
            ev.tools.push(runtime_audit::tool_call(
                "caldav_get_ics_tasks",
                Some(format!("limit={}", limit)),
            ));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: "success".to_string(),
                error: None,
            };
            ev.risk = runtime_audit::RuntimeAuditRisk::default();
            let _ = runtime_audit::record(&ev);

            res
        }
    }
}

fn l3_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("failed to locate home directory")?;
    let dir = home.join(".openpup");
    create_dir_all(&dir).with_context(|| format!("failed to create L3 dir {:?}", dir))?;
    Ok(dir)
}

fn append_l3_decision_log(summary: &str, details: Option<Value>) -> Result<PathBuf> {
    let dir = l3_dir()?;
    let path = dir.join("l3-decisions.log");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open L3 decision log at {:?}", path))?;
    let ts = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let line = serde_json::json!({
        "ts": ts,
        "summary": summary,
        "details": details
    });
    let s = serde_json::to_string(&line).context("failed to serialize L3 decision line")?;
    writeln!(file, "{s}").context("failed to write L3 decision line")?;
    Ok(path)
}

fn update_l3_progress(key: &str, status: &str, meta: Option<Value>) -> Result<PathBuf> {
    let dir = l3_dir()?;
    let path = dir.join("l3-progress.json");
    let mut current: serde_json::Map<String, Value> = if path.exists() {
        let s = fs::read_to_string(&path)
            .with_context(|| format!("failed to read L3 progress file at {:?}", path))?;
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };
    let mut entry = serde_json::Map::new();
    entry.insert("status".to_string(), Value::String(status.to_string()));
    if let Some(m) = meta {
        entry.insert("meta".to_string(), m);
    }
    current.insert(key.to_string(), Value::Object(entry));
    let s =
        serde_json::to_string_pretty(&current).context("failed to serialize L3 progress map")?;
    fs::write(&path, s)
        .with_context(|| format!("failed to write L3 progress file at {:?}", path))?;
    Ok(path)
}

fn l3_todo_path() -> Result<PathBuf> {
    let dir = l3_dir()?;
    Ok(dir.join("l3-todos.json"))
}

fn add_l3_todo(id: &str, title: &str, status: &str, tags: Vec<String>) -> Result<PathBuf> {
    let path = l3_todo_path()?;
    let mut todos: Vec<Value> = if path.exists() {
        let s = fs::read_to_string(&path)
            .with_context(|| format!("failed to read L3 todo file at {:?}", path))?;
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ts = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let todo = serde_json::json!({
        "id": id,
        "title": title,
        "status": status,
        "tags": tags,
        "created_ts": ts,
        "updated_ts": ts,
    });
    todos.push(todo);
    let s = serde_json::to_string_pretty(&todos).context("failed to serialize L3 todos")?;
    fs::write(&path, s).with_context(|| format!("failed to write L3 todo file at {:?}", path))?;
    Ok(path)
}

fn update_l3_todo_status(id: &str, status: &str) -> Result<PathBuf> {
    let path = l3_todo_path()?;
    let mut todos: Vec<Value> = if path.exists() {
        let s = fs::read_to_string(&path)
            .with_context(|| format!("failed to read L3 todo file at {:?}", path))?;
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ts = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    for t in &mut todos {
        if t.get("id").and_then(|v| v.as_str()) == Some(id) {
            if let Some(obj) = t.as_object_mut() {
                obj.insert("status".to_string(), Value::String(status.to_string()));
                obj.insert("updated_ts".to_string(), Value::String(ts.clone()));
            }
            break;
        }
    }
    let s = serde_json::to_string_pretty(&todos).context("failed to serialize L3 todos")?;
    fs::write(&path, s).with_context(|| format!("failed to write L3 todo file at {:?}", path))?;
    Ok(path)
}
