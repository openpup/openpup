use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

/// 默认 Realm 与 Agent 命名常量，便于在全局保持一致。
pub const REALM_DEFAULT: &str = "default";
pub const AGENT_CORE: &str = "core";

/// 运行时审计事件，记录 openpup 在自主模式下做过的决策与动作。
#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeAuditEvent {
    /// 唯一事件 ID，便于跨日志/通知引用。
    pub id: String,
    /// ISO8601 时间戳（UTC）。
    pub ts: String,
    /// 所属 Realm，例如公司/部门/王朝标识。
    pub realm: String,
    /// 发起该行为的代理标识（core / worker-1 等）。
    pub agent: String,
    /// 触发源（cron / event / manual 等）。
    pub source: String,
    /// 触发的具体事件类型（market_signal / home_sensor / dm_message 等）。
    pub trigger_kind: String,
    /// 高层目标与计划摘要（脱敏后的自然语言或结构化摘要）。
    pub decision_summary: String,
    /// 涉及到的工具调用（名称、等级、参数摘要）。
    pub tools: Vec<RuntimeAuditToolCall>,
    /// 执行结果。
    pub result: RuntimeAuditResult,
    /// 风险标记（是否涉及金融/Spawn 等高危路径）。
    pub risk: RuntimeAuditRisk,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeAuditToolCall {
    pub name: String,
    pub level: String, // "L1" | "L2" | "L3" | "L4"
    /// 参数的脱敏摘要或哈希，而不是完整明文。
    pub args_digest: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeAuditResult {
    pub status: String, // "success" | "error" | "skipped"
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RuntimeAuditRisk {
    pub finance: Option<FinanceRisk>,
    pub spawn: Option<SpawnRisk>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct FinanceRisk {
    pub real_money_touched: bool,
    pub paper_trade: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SpawnRisk {
    pub attempted: bool,
    pub host_digest: Option<String>,
}

/// 将一条运行时审计事件写入 `~/.openpup/runtime-audit.log`（JSONL）。
pub fn record(event: &RuntimeAuditEvent) -> Result<()> {
    let home = home_dir().context("failed to locate home directory")?;
    let dir = home.join(".openpup");
    let log_path = dir.join("runtime-audit.log");

    create_dir_all(&dir).with_context(|| format!("failed to create audit dir {:?}", dir))?;

    let line = serde_json::to_string(event).context("failed to serialize runtime audit event")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open runtime audit log at {:?}", log_path))?;

    writeln!(file, "{line}").context("failed to write runtime audit log line")?;
    Ok(())
}

/// 便捷构造一个带基础字段的事件骨架，供上层填充。
pub fn new_event(
    realm: impl Into<String>,
    agent: impl Into<String>,
    source: impl Into<String>,
    trigger_kind: impl Into<String>,
    decision_summary: impl Into<String>,
) -> RuntimeAuditEvent {
    let ts = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    RuntimeAuditEvent {
        id: Uuid::new_v4().to_string(),
        ts,
        realm: realm.into(),
        agent: agent.into(),
        source: source.into(),
        trigger_kind: trigger_kind.into(),
        decision_summary: decision_summary.into(),
        tools: Vec::new(),
        result: RuntimeAuditResult {
            status: "pending".to_string(),
            error: None,
        },
        risk: RuntimeAuditRisk::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_realm_and_agent_are_non_empty() {
        assert!(!REALM_DEFAULT.is_empty());
        assert!(!AGENT_CORE.is_empty());
    }
}

/// 简易工具元数据表：集中维护各工具的默认等级，避免在调用处手写 "L1"/"L2"。
fn default_tool_level(name: &str) -> &'static str {
    match name {
        // Home Assistant 只读
        "home_assistant_get_state" => "L1",
        // 行情与资讯只读
        "market_quote" => "L1",
        "news_rss_headlines" => "L1",
        // 邮件与日历只读
        "email_imap_unseen_envelope" => "L1",
        "caldav_get_ics_events" => "L1",
        "caldav_get_ics_tasks" => "L1",
        // 文件读取型 Loop
        "read_file" => "L1",
        // 其他未登记工具默认视为 L1（保守策略，只读/低权限）
        _ => "L1",
    }
}

/// 通过统一入口构造工具调用审计记录，自动填充 level。
pub fn tool_call(name: &str, args_digest: Option<String>) -> RuntimeAuditToolCall {
    RuntimeAuditToolCall {
        name: name.to_string(),
        level: default_tool_level(name).to_string(),
        args_digest,
    }
}
