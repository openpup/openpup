use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;

#[derive(Deserialize)]
struct ToolRequest {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[derive(Serialize)]
struct ToolResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// L1 只读：返回 Worker 版本与系统信息，供 Core 做 node test 健康检查。
fn tool_health_check() -> ToolResponse {
    ToolResponse {
        ok: true,
        value: Some(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        })),
        error: None,
    }
}

/// L1 只读：返回当前工作目录与简单磁盘信息（占位，可扩展为 df/stat）。
fn tool_disk_usage_readonly() -> ToolResponse {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    ToolResponse {
        ok: true,
        value: Some(serde_json::json!({
            "cwd": cwd,
            "message": "L1 read-only disk info (extend with df/stat if needed)",
        })),
        error: None,
    }
}

async fn handle_tool(Json(req): Json<ToolRequest>) -> Json<ToolResponse> {
    let resp = match req.tool.as_str() {
        "health_check" => tool_health_check(),
        "disk_usage_readonly" => tool_disk_usage_readonly(),
        "echo" => ToolResponse {
            ok: true,
            value: Some(req.args),
            error: None,
        },
        "time_now" => {
            let ts = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "unknown".to_string());
            ToolResponse {
                ok: true,
                value: Some(serde_json::json!({ "now_utc": ts })),
                error: None,
            }
        }
        other => ToolResponse {
            ok: false,
            value: None,
            error: Some(format!("unknown tool {:?}", other)),
        },
    };
    Json(resp)
}

#[tokio::main]
async fn main() {
    let addr: SocketAddr = std::env::var("OPENPUP_NODE_WORKER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()
        .expect("invalid OPENPUP_NODE_WORKER_ADDR (expected host:port)");

    let app = Router::new().route("/tool", post(handle_tool));
    println!("openpup node-worker listening on http://{addr}/tool");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");
    axum::serve(listener, app)
        .await
        .expect("node-worker server error");
}
