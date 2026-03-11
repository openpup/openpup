//! Agent 引擎：单轮对话、工具调用与交互式 REPL，供 CLI / runtime 复用。
//! 多 Agent：invoke_sub_agent 在本模块内执行（run_sub_agent_turn），避免 tools 与 agent_runtime 循环依赖。

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::OpenpupConfig;
use crate::llm::{self, OpenpupLlmConfig};
use crate::core::registry;
use crate::tools::{self, ExposedTool, ToolCall, ToolResult};

/// 单次会话的配置与状态（无状态：每轮只读配置，不持久化多轮历史）。
/// 工具执行由调用方通过 run_single_turn 的 execute_tool 参数注入，不再在此持有 node_transport。
#[derive(Clone)]
pub struct AgentSession {
    pub session_id: String,
    pub system_prompt: String,
    pub exposed_tools: Vec<ExposedTool>,
    pub llm_cfg: OpenpupLlmConfig,
    pub cfg: OpenpupConfig,
}

/// 单轮结果：回复文本 + 若有工具调用则带调用与结果。
#[derive(Debug)]
pub struct AgentTurnResult {
    pub reply_text: String,
    pub tool_call: Option<(ToolCall, ToolResult)>,
}

/// 解析 LLM 返回的 JSON tool 调用。
/// 支持三种形态：
/// - 整条回复就是 {"tool": "...", "args": {...}}
/// - 回复中嵌入单行 JSON（如前后有自然语言）
/// - 回复中嵌入多行/缩进 JSON（如 markdown 代码块内）
pub fn parse_tool_call(text: &str, exposed: &[ExposedTool]) -> Option<ToolCall> {
    fn value_to_call(v: &Value, exposed: &[ExposedTool]) -> Option<ToolCall> {
        let tool_name = v.get("tool")?.as_str()?;
        let args = v
            .get("args")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        if tool_name == "save_composite_tool" {
            return Some(ToolCall {
                kind: tools::ToolKind::SaveCompositeTool,
                args,
            });
        }
        if tool_name == "register_sub_agent" || tool_name == "spawn" {
            return Some(ToolCall {
                kind: tools::ToolKind::RegisterSubAgent,
                args,
            });
        }
        if tool_name == "register_node" || tool_name == "node_register" {
            return Some(ToolCall {
                kind: tools::ToolKind::RegisterNode,
                args,
            });
        }
        if tool_name == "invoke_sub_agent" || tool_name == "call_sub_agent" {
            return Some(ToolCall {
                kind: tools::ToolKind::InvokeSubAgent,
                args,
            });
        }
        if tool_name == "invoke_node_tool" || tool_name == "node_tool" {
            return Some(ToolCall {
                kind: tools::ToolKind::InvokeNodeTool,
                args,
            });
        }

        let exposed_tool = exposed.iter().find(|t| t.name == tool_name)?;
        Some(ToolCall {
            kind: exposed_tool.kind.clone(),
            args,
        })
    }

    let trimmed = text.trim();

    // 1) 优先尝试把整个回复当成 JSON 解析（兼容原先“整条就是 {\"tool\": ...}” 的约定）。
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() {
            if let Some(call) = value_to_call(&v, exposed) {
                return Some(call);
            }
        }
    }

    // 2) 回退：在整段文本中寻找第一个完整的 JSON 对象（支持多行与缩进）。
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut depth = 0i32;
            let mut j = i;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            // 尝试解析 [i..=j] 这一段为 JSON 对象。
                            if let Ok(slice) = std::str::from_utf8(&bytes[i..=j]) {
                                if let Ok(v) = serde_json::from_str::<Value>(slice) {
                                    if v.is_object() {
                                        if let Some(call) = value_to_call(&v, exposed) {
                                            return Some(call);
                                        }
                                    }
                                }
                            }
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        i += 1;
    }

    None
}

