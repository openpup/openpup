/// Command risk assessment for shell_exec safety gating.
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandRiskLevel {
    Low,
    Medium,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Real,
    Sandbox,
}

#[derive(Debug, Clone)]
pub struct CommandRiskContext {
    pub kind: ShellKind,
    pub allowed_roots: Vec<PathBuf>,
}

const HIGH_RISK_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf .",
    "rmdir /s",
    "mkfs",
    "dd if=",
    ":(){ :|:",
    "> /dev/sd",
    "chmod -R 777 /",
    "git push --force",
    "git push -f",
    "git reset --hard",
    "git clean -fd",
    "drop table",
    "drop database",
    "truncate table",
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
    ".npmrc",
    ".netrc",
    "id_rsa",
    "id_ed25519",
    "/etc/passwd",
    "/etc/shadow",
    "/private/etc/passwd",
    "%userprofile%",
];

const SENSITIVE_PATH_FRAGMENTS: &[&str] = &[
    "~/.ssh",
    "~/.aws",
    "~/.gnupg",
    "~/.kube",
    "~/.docker",
    "~/.config",
    "$home",
    "${home}",
    "/.ssh",
    "\\.ssh",
    "/.aws",
    "\\.aws",
    "/.gnupg",
    "\\.gnupg",
    "/.kube",
    "\\.kube",
    "/.docker",
    "\\.docker",
    "/.config",
    "\\.config",
];

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

pub fn assess_command_risk(command: &str, ctx: &CommandRiskContext) -> CommandRiskLevel {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();

    if HIGH_RISK_PATTERNS.iter().any(|pat| lower.contains(pat))
        || SENSITIVE_PATH_FRAGMENTS
            .iter()
            .any(|fragment| lower.contains(fragment))
        || pipes_remote_script_to_shell(&lower)
    {
        return CommandRiskLevel::High;
    }

    let tokens = rough_shell_tokens(trimmed);
    if tokens.iter().any(|token| disallowed_path_token(token, ctx)) {
        return CommandRiskLevel::High;
    }

    if is_low_risk_prefix(&lower) {
        CommandRiskLevel::Low
    } else {
        CommandRiskLevel::Medium
    }
}

fn pipes_remote_script_to_shell(lower: &str) -> bool {
    (lower.contains("curl ") || lower.contains("wget "))
        && (lower.contains("| sh") || lower.contains("| bash") || lower.contains("| zsh"))
}

fn disallowed_path_token(token: &str, ctx: &CommandRiskContext) -> bool {
    let token = strip_path_punctuation(token);
    if token.is_empty() || token.starts_with('-') {
        return false;
    }

    let lower = token.to_ascii_lowercase();
    if lower == "/" || lower == "/*" || lower.starts_with("~/") || lower.starts_with("~\\") {
        return true;
    }

    let Some(path) = absolute_path_token(token) else {
        return false;
    };

    match ctx.kind {
        ShellKind::Real => !is_under_allowed_root(&path, &ctx.allowed_roots),
        ShellKind::Sandbox => !is_under_allowed_root(&path, &[std::env::temp_dir()]),
    }
}

fn absolute_path_token(token: &str) -> Option<PathBuf> {
    let looks_windows_absolute = token.len() >= 3
        && token.as_bytes()[1] == b':'
        && matches!(token.as_bytes()[2], b'\\' | b'/');
    let looks_unc = token.starts_with("\\\\");
    let path = Path::new(token);
    if path.is_absolute() || looks_windows_absolute || looks_unc {
        Some(normalize_path(path))
    } else {
        None
    }
}

fn is_under_allowed_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .map(|root| normalize_path(root))
        .any(|root| path.starts_with(root))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

fn strip_path_punctuation(token: &str) -> &str {
    token.trim_matches(|c| matches!(c, '\'' | '"' | '`' | ',' | ')' | '('))
}

fn rough_shell_tokens(command: &str) -> Vec<&str> {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | ';' | '&'))
        .filter(|token| !token.is_empty())
        .collect()
}

fn is_low_risk_prefix(lower: &str) -> bool {
    LOW_RISK_PREFIXES.iter().any(|prefix| {
        lower.starts_with(prefix)
            && lower[prefix.len()..]
                .chars()
                .next()
                .map(|c| c.is_whitespace() || matches!(c, '|' | ';'))
                .unwrap_or(true)
            && !lower.contains("| rm")
            && !lower.contains("| xargs rm")
    })
}

pub fn format_risk_warning(command: &str, risk: CommandRiskLevel) -> Option<String> {
    if risk != CommandRiskLevel::High {
        return None;
    }
    Some(format!(
        "{{\"risk_level\":\"high\",\"command\":{},\"warning\":\"This command is potentially destructive, reads sensitive host data, or scans outside allowed roots. It was blocked for safety.\"}}",
        serde_json::to_string(command)
            .unwrap_or_else(|_| format!("\"{}\"", command.replace('\"', "\\\"")))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_ctx() -> CommandRiskContext {
        CommandRiskContext {
            kind: ShellKind::Real,
            allowed_roots: vec![
                PathBuf::from("/Users/ben/Workspace/Git/openpup"),
                std::env::temp_dir(),
            ],
        }
    }

    fn sandbox_ctx() -> CommandRiskContext {
        CommandRiskContext {
            kind: ShellKind::Sandbox,
            allowed_roots: vec![std::env::temp_dir()],
        }
    }

    #[test]
    fn real_shell_allows_workspace_absolute_paths() {
        assert_eq!(
            assess_command_risk("rg TODO /Users/ben/Workspace/Git/openpup/src", &real_ctx()),
            CommandRiskLevel::Low
        );
    }

    #[test]
    fn real_shell_blocks_host_root_and_sensitive_paths() {
        assert_eq!(
            assess_command_risk("find / -name '*.rs'", &real_ctx()),
            CommandRiskLevel::High
        );
        assert_eq!(
            assess_command_risk("cat ~/.ssh/id_rsa", &real_ctx()),
            CommandRiskLevel::High
        );
        assert_eq!(
            assess_command_risk("rg token /etc", &real_ctx()),
            CommandRiskLevel::High
        );
    }

    #[test]
    fn sandbox_shell_blocks_workspace_absolute_paths() {
        assert_eq!(
            assess_command_risk(
                "cat /Users/ben/Workspace/Git/openpup/README.md",
                &sandbox_ctx()
            ),
            CommandRiskLevel::High
        );
        assert_eq!(
            assess_command_risk("rg TODO .", &sandbox_ctx()),
            CommandRiskLevel::Low
        );
    }
}
