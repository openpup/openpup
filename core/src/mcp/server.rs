/// Native Rust MCP tool server — replaces the Python FastAPI sidecar.
///
/// Tools exposed:
///   ping          → health check
///   read_file     → read a local file (params: {"path": "..."})
///   write_file    → write a local file (params: {"path": "...", "content": "..."})
///   open_browser  → open a URL in the default browser (params: {"url": "..."})
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

pub struct LocalMcpServer;

impl LocalMcpServer {
    pub async fn call(tool: &str, params: &Value) -> Result<Value> {
        match tool {
            "ping" => Ok(json!({"result": "pong"})),
            "read_file" => Self::read_file(params),
            "write_file" => Self::write_file(params),
            "open_browser" => Self::open_browser(params),
            _ => Err(anyhow!("Unknown local tool: {}", tool)),
        }
    }

    fn read_file(params: &Value) -> Result<Value> {
        let path = require_str(params, "path")?;
        let path = safe_path(&path)?;
        let content = fs::read_to_string(&path)
            .map_err(|e| anyhow!("read_file {}: {}", path.display(), e))?;
        Ok(json!({"result": content}))
    }

    fn write_file(params: &Value) -> Result<Value> {
        let path = require_str(params, "path")?;
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let path = safe_path(&path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content).map_err(|e| anyhow!("write_file {}: {}", path.display(), e))?;
        Ok(json!({"result": format!("written:{}", path.display())}))
    }

    fn open_browser(params: &Value) -> Result<Value> {
        let url = require_str(params, "url")?;
        // Validate scheme to prevent command injection
        if !url.starts_with("https://") && !url.starts_with("http://") {
            bail!("open_browser: only http/https URLs are allowed");
        }
        open_url(&url)?;
        Ok(json!({"result": format!("opened:{url}")}))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn require_str(params: &Value, key: &str) -> Result<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing required param: {}", key))
}

/// Reject paths that escape the application workspace root.
fn safe_path(raw: &str) -> Result<PathBuf> {
    let app_root = crate::config::app_root()?;

    // Expand leading ~
    let expanded = if let Some(stripped) = raw.strip_prefix("~/") {
        app_root.join(stripped)
    } else {
        PathBuf::from(raw)
    };

    // Canonicalise the parent to resolve symlinks and .. traversal, then
    // re-attach the filename so the file need not exist yet.
    let parent = expanded.parent().unwrap_or(&expanded);
    let filename = expanded
        .file_name()
        .ok_or_else(|| anyhow!("path has no filename: {}", raw))?;

    let resolved_parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf()); // allow non-existent parents (write_file will create them)

    let resolved = resolved_parent.join(filename);

    // Ensure the resolved path stays within the app workspace root.
    if !resolved.starts_with(&app_root) {
        bail!(
            "path '{}' is outside the app workspace root — refusing for safety",
            resolved.display()
        );
    }

    Ok(resolved)
}

/// Cross-platform browser opener — no shell injection risk since we pass the
/// URL as a separate argument, not through a shell.
fn open_url(url: &str) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    bail!("open_browser is not supported on mobile");

    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()?;

    Ok(())
}
