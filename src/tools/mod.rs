pub mod integrations;
pub mod net;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{OpenpupConfig, ToolExposeConfig};
use crate::core::memory;
use crate::core::runtime_audit;
use crate::core::registry;
use crate::core::workspace;
use crate::tools::integrations::{caldav, email_imap, home_assistant, market};
use anyhow::{Context, Result};
use std::fs::{self, create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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
        "l3_log_decision" => Some(ToolKind::L3LogDecision),
        "l3_update_progress" => Some(ToolKind::L3UpdateProgress),
        "l3_add_todo" => Some(ToolKind::L3AddTodo),
        "l3_update_todo_status" => Some(ToolKind::L3UpdateTodoStatus),
        // 其他 id 可能对应组合工具，由组合工具加载逻辑处理。
        _ => None,
    }
}

/// 从配置构建一份对 LLM 可见的工具列表。
///
/// - 若 cfg.tools 为空，则返回空列表（agent 将不暴露任何工具）。
/// - 无法映射到已知 ToolKind 的条目会被忽略。
pub fn exposed_tools_from_config(cfg: &OpenpupConfig) -> Vec<ExposedTool> {
    let mut out = Vec::new();
    let list: &[ToolExposeConfig] = match cfg.tools.as_ref() {
        Some(v) => v.as_slice(),
        None => return out,
    };

    for t in list {
        if let Some(kind) = map_id_to_kind(t.id.as_str()) {
            out.push(ExposedTool {
                name: t.name.clone(),
                description: t.description.clone(),
                level: if t.level.is_empty() {
                    "L1".to_string()
                } else {
                    t.level.clone()
                },
                args: t.args.clone(),
                kind,
            });
        }
    }

    // 叠加 workspace 下声明的组合工具。
    if let Ok(extra) = load_composite_tools() {
        out.extend(extra);
    }

    out
}

/// 组合工具在 workspace 下的声明格式。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompositeToolFile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    level: String,
    #[serde(default)]
    args: String,
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
        ToolKind::SaveCompositeTool
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

fn level_allowed(mode: &str, level: &str) -> bool {
    // tools-autonomy-safety：L3 仅在 execution_mode = "full" 时允许执行。
    if level == "L3" {
        return mode == "full";
    }
    match mode {
        "readonly" => level == "L1",
        "draft-only" => level == "L1" || level == "L2",
        "full" => true,
        _ => level == "L1",
    }
}

/// 统一的同步执行器：在给定配置下执行某个工具。
///
/// - 不打印到 stdout，由上层决定如何渲染；
/// - 按 execution_mode 与工具 level 做最小权限控制：readonly 仅 L1，draft-only 允许 L1/L2，full 不限制。
/// - `node_transport`：调用 Worker 节点工具时使用，由 kernel 注入；若为 None 且请求为 InvokeNodeTool 则返回错误。
pub fn execute_tool(
    cfg: &OpenpupConfig,
    call: &ToolCall,
    node_transport: Option<&dyn NodeTransport>,
) -> ToolResult {
    let mode = cfg.autonomy.execution_mode.as_str();
    let level = tool_level_for_kind(&call.kind);
    if !level_allowed(mode, level) {
        return ToolResult {
            ok: false,
            value: None,
            error: Some(format!(
                "execution_mode {:?} does not allow tool level {}",
                mode, level
            )),
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
            let name = call.args.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            if name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.name is required".to_string()),
                };
            }
            let spec = registry::SubAgentSpec {
                name,
                model: call.args.get("model").and_then(|v| v.as_str()).map(String::from),
                persona: call.args.get("persona").and_then(|v| v.as_str()).map(String::from),
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
            let name = call.args.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            if name.is_empty() {
                return ToolResult {
                    ok: false,
                    value: None,
                    error: Some("args.name is required".to_string()),
                };
            }
            let info = registry::NodeInfo {
                name,
                host: call.args.get("host").and_then(|v| v.as_str()).map(String::from),
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
                error: Some("invoke_sub_agent must be dispatched by agent runtime (internal)".to_string()),
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
            let node_name = call.args.get("node").and_then(|v| v.as_str()).unwrap_or_default();
            let tool_name = call.args.get("tool").and_then(|v| v.as_str()).unwrap_or_default();
            let tool_args = call.args.get("args").cloned().unwrap_or_else(|| serde_json::json!({}));
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
                args_digest: Some(format!("summary_len={},details={}", summary.len(), details.is_some())),
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
                    error: Some("args.key and args.status are required for l3_update_progress".to_string()),
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
                args_digest: Some(format!("key={},status_len={},meta={}", key, status.len(), meta.is_some())),
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
                        "args.id and args.status are required for l3_update_todo_status".to_string(),
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
                            id, idx + 1, sub_res.error
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
                status: if res.ok { "success".to_string() } else { "error".to_string() },
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
            ev.tools
                .push(runtime_audit::tool_call("market_quote", Some(symbol.to_string())));
            ev.result = runtime_audit::RuntimeAuditResult {
                status: if res.ok { "success".to_string() } else { "error".to_string() },
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
                            "news_rss is not configured. Run `openpup add-tool news-rss`.".to_string(),
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
                status: if res.ok { "success".to_string() } else { "error".to_string() },
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
    let s = serde_json::to_string_pretty(&current)
        .context("failed to serialize L3 progress map")?;
    fs::write(&path, s).with_context(|| format!("failed to write L3 progress file at {:?}", path))?;
    Ok(path)
}

fn l3_todo_path() -> Result<PathBuf> {
    let dir = l3_dir()?;
    Ok(dir.join("l3-todos.json"))
}

fn add_l3_todo(
    id: &str,
    title: &str,
    status: &str,
    tags: Vec<String>,
) -> Result<PathBuf> {
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


