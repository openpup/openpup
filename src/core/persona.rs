//! Persona 文档：IDENTITY / SOUL / USER / TOOLS 的创建与健康检查。

use anyhow::{Context, Result};
use std::fs::{create_dir_all, File};
use std::io::Write;
use std::path::PathBuf;

use crate::core::workspace;

const IDENTITY_TEMPLATE: &str = r#"# IDENTITY – 我是谁

- **职业/领域**：（如：工程 / 创业 / 投资）
- **长期目标**：事业 / 资产 / 生活
- **决策风格**：偏数据 / 偏直觉 / 偏保守 / 偏进取

（请按需编辑，供 openpup 注入人格。）
"#;

const SOUL_TEMPLATE: &str = r#"# SOUL – 价值观与红线

## 价值观
- 先保命再赚钱，长期主义，不做自己不理解的事。
- 尊重隐私与安全，不轻易扩大攻击面。

## 金融红线
- 不使用杠杆，不卖空，无期权/衍生品操作。
- 单笔投入不超过流动资金 X%。
- 总权益资产敞口不超过净资产 Y%。

## 行为红线
- 不伪造身份，不绕过安全机制，不做违法行为。
- 不对家庭/隐私/资产造成不可逆损害。

（请按需填写具体比例与补充条款。）
"#;

const USER_TEMPLATE: &str = r#"# USER – 当前用户状态与偏好

- **生活/工作状态**：（是否高强度工作期、家庭责任等）
- **时间偏好**：早起/晚睡、深度工作时段
- **沟通偏好**：简洁/结构化；希望明确给出「结论 + 备选方案 + 风险说明」

（请按需编辑。）
"#;

const TOOLS_TEMPLATE: &str = r#"# TOOLS – 工具使用哲学与权限分级

- 能自动化解决的重复劳动尽量自动化；高风险/不可逆操作必须经过明确说明与确认。
- **当前阶段**：仅允许 L1（只读）与 L2（草稿），不执行 L3/L4 自动动作。

（后续可在此声明各场景下允许的工具等级。）
"#;

fn persona_dir() -> Result<PathBuf> {
    Ok(workspace::workspace_root()?.join("persona"))
}

fn ensure_persona_dir() -> Result<PathBuf> {
    let dir = persona_dir()?;
    create_dir_all(&dir).with_context(|| format!("failed to create persona dir {:?}", dir))?;
    Ok(dir)
}

/// 在 workspace/persona 下创建 IDENTITY.md, SOUL.md, USER.md, TOOLS.md 模板（不覆盖已有文件）。
pub fn init() -> Result<()> {
    workspace::ensure_workspace_and_logs()?;
    let dir = ensure_persona_dir()?;

    let files = [
        ("IDENTITY.md", IDENTITY_TEMPLATE),
        ("SOUL.md", SOUL_TEMPLATE),
        ("USER.md", USER_TEMPLATE),
        ("TOOLS.md", TOOLS_TEMPLATE),
    ];

    for (name, content) in files {
        let path = dir.join(name);
        if path.exists() {
            println!("  skip (exists): {}", path.display());
            continue;
        }
        let mut f = File::create(&path).with_context(|| format!("failed to create {:?}", path))?;
        f.write_all(content.as_bytes())
            .with_context(|| format!("failed to write {:?}", path))?;
        println!("  created: {}", path.display());
    }

    println!("Persona templates are in {:?}. Edit and save.", dir);
    Ok(())
}

/// 检查 Persona 是否完整、安全：四份文件是否存在；SOUL 是否包含「红线」相关关键词。
pub fn doctor() -> Result<()> {
    workspace::ensure_workspace_and_logs()?;
    let dir = persona_dir()?;

    let required = ["IDENTITY.md", "SOUL.md", "USER.md", "TOOLS.md"];
    let mut missing = Vec::new();
    for name in required {
        if !dir.join(name).exists() {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        println!(
            "Persona doctor: missing files: {:?}. Run `openpup persona init`.",
            missing
        );
        return Ok(());
    }

    // 简单安全检查：SOUL 里应有红线/金融相关表述
    let soul_path = dir.join("SOUL.md");
    let soul = std::fs::read_to_string(&soul_path)
        .with_context(|| format!("failed to read {:?}", soul_path))?;
    let has_red_line = soul.contains("红线") || soul.contains("金融");
    if !has_red_line {
        println!("Persona doctor: SOUL.md should define 红线 or 金融 constraints. Please edit.");
    } else {
        println!("Persona doctor: all required files present; SOUL contains 红线/金融.");
    }
    Ok(())
}

/// Persona 四份文档在运行时中注入时的顺序（IDENTITY → SOUL → USER → TOOLS）。
const PERSONA_FILES: [&str; 4] = ["IDENTITY.md", "SOUL.md", "USER.md", "TOOLS.md"];

/// 从 `workspace/persona/` 读取四份 Persona 文档并拼成一段 Markdown，供上层 runtime 作为 system prompt 或配置注入。
/// 若某文件缺失则跳过该文件并继续，仅当全部缺失时返回错误。
pub fn load_assembled_persona() -> Result<String> {
    let dir = persona_dir()?;
    load_assembled_persona_from_dir(&dir)
}

fn load_assembled_persona_from_dir(dir: &PathBuf) -> Result<String> {
    let mut parts = Vec::new();
    for name in PERSONA_FILES {
        let path = dir.join(name);
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read persona file {:?}", path))?;
            parts.push(format!(
                "## {}\n\n{}",
                name.replace(".md", ""),
                content.trim()
            ));
        }
    }
    if parts.is_empty() {
        anyhow::bail!(
            "no persona files found in {:?}; run `openpup persona init` and edit the files",
            dir
        );
    }
    Ok(parts.join("\n\n---\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_persona_from_dir_orders_and_separates() {
        let base =
            std::env::temp_dir().join(format!("openpup-persona-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("IDENTITY.md"), "A").unwrap();
        std::fs::write(base.join("SOUL.md"), "B").unwrap();
        std::fs::write(base.join("USER.md"), "C").unwrap();
        std::fs::write(base.join("TOOLS.md"), "D").unwrap();

        let out = load_assembled_persona_from_dir(&base).unwrap();
        assert!(out.contains("## IDENTITY"));
        assert!(out.contains("## SOUL"));
        assert!(out.contains("## USER"));
        assert!(out.contains("## TOOLS"));
        assert!(out.contains("\n\n---\n\n"));

        // cleanup best-effort
        let _ = std::fs::remove_dir_all(&base);
    }
}

