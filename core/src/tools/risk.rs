/// Command risk assessment for shell_exec safety gating.
use serde::Serialize;

/// Risk level for a shell command, used to decide whether to auto-approve
/// in leashed mode or require explicit confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandRiskLevel {
    /// Read-only, no side effects (ls, cat, git status, pwd, echo, etc.)
    Low,
    /// Writes files, installs packages, or modifies local state
    Medium,
    /// Destructive, irreversible, or affects remote systems
    High,
}

impl std::fmt::Display for CommandRiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

/// High-risk patterns that warrant blocking or explicit user confirmation.
const HIGH_RISK_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf .",
    "rmdir /s",
    "mkfs",
    "dd if=",
    ":(){ :|:", // fork bomb
    "> /dev/sd",
    "chmod -R 777 /",
    "git push --force",
    "git push -f",
    "git reset --hard",
    "git clean -fd",
    "DROP TABLE",
    "DROP DATABASE",
    "TRUNCATE TABLE",
    "shutdown",
    "reboot",
    "halt",
    "init 0",
    "kill -9 1",
    "pkill -9",
    "curl | sh",
    "curl | bash",
    "wget | sh",
    "wget | bash",
    "> /etc/",
    "sudo rm",
];

/// Read-only command prefixes that are safe to auto-approve.
const LOW_RISK_PREFIXES: &[&str] = &[
    "ls",
    "dir",
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "pwd",
    "echo",
    "printf",
    "wc",
    "which",
    "where",
    "whoami",
    "hostname",
    "uname",
    "date",
    "cal",
    "git status",
    "git log",
    "git diff",
    "git branch",
    "git show",
    "git remote -v",
    "git tag",
    "git stash list",
    "env",
    "printenv",
    "set",
    "ps",
    "top",
    "htop",
    "df",
    "du",
    "free",
    "uptime",
    "file",
    "stat",
    "md5sum",
    "sha256sum",
    "shasum",
    "find",
    "locate",
    "grep",
    "rg",
    "ag",
    "ack",
    "tree",
    "realpath",
    "readlink",
    "basename",
    "dirname",
    "jq",
    "yq",
    "python -c",
    "python3 -c",
    "node -e",
    "cargo --version",
    "rustc --version",
    "npm --version",
    "go version",
    "java -version",
];

/// Assess the risk level of a shell command.
pub fn assess_command_risk(command: &str) -> CommandRiskLevel {
    let trimmed = command.trim();
    let lower = trimmed.to_lowercase();

    // Check high-risk patterns first
    for pat in HIGH_RISK_PATTERNS {
        if lower.contains(&pat.to_lowercase()) {
            return CommandRiskLevel::High;
        }
    }

    // Pipe into shell is high risk
    if (lower.contains("curl ") || lower.contains("wget "))
        && (lower.contains("| sh") || lower.contains("| bash") || lower.contains("| zsh"))
    {
        return CommandRiskLevel::High;
    }

    // Check if it's a known read-only command
    for prefix in LOW_RISK_PREFIXES {
        if lower.starts_with(prefix) {
            let rest = &lower[prefix.len()..];
            if rest.is_empty()
                || rest.starts_with(' ')
                || rest.starts_with('\t')
                || rest.starts_with('|')
                || rest.starts_with(';')
            {
                if lower.contains("| rm") || lower.contains("| xargs rm") {
                    return CommandRiskLevel::High;
                }
                return CommandRiskLevel::Low;
            }
        }
    }

    CommandRiskLevel::Medium
}

/// Format a structured warning for high-risk commands.
pub fn format_risk_warning(command: &str, risk: CommandRiskLevel) -> Option<String> {
    if risk != CommandRiskLevel::High {
        return None;
    }
    Some(format!(
        "{{\"risk_level\":\"high\",\"command\":{},\"warning\":\"This command is potentially destructive or irreversible. It was blocked for safety. Use sandbox_shell_exec to test, or ask the user for explicit confirmation.\"}}",
        serde_json::to_string(command).unwrap_or_else(|_| format!("\"{}\"", command.replace('\"', "\\\"")))
    ))
}
