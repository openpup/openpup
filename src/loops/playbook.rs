use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::workspace;

/// 文件摘要型 Loop 的 Playbook 定义。
#[derive(Debug, Deserialize)]
pub struct FileLoopPlaybook {
    /// 友好名称，用于审计与输出说明。
    pub name: String,
    /// 相对于 base 的输入文件路径，如 "today_tasks.md" 或 "INVEST_LOG.md"。
    pub input: String,
    /// "workspace" | "logs"，决定从哪个根目录起拼接 input。
    #[serde(default = "default_base")]
    pub base: String,
    /// 写入本地长期语义记忆时使用的 kind（如 work_log / invest_log / life_log / loop_log）。
    #[serde(default)]
    pub semantic_kind: Option<String>,
    /// 若文件不存在或无有效内容时的提示。
    pub empty_message: String,
    /// 输出标题行。
    pub header: String,
    /// 输出尾注说明。
    pub footer: String,
    /// 行之间的连接符（仅在 max_lines 为 0 时使用）。
    #[serde(default)]
    pub join_separator: Option<String>,
    /// 若 > 0，则最多取最近的 max_lines 行；否则合并全部行。
    #[serde(default)]
    pub max_lines: Option<usize>,
    /// 可选：Loop 触发时传给 planner/agent 的目标描述（未来扩展用）。
    #[serde(default)]
    pub llm_goal: Option<String>,
}

fn default_base() -> String {
    "workspace".to_string()
}

fn playbook_path(loop_id: &str) -> Result<PathBuf> {
    let root = workspace::workspace_root()?;
    Ok(root.join("playbooks").join(format!("{loop_id}.toml")))
}

fn load_from_disk(loop_id: &str) -> Result<Option<FileLoopPlaybook>> {
    let path = playbook_path(loop_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&path)
        .with_context(|| format!("failed to read playbook at {:?}", path))?;
    let pb: FileLoopPlaybook =
        toml::from_str(&s).with_context(|| format!("failed to parse playbook {:?}", path))?;
    Ok(Some(pb))
}

/// 公共入口：加载某个 Loop 的 FileLoopPlaybook；若磁盘上不存在，则返回内置默认。
pub fn load_file_loop(loop_id: &str) -> Result<FileLoopPlaybook> {
    if let Some(pb) = load_from_disk(loop_id)? {
        return Ok(pb);
    }
    Ok(builtin_default(loop_id))
}

fn builtin_default(loop_id: &str) -> FileLoopPlaybook {
    match loop_id {
        "work_morning" => FileLoopPlaybook {
            name: "工作 – 早晨计划（只读）".to_string(),
            base: "workspace".to_string(),
            input: "today_tasks.md".to_string(),
            semantic_kind: Some("work_log".to_string()),
            empty_message: "（暂无今日任务，请编辑 workspace/today_tasks.md）".to_string(),
            header: "--- 今日计划（只读） ---".to_string(),
            footer: "(已写入 runtime-audit.log，source=work_morning)".to_string(),
            join_separator: Some("；".to_string()),
            max_lines: Some(0),
            llm_goal: None,
        },
        "work_plan_draft" => FileLoopPlaybook {
            name: "工作 – 今日计划草稿（草稿）".to_string(),
            base: "workspace".to_string(),
            input: "today_tasks.md".to_string(),
            semantic_kind: Some("work_log".to_string()),
            empty_message:
                "（暂无今日任务，请编辑 workspace/today_tasks.md，以便生成今日计划草稿）".to_string(),
            header: "--- 今日计划草稿（待你确认） ---".to_string(),
            footer: "(Phase 2：草稿 Loop，已写入 runtime-audit.log，source=work_plan_draft)".to_string(),
            // 草稿模式下保留全部非空行，由上层选择如何转化为具体执行项或对外输出。
            join_separator: Some("\n".to_string()),
            max_lines: Some(0),
            llm_goal: None,
        },
        "invest_morning" => FileLoopPlaybook {
            name: "投资 – 早盘简报（只读）".to_string(),
            base: "logs".to_string(),
            input: "INVEST_LOG.md".to_string(),
            semantic_kind: Some("invest_log".to_string()),
            empty_message: "（暂无投资日志，请编辑 workspace/logs/INVEST_LOG.md）".to_string(),
            header: "--- 早盘简报（只读） ---".to_string(),
            footer: "(已写入 runtime-audit.log，source=invest_morning)".to_string(),
            join_separator: None,
            max_lines: Some(20),
            llm_goal: None,
        },
        "invest_close" => FileLoopPlaybook {
            name: "投资 – 收盘复盘（只读）".to_string(),
            base: "logs".to_string(),
            input: "INVEST_LOG.md".to_string(),
            semantic_kind: Some("invest_log".to_string()),
            empty_message: "（暂无投资日志，请编辑 workspace/logs/INVEST_LOG.md）".to_string(),
            header: "--- 收盘复盘（只读） ---".to_string(),
            footer: "(已写入 runtime-audit.log，source=invest_close)".to_string(),
            join_separator: None,
            max_lines: Some(20),
            llm_goal: None,
        },
        "life_morning" => FileLoopPlaybook {
            name: "生活 – 早晨摘要（只读）".to_string(),
            base: "workspace".to_string(),
            input: "life_notes.md".to_string(),
            semantic_kind: Some("life_log".to_string()),
            empty_message: "（暂无生活笔记，请编辑 workspace/life_notes.md）".to_string(),
            header: "--- 生活摘要（早晨·只读） ---".to_string(),
            footer: "(已写入 runtime-audit.log，source=life_morning)".to_string(),
            join_separator: None,
            max_lines: Some(20),
            llm_goal: None,
        },
        "life_evening" => FileLoopPlaybook {
            name: "生活 – 晚间摘要（只读）".to_string(),
            base: "workspace".to_string(),
            input: "life_notes.md".to_string(),
            semantic_kind: Some("life_log".to_string()),
            empty_message: "（暂无生活笔记，请编辑 workspace/life_notes.md）".to_string(),
            header: "--- 生活摘要（晚间·只读） ---".to_string(),
            footer: "(已写入 runtime-audit.log，source=life_evening)".to_string(),
            join_separator: None,
            max_lines: Some(20),
            llm_goal: None,
        },
        _ => FileLoopPlaybook {
            name: loop_id.to_string(),
            base: "workspace".to_string(),
            input: "today_tasks.md".to_string(),
            semantic_kind: Some("loop_log".to_string()),
            empty_message: "（暂无内容）".to_string(),
            header: format!("--- {loop_id} ---"),
            footer: String::new(),
            join_separator: Some("；".to_string()),
            max_lines: Some(0),
            llm_goal: None,
        },
    }
}

