use crate::cli::OpenpupCli;
use anyhow::{Context, Result};
use dirs::home_dir;
use serde::Serialize;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug, Serialize)]
pub struct AuditRecord<'a> {
    #[serde(rename = "ts")]
    pub timestamp: String,
    pub cli: &'a OpenpupCli,
}

/// 记录一次 CLI 调用到本地审计日志（JSONL）。
///
/// safe first：仅写入用户主目录下的 `~/.openpup/audit.log`，
/// 不包含敏感环境变量或外部系统信息。
pub fn record_invocation(cli: &OpenpupCli) -> Result<()> {
    let home = home_dir().context("failed to locate home directory")?;
    let dir = home.join(".openpup");
    let log_path = dir.join("audit.log");

    create_dir_all(&dir).with_context(|| format!("failed to create audit dir {:?}", dir))?;

    let ts = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    let record = AuditRecord { timestamp: ts, cli };
    let line = serde_json::to_string(&record).context("failed to serialize audit record")?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open audit log at {:?}", log_path))?;

    writeln!(file, "{line}").context("failed to write audit log line")?;
    Ok(())
}
