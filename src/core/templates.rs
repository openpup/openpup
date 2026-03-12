//! 内置模板：Loop Playbook / 其他 workspace 文件的默认内容。
//!
//! 说明：
//! - 模板内容在编译时通过 `include_str!` 嵌入二进制，运行时不依赖仓库路径。
//! - CLI（如 `openpup onboard`）可以调用这些函数，将模板写入 `~/.openpup/workspace`。

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::core::workspace;

/// 已知的内置 Loop ID。
pub const BUILTIN_LOOP_IDS: &[&str] = &[
    "work_morning",
    "work_plan_draft",
    "invest_morning",
    "invest_close",
    "life_morning",
    "life_evening",
];

fn loop_playbook_template(loop_id: &str) -> Option<&'static str> {
    match loop_id {
        "work_morning" => Some(include_str!("../../templates/loops/work_morning.toml")),
        "work_plan_draft" => Some(include_str!("../../templates/loops/work_plan_draft.toml")),
        "invest_morning" => Some(include_str!("../../templates/loops/invest_morning.toml")),
        "invest_close" => Some(include_str!("../../templates/loops/invest_close.toml")),
        "life_morning" => Some(include_str!("../../templates/loops/life_morning.toml")),
        "life_evening" => Some(include_str!("../../templates/loops/life_evening.toml")),
        _ => None,
    }
}

fn playbooks_dir() -> Result<PathBuf> {
    Ok(workspace::workspace_root()?.join("playbooks"))
}

/// 若 `~/.openpup/workspace/playbooks/<loop_id>.toml` 不存在，则写入对应模板。
/// 若模板不存在或文件已存在，则什么都不做。
pub fn ensure_loop_playbook(loop_id: &str) -> Result<()> {
    let Some(tpl) = loop_playbook_template(loop_id) else {
        return Ok(());
    };
    let dir = playbooks_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("failed to create playbooks dir {:?}", dir))?;
    let path = dir.join(format!("{loop_id}.toml"));
    if path.exists() {
        return Ok(());
    }
    let mut f =
        fs::File::create(&path).with_context(|| format!("failed to create {:?}", path))?;
    f.write_all(tpl.as_bytes())
        .with_context(|| format!("failed to write {:?}", path))?;
    Ok(())
}

/// 在 workspace 根目录下，若缺少 today_tasks.md / life_notes.md，则写入最小模板。
pub fn ensure_workspace_markdown_templates() -> Result<()> {
    let root = workspace::workspace_root()?;
    let today = root.join("today_tasks.md");
    if !today.exists() {
        let mut f = fs::File::create(&today)
            .with_context(|| format!("failed to create {:?}", today))?;
        writeln!(
            f,
            "# Today Tasks\n\n- [ ] 示例：整理今日工作任务\n- [ ] 示例：处理重要邮件/PR\n"
        )
        .with_context(|| format!("failed to write {:?}", today))?;
    }

    let life = root.join("life_notes.md");
    if !life.exists() {
        let mut f =
            fs::File::create(&life).with_context(|| format!("failed to create {:?}", life))?;
        writeln!(
            f,
            "# Life Notes\n\n- 示例：记录生活观察、习惯调整、健康相关想法。\n"
        )
        .with_context(|| format!("failed to write {:?}", life))?;
    }

    Ok(())
}

