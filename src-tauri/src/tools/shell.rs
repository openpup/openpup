/// Shell tool implementations for ToolRegistry.

use std::time::Duration;

use anyhow::{anyhow, Result};
use tracing::debug;

use super::primitive::{truncate_chars, ToolRegistry};

impl ToolRegistry {
    /// Maximum execution time for shell_exec (2 minutes).
    pub(crate) const SHELL_EXEC_TIMEOUT_MS: u64 = 120_000;

    /// Build a platform-appropriate shell command.
    pub(crate) fn build_shell_command(command: &str) -> tokio::process::Command {
        #[cfg(windows)]
        {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/d", "/s", "/c", command]);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(["-c", command]);
            cmd
        }
    }

    /// Validate a command for sandbox execution. Rejects destructive patterns.
    pub(crate) fn validate_sandbox_command(command: &str) -> Result<()> {
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

    /// Kill a child process and its descendants.
    #[cfg(not(windows))]
    pub(crate) async fn kill_process_tree(pid: u32) {
        use tokio::process::Command;
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .output()
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .output()
            .await;
    }

    #[cfg(windows)]
    pub(crate) async fn kill_process_tree(pid: u32) {
        use tokio::process::Command;
        let _ = Command::new("taskkill")
            .args(["/T", "/PID", &pid.to_string()])
            .output()
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }

    pub(crate) async fn shell_exec(&self, command: &str) -> Result<String> {
        debug!("[tool/shell_exec] $ {}", truncate_chars(command, 120));

        let mut child = Self::build_shell_command(command)
            .current_dir(&self.workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("shell_exec failed to spawn: {e}"))?;

        let pid = child.id();

        match tokio::time::timeout(
            Duration::from_millis(Self::SHELL_EXEC_TIMEOUT_MS),
            child.wait_with_output(),
        )
        .await
        {
            Ok(Ok(output)) => Ok(self.format_process_output(
                output.stdout,
                output.stderr,
                output.status.code(),
                false,
                None,
            )),
            Ok(Err(e)) => Err(anyhow!("shell_exec failed: {e}")),
            Err(_) => {
                if let Some(pid) = pid {
                    Self::kill_process_tree(pid).await;
                }
                Ok(self.dynamic_truncate(&format!(
                    "shell_exec timed out after {} ms (process killed)",
                    Self::SHELL_EXEC_TIMEOUT_MS
                )))
            }
        }
    }

    pub(crate) async fn sandbox_shell_exec(&self, command: &str, timeout_ms: u64) -> Result<String> {
        Self::validate_sandbox_command(command)?;

        let timeout_ms = timeout_ms.clamp(1_000, 30_000);
        let sandbox_dir =
            tempfile::tempdir().map_err(|e| anyhow!("sandbox_shell_exec tempdir: {e}"))?;
        let sandbox_path = sandbox_dir.path().to_path_buf();
        debug!(
            "[tool/sandbox_shell_exec] {} in {}",
            truncate_chars(command, 120),
            sandbox_path.display()
        );

        let mut cmd = Self::build_shell_command(command);
        cmd.current_dir(&sandbox_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(windows)]
        {
            cmd.env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default())
                .env(
                    "SYSTEMROOT",
                    std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into()),
                )
                .env("TEMP", &sandbox_path)
                .env("TMP", &sandbox_path)
                .env("USERPROFILE", &sandbox_path);
        }
        #[cfg(not(windows))]
        {
            cmd.env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("HOME", &sandbox_path)
                .env("TMPDIR", &sandbox_path);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("sandbox_shell_exec failed to spawn: {e}"))?;

        let pid = child.id();

        match tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
            .await
        {
            Ok(Ok(output)) => Ok(self.format_process_output(
                output.stdout,
                output.stderr,
                output.status.code(),
                false,
                Some(&sandbox_path),
            )),
            Ok(Err(e)) => Err(anyhow!("sandbox_shell_exec failed: {e}")),
            Err(_) => {
                if let Some(pid) = pid {
                    Self::kill_process_tree(pid).await;
                }
                Ok(self.dynamic_truncate(&format!(
                    "sandbox_shell_exec timed out after {} ms (process killed)\nsandbox_dir: {}",
                    timeout_ms,
                    sandbox_path.display()
                )))
            }
        }
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
