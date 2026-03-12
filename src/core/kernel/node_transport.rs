//! Worker 节点调用传输层：与具体协议（HTTP 等）解耦，由 kernel 提供，tools 通过 trait 调用。

use anyhow::{Context, Result};
use serde_json::Value;

use crate::core::registry::NodeInfo;
use crate::tools::{NodeTransport, ToolResult};

/// 通过 HTTP POST {host}/tool 调用远端 Worker 的实现。
pub struct HttpNodeTransport;

impl NodeTransport for HttpNodeTransport {
    fn invoke_tool(&self, node: &NodeInfo, tool: &str, args: &Value) -> Result<ToolResult> {
        let host = node
            .host
            .as_deref()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| anyhow::anyhow!("node {:?} has no host configured", node.name))?;
        let url = format!("{}/tool", host.trim_end_matches('/'));
        let body = serde_json::json!({ "tool": tool, "args": args });
        let resp = crate::tools::net::block_on_async(async {
            let client = crate::tools::net::async_client()
                .context("failed to build HTTP client for node tool")?;
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    let s = e.to_string();
                    let hint = if s.contains("connection refused") || s.contains("Connect") {
                        format!(" (is the worker running at {}?)", url)
                    } else {
                        String::new()
                    };
                    anyhow::anyhow!("node request failed: {}{}", s, hint)
                })?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .context("failed to read node response body")?;
            Ok::<(reqwest::StatusCode, String), anyhow::Error>((status, text))
        })?;
        let status = resp.0;
        let text = resp.1;
        if !status.is_success() {
            return Ok(ToolResult {
                ok: false,
                value: None,
                error: Some(format!("node returned {}: {}", status, text)),
            });
        }
        let value: Option<Value> = serde_json::from_str(&text).ok();
        let ok = value
            .as_ref()
            .and_then(|v| v.get("ok"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let error = value
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let result_value = value.as_ref().and_then(|v| v.get("value").cloned());
        Ok(ToolResult {
            ok,
            value: result_value,
            error,
        })
    }
}
