/// Shell tool implementations for ToolRegistry.
use anyhow::{anyhow, Result};
use openpup_capabilities::ExecRequest;
use tracing::debug;

use super::primitive::{truncate_chars, ToolRegistry};
use super::risk::{assess_command_risk, CommandRiskLevel, ShellKind};

impl ToolRegistry {
    /// Maximum execution time for shell_exec (2 minutes).
    pub(crate) const SHELL_EXEC_TIMEOUT_MS: u64 = 120_000;

    /// Validate a command for sandbox execution. Rejects destructive patterns.
    pub(crate) fn validate_sandbox_command(&self, command: &str) -> Result<()> {
        if assess_command_risk(command, &self.command_risk_context(ShellKind::Sandbox))
            == CommandRiskLevel::High
        {
            return Err(anyhow!(
                "sandbox_shell_exec: blocked high-risk command: {}",
                truncate_chars(command, 120)
            ));
        }

        let blocked_patterns = [
            "rm -rf /",
            "mkfs",
            "dd if=",
            ":(){ :|:",
            "> /dev/sd",
            "chmod -R 777 /",
        ];
        let lower = command.to_lowercase();
        for pat in &blocked_patterns {
            if lower.contains(pat) {
                return Err(anyhow!(
                    "sandbox_shell_exec: blocked dangerous command pattern: '{pat}'"
                ));
            }
        }

        #[cfg(windows)]
        {
            if command.contains("&&") && (lower.contains("del /") || lower.contains("rmdir /s")) {
                return Err(anyhow!(
                    "sandbox_shell_exec: blocked potentially destructive chained command"
                ));
            }
        }

        Ok(())
    }
    pub(crate) async fn shell_exec(&self, command: &str) -> Result<String> {
        debug!("[tool/shell_exec] $ {}", truncate_chars(command, 120));
        if !self.capabilities.process.is_supported() {
            return Err(anyhow!(
                "shell_exec is not supported by the current runtime"
            ));
        }
        let output = self
            .capabilities
            .process
            .exec(ExecRequest {
                command: command.to_string(),
                cwd: self.workspace_root.clone(),
                timeout_ms: Self::SHELL_EXEC_TIMEOUT_MS,
                sandboxed: false,
            })
            .await?;

        Ok(self.format_process_output(
            output.stdout,
            output.stderr,
            output.exit_code,
            output.timed_out,
            output.sandbox_dir.as_deref(),
        ))
    }

    pub(crate) async fn sandbox_shell_exec(
        &self,
        command: &str,
        timeout_ms: u64,
    ) -> Result<String> {
        self.validate_sandbox_command(command)?;

        let timeout_ms = timeout_ms.clamp(1_000, 30_000);
        if !self.capabilities.process.is_supported() {
            return Err(anyhow!(
                "sandbox_shell_exec is not supported by the current runtime"
            ));
        }
        let output = self
            .capabilities
            .process
            .exec(ExecRequest {
                command: command.to_string(),
                cwd: self.workspace_root.clone(),
                timeout_ms,
                sandboxed: true,
            })
            .await?;

        Ok(self.format_process_output(
            output.stdout,
            output.stderr,
            output.exit_code,
            output.timed_out,
            output.sandbox_dir.as_deref(),
        ))
    }

    pub(crate) fn format_process_output(
        &self,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: Option<i32>,
        timed_out: bool,
        sandbox_dir: Option<&std::path::Path>,
    ) -> String {
        let stdout = String::from_utf8_lossy(&stdout).into_owned();
        let stderr = String::from_utf8_lossy(&stderr).into_owned();
        let mut sections = Vec::new();
        sections.push(format!(
            "exit_code: {}",
            exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "terminated".to_string())
        ));
        sections.push(format!("timed_out: {timed_out}"));
        if let Some(path) = sandbox_dir {
            sections.push(format!("sandbox_dir: {}", path.display()));
        }
        if !stdout.trim().is_empty() {
            sections.push(format!("stdout:\n{}", stdout.trim()));
        }
        if !stderr.trim().is_empty() {
            sections.push(format!("stderr:\n{}", stderr.trim()));
        }
        self.dynamic_truncate(&sections.join("\n\n"))
    }
}
