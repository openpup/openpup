use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use openpup_core::ipc::{
    daemon_log_path, daemon_pid_path, daemon_runtime_dir, daemon_socket_path, DaemonEvent,
    DaemonRequest, DaemonResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use tokio::net::UnixStream;

#[cfg(not(unix))]
use tokio::net::TcpStream;

pub async fn is_running() -> bool {
    call(DaemonRequest::Ping).await.is_ok()
}

pub async fn call(request: DaemonRequest) -> Result<DaemonResponse> {
    let events = collect(request).await?;
    for event in events {
        match event {
            DaemonEvent::Response { response } => match response {
                DaemonResponse::Error { message } => return Err(anyhow!(message)),
                other => return Ok(other),
            },
            DaemonEvent::Error { message } => return Err(anyhow!(message)),
            _ => {}
        }
    }
    Err(anyhow!("daemon did not return a response"))
}

pub async fn stream<F>(request: DaemonRequest, mut on_event: F) -> Result<()>
where
    F: FnMut(DaemonEvent) -> Result<()>,
{
    let mut stream = connect().await?;
    let payload = serde_json::to_string(&request)?;
    stream.write_all(payload.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        let event: DaemonEvent = serde_json::from_str(&line)?;
        match &event {
            DaemonEvent::Response {
                response: DaemonResponse::Error { message },
            }
            | DaemonEvent::Error { message } => return Err(anyhow!(message.clone())),
            _ => on_event(event)?,
        }
    }
    Ok(())
}

pub async fn start_process() -> Result<()> {
    std::fs::create_dir_all(daemon_runtime_dir())?;
    let log_path = daemon_log_path();
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open log file `{}`", log_path.display()))?;

    let (mut command, timeout) = daemon_start_command();
    command.stdin(Stdio::null());
    command.stdout(Stdio::from(log_file.try_clone()?));
    command.stderr(Stdio::from(log_file));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    command
        .spawn()
        .context("failed to start daemon process")?;

    wait_until_ready(timeout).await
}

pub async fn stop_process() -> Result<()> {
    match call(DaemonRequest::Shutdown).await {
        Ok(_) => Ok(()),
        Err(error) => {
            if daemon_pid_path().exists() || daemon_socket_path().exists() {
                Err(error)
            } else {
                Ok(())
            }
        }
    }
}

pub fn socket_display() -> String {
    #[cfg(unix)]
    {
        daemon_socket_path().display().to_string()
    }
    #[cfg(not(unix))]
    {
        "127.0.0.1:47829".to_string()
    }
}

pub fn log_path() -> PathBuf {
    daemon_log_path()
}

pub fn pid_path() -> PathBuf {
    daemon_pid_path()
}

pub async fn collect(request: DaemonRequest) -> Result<Vec<DaemonEvent>> {
    let mut events = Vec::new();
    stream(request, |event| {
        events.push(event);
        Ok(())
    })
    .await?;
    Ok(events)
}

async fn wait_until_ready(timeout: Duration) -> Result<()> {
    let started = std::time::Instant::now();
    loop {
        if is_running().await {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(anyhow!("daemon did not become ready in time"));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(unix)]
async fn connect() -> Result<UnixStream> {
    UnixStream::connect(daemon_socket_path())
        .await
        .with_context(|| format!("failed to connect to daemon socket `{}`", socket_display()))
}

#[cfg(not(unix))]
async fn connect() -> Result<TcpStream> {
    TcpStream::connect("127.0.0.1:47829")
        .await
        .context("failed to connect to daemon tcp endpoint")
}

fn resolve_daemon_binary() -> Option<PathBuf> {
    if std::env::current_dir()
        .ok()
        .map(|dir| dir.join("Cargo.toml").exists())
        .unwrap_or(false)
    {
        return None;
    }

    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            let sibling = parent.join("openpupd");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }
    None
}

fn daemon_start_command() -> (Command, Duration) {
    if let Some(binary) = resolve_daemon_binary() {
        let mut command = Command::new(binary);
        command.arg("serve");
        return (command, Duration::from_secs(8));
    }

    let mut command = Command::new("cargo");
    command.args(["run", "-p", "openpup-daemon", "--bin", "openpupd", "--", "serve"]);
    (command, Duration::from_secs(90))
}
