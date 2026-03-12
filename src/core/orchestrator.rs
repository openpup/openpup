//! Planner-Executor 多代理编排引擎（最小可用）。

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use crate::config::OpenpupConfig;
use crate::core::agent_runtime;
use crate::core::gateway::events::GatewayToClient;
use crate::core::kernel::{AgentRequest, DefaultKernel};
use crate::core::registry;

#[derive(Debug, Clone)]
pub struct OrchestrationRun {
    pub run_id: String,
    pub goal: String,
    pub agents: Vec<String>,
}

/// 运行一次 Planner-Executor 编排。
///
/// - Planner：使用主 Kernel 产出 JSON plan（steps）。
/// - Executor：逐步调用子 agent 单轮对话。
/// - Summarizer：主 Kernel 汇总输出。
///
/// `emit` 用于将进度事件发送到网关/日志；如不需要可传入空闭包。
pub async fn run_planner_executor<F>(
    cfg: &OpenpupConfig,
    session_id: &str,
    goal: &str,
    agents: Vec<String>,
    mut emit: F,
) -> Result<(OrchestrationRun, String)>
where
    F: FnMut(GatewayToClient) + Send,
{
    let run = OrchestrationRun {
        run_id: Uuid::new_v4().to_string(),
        goal: goal.to_string(),
        agents,
    };

    let resolved_agents = if run.agents.is_empty() {
        registry::list_sub_agents()?
            .into_iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
    } else {
        run.agents.clone()
    };
    if resolved_agents.is_empty() {
        return Err(anyhow!(
            "no sub-agents available (register via `openpup spawn <name>` first)"
        ));
    }
    if cfg.autonomy.spawn.mode == "disabled" {
        return Err(anyhow!(
            "spawn.mode is disabled; orchestration requires sub-agents. Set autonomy.spawn.mode to allow."
        ));
    }

    let kernel = DefaultKernel::from_config(cfg.clone());

    // 1) Planner: produce a JSON plan.
    let planner_input = format!(
        "You are the Planner.\n\
Goal: {goal}\n\n\
Available executors (sub-agents): {agents}\n\n\
Return ONLY valid JSON (no markdown) in this schema:\n\
{{\"steps\":[{{\"agent\":\"<name>\",\"input\":\"<task>\",\"expected_output\":\"<brief>\"}}]}}\n\n\
Rules:\n\
- Use only agent names from the list.\n\
- Keep steps <= 6.\n",
        goal = goal,
        agents = resolved_agents.join(", ")
    );

    let plan_req = AgentRequest {
        session_id: session_id.to_string(),
        input: planner_input,
        semantic_kind: Some("orchestration".to_string()),
    };
    eprintln!(
        "openpup orchestrator: planning start session_id={} goal={}",
        session_id, goal
    );
    let plan_turn = match kernel.run_turn(plan_req).await {
        Ok(turn) => {
            eprintln!(
                "openpup orchestrator: planning done session_id={} run_id={} reply_len={}",
                session_id,
                run.run_id,
                turn.reply_text.len()
            );
            turn
        }
        Err(e) => {
            eprintln!(
                "openpup orchestrator: planning error session_id={} run_id={} error={e:#}",
                session_id, run.run_id
            );
            return Err(e);
        }
    };

    let plan_json: Value = serde_json::from_str(plan_turn.reply_text.trim()).unwrap_or_else(|_| {
        // fallback: one step per agent with same goal
        Value::Object(
            [(
                "steps".to_string(),
                Value::Array(
                    resolved_agents
                        .iter()
                        .map(|a| {
                            serde_json::json!({
                                "agent": a,
                                "input": goal,
                                "expected_output": "useful partial output"
                            })
                        })
                        .collect(),
                ),
            )]
            .into_iter()
            .collect(),
        )
    });

    emit(GatewayToClient::OrchestrationPlan {
        run_id: run.run_id.clone(),
        goal: run.goal.clone(),
        plan: plan_json.clone(),
    });

    let steps = plan_json
        .get("steps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 2) Execute steps.
    let mut outputs: Vec<Value> = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        let agent = step
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let input = step
            .get("input")
            .and_then(|v| v.as_str())
            .unwrap_or(goal)
            .to_string();

        eprintln!(
            "openpup orchestrator: step_start run_id={} step_idx={} agent={} input_len={}",
            run.run_id,
            idx,
            agent,
            input.len()
        );
        emit(GatewayToClient::OrchestrationStepStarted {
            run_id: run.run_id.clone(),
            step_idx: idx,
            agent: agent.clone(),
            input: input.clone(),
        });

        let tool_res = agent_runtime::run_sub_agent_turn(cfg, &agent, &input).await;
        let (ok, output) = match tool_res {
            Ok(r) => {
                let v = r.value.unwrap_or(Value::Null);
                (r.ok, v)
            }
            Err(e) => {
                eprintln!(
                    "openpup orchestrator: step_error run_id={} step_idx={} agent={} error={e:#}",
                    run.run_id, idx, agent
                );
                (false, Value::String(format!("{e:#}")))
            }
        };

        emit(GatewayToClient::OrchestrationStepFinished {
            run_id: run.run_id.clone(),
            step_idx: idx,
            agent: agent.clone(),
            ok,
            output: output.clone(),
        });

        outputs.push(serde_json::json!({
            "step_idx": idx,
            "agent": agent,
            "input": input,
            "ok": ok,
            "output": output
        }));
    }

    // 3) Summarize.
    let summarize_input = format!(
        "You are the Summarizer.\n\
Goal: {goal}\n\n\
Here are execution outputs as JSON array:\n{outputs}\n\n\
Produce a concise final answer for the user (plain text).",
        goal = goal,
        outputs = Value::Array(outputs).to_string()
    );

    let sum_req = AgentRequest {
        session_id: session_id.to_string(),
        input: summarize_input,
        semantic_kind: Some("orchestration".to_string()),
    };
    let sum_turn = kernel.run_turn(sum_req).await?;

    emit(GatewayToClient::OrchestrationFinished {
        run_id: run.run_id.clone(),
        ok: true,
        summary: sum_turn.reply_text.clone(),
    });

    Ok((run, sum_turn.reply_text))
}