/// 在本地同步执行一次子 Agent 单轮对话，返回其回复作为 ToolResult（供主 Agent 使用）。
pub async fn run_sub_agent_turn(
    cfg: &OpenpupConfig,
    agent_name: &str,
    input: &str,
) -> Result<ToolResult> {
    let agents_file = registry::load_agents().context("load agents registry")?;
    let spec = agents_file
        .agents
        .get(agent_name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("sub-agent {:?} not found in registry", agent_name))?;
    let system_body = spec
        .persona
        .as_deref()
        .unwrap_or("(no persona)");
    let system_prompt = format!("You are {}.\n\n{}", spec.name, system_body);
    let exposed_tools = tools::exposed_tools_from_config(cfg);
    let mut llm_cfg = llm::load_openai_from_config(cfg).context("load LLM config for sub-agent")?;
    if let Some(m) = &spec.model {
        llm_cfg.model = m.clone();
    }
    let session = AgentSession {
        session_id: format!("sub:{}", agent_name),
        system_prompt,
        exposed_tools,
        llm_cfg,
        cfg: cfg.clone(),
    };
    let default_executor = |c: &OpenpupConfig, call: &ToolCall| tools::execute_tool(c, call, None);
    let result = Box::pin(run_single_turn(&session, input, None, 5, default_executor)).await?;
    Ok(ToolResult {
        ok: true,
        value: Some(Value::String(result.reply_text)),
        error: None,
    })
}

/// 单轮：用户输入 -> LLM（带记忆）-> 若为 tool 调用则执行并再调 LLM 总结，返回最终回复与可选的 (call, result)。
/// 工具执行通过 `execute_tool` 注入，由 kernel 或调用方提供，决策循环不直接依赖具体 tools 实现。
pub async fn run_single_turn<F>(
    session: &AgentSession,
    user_input: &str,
    semantic_kind: Option<&str>,
    memory_limit: usize,
    execute_tool: F,
) -> Result<AgentTurnResult>
where
    F: Fn(&OpenpupConfig, &ToolCall) -> ToolResult,
{
    let text = llm::openai_complete_with_memory(
        &session.llm_cfg,
        &session.system_prompt,
        user_input,
        semantic_kind,
        memory_limit,
    )
    .await?;

    if let Some(call) = parse_tool_call(&text, &session.exposed_tools) {
        let tool_res = if matches!(call.kind, tools::ToolKind::InvokeSubAgent) {
            let name = call
                .args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let input = call
                .args
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            run_sub_agent_turn(&session.cfg, name, input)
                .await
                .unwrap_or_else(|e| ToolResult {
                    ok: false,
                    value: None,
                    error: Some(e.to_string()),
                })
        } else {
            execute_tool(&session.cfg, &call)
        };
        let tool_ctx = format!(
            "Original request:\n{}\n\nTool call: {:?}\nTool result: {:?}\n",
            user_input, call, tool_res
        );
        let final_reply = llm::openai_complete_with_memory(
            &session.llm_cfg,
            &session.system_prompt,
            &tool_ctx,
            semantic_kind,
            3,
        )
        .await?;
        return Ok(AgentTurnResult {
            reply_text: final_reply,
            tool_call: Some((call, tool_res)),
        });
    }

    Ok(AgentTurnResult {
        reply_text: text,
        tool_call: None,
    })
}

/// 交互式 REPL：循环读行、调用 run_single_turn、打印回复；由 CLI 在同步上下文中 block_on 使用。
/// `execute_tool` 由调用方注入（如默认实现可用 `|c, call| tools::execute_tool(c, call, None)`）。
pub async fn run_interactive_repl<F>(session: &AgentSession, execute_tool: F) -> Result<()>
where
    F: Fn(&OpenpupConfig, &ToolCall) -> ToolResult + Copy,
{
    use std::io::{self, Write};

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let semantic_kind = Some("loop_log");
    let memory_limit = 5usize;

    loop {
        print!("you> ");
        stdout.flush().ok();
        let mut line = String::new();
        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match run_single_turn(session, line, semantic_kind, memory_limit, execute_tool).await {
            Ok(result) => {
                if let Some((call, res)) = &result.tool_call {
                    println!("pup (tool request): {:?} -> {:?}", call.kind, res);
                }
                println!("pup> {}\n", result.reply_text.trim());
            }
            Err(e) => {
                eprintln!("pup (error): {:#}", e);
            }
        }
    }
    Ok(())
}

