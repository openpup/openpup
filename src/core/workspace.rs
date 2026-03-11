//! openpup workspace：Persona 文档、日志文件、占位数据的根目录。

use anyhow::{Context, Result};
use dirs::home_dir;
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

/// `~/.openpup/workspace`（若旧路径 `~/.openpup/workspace` 已存在且新路径不存在，则优先使用旧路径以兼容历史数据）。
pub fn workspace_root() -> Result<PathBuf> {
    let home = home_dir().context("failed to locate home directory")?;
    let new = home.join(".openpup").join("workspace");
    let legacy = home.join(".openpup").join("workspace");
    if new.exists() || !legacy.exists() {
        Ok(new)
    } else {
        Ok(legacy)
    }
}

/// `~/.openpup/workspace/logs`
pub fn logs_dir() -> Result<PathBuf> {
    Ok(workspace_root()?.join("logs"))
}

/// 确保 workspace 与 logs 目录存在，并创建空的 WORK_LOG.md / INVEST_LOG.md（若不存在）。
pub fn ensure_workspace_and_logs() -> Result<()> {
    let root = workspace_root()?;
    create_dir_all(&root).with_context(|| format!("failed to create workspace {:?}", root))?;

    let log_dir = logs_dir()?;
    create_dir_all(&log_dir).with_context(|| format!("failed to create logs dir {:?}", log_dir))?;

    for name in ["WORK_LOG.md", "INVEST_LOG.md"] {
        let p = log_dir.join(name);
        if !p.exists() {
            let mut f =
                File::create(&p).with_context(|| format!("failed to create log file {:?}", p))?;
            writeln!(f, "# {}", name.replace('_', " "))?;
            writeln!(f, "")?;
            writeln!(f, "<!-- openpup 将在此追加每日/每周摘要 -->")?;
        }
    }
    Ok(())
}

