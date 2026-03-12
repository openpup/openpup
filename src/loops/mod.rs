//! 只读/草稿 Loop：工作早晨计划、投资简报等。每次执行均写入 runtime_audit。

use anyhow::Result;
use std::fs;

use crate::memory;
use crate::runtime_audit::{self, RuntimeAuditResult, RuntimeAuditRisk};
use crate::workspace;

pub mod playbook;

use self::playbook::FileLoopPlaybook;

/// 单次 file-loop 执行结果，供 runtime / 测试复用。
#[derive(Debug, Clone)]
pub struct FileLoopResult {
    pub loop_id: String,
    pub name: String,
    pub summary: String,
    pub header: String,
    pub footer: String,
}

/// PlaybookEngine 抽象：后续可扩展为更复杂的 Loop/Playbook 执行器。
pub trait PlaybookEngine {
    fn run(&self, loop_id: &str) -> Result<FileLoopResult>;
}

/// 默认实现：基于本地 FileLoopPlaybook 的简单引擎。
pub struct FileLoopEngine;

impl PlaybookEngine for FileLoopEngine {
    fn run(&self, loop_id: &str) -> Result<FileLoopResult> {
        run_file_loop(loop_id)
    }
}

/// 纯函数：执行 file-loop 逻辑，返回结果；不打印、不写 audit（由调用方负责）。
pub fn run_file_loop(loop_id: &str) -> Result<FileLoopResult> {
    workspace::ensure_workspace_and_logs()?;
    let pb = playbook::load_file_loop(loop_id)?;
    let base_dir = match pb.base.as_str() {
        "logs" => workspace::logs_dir()?,
        _ => workspace::workspace_root()?,
    };
    let path = base_dir.join(&pb.input);
    let content = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let summary = summarize(&content, &pb);
    Ok(FileLoopResult {
        loop_id: loop_id.to_string(),
        name: pb.name.clone(),
        summary,
        header: pb.header.clone(),
        footer: pb.footer.clone(),
    })
}

/// 公共入口：按给定 loop_id 执行配置式 FileLoopPlaybook，写 audit + memory 并打印。
pub fn run(loop_id: &str) -> Result<()> {
    let engine = FileLoopEngine;
    let result = engine.run(loop_id)?;
    let pb = playbook::load_file_loop(loop_id)?;
    let base_dir = match pb.base.as_str() {
        "logs" => workspace::logs_dir()?,
        _ => workspace::workspace_root()?,
    };
    let path = base_dir.join(&pb.input);

    let mut event = runtime_audit::new_event(
        runtime_audit::REALM_DEFAULT,
        runtime_audit::AGENT_CORE,
        "cron",
        loop_id,
        format!("Execute file-based loop {}", result.name),
    );
    event.tools.push(runtime_audit::tool_call(
        "read_file",
        Some(path.display().to_string()),
    ));

    let semantic_kind = pb.semantic_kind.as_deref().unwrap_or("loop_log");
    let _ = memory::add_semantic_item(semantic_kind, &result.summary, Some(loop_id));

    event.result = RuntimeAuditResult {
        status: "success".to_string(),
        error: None,
    };
    event.risk = RuntimeAuditRisk::default();
    runtime_audit::record(&event)?;

    println!("{}", result.header);
    println!("{}", result.summary);
    println!("---");
    if !result.footer.is_empty() {
        println!("{}", result.footer);
    }
    Ok(())
}

fn summarize(content: &str, pb: &FileLoopPlaybook) -> String {
    let lines: Vec<&str> = content.lines().filter(|s| !s.trim().is_empty()).collect();
    if lines.is_empty() {
        return pb.empty_message.clone();
    }

    let max = pb.max_lines.unwrap_or(0);
    if max > 0 {
        let take = lines.len().min(max);
        return lines[lines.len() - take..].join("\n");
    }

    if let Some(sep) = &pb.join_separator {
        return lines.join(sep.as_str());
    }
    lines.join("\n")
}
