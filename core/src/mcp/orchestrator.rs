use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo},
    transport::StreamableHttpClientTransport,
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::mcp::server::LocalMcpServer;

/// A single configured MCP server (serialisable → persisted to JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
    pub enabled: bool,
}

/// Legacy alias used in main.rs env-var bootstrap.
pub type ServerConfig = McpServerEntry;

/// A tool discovered from a connected MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server: String,
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's input parameters (OpenAI-compatible).
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// Sanitize a string so it matches `^[a-zA-Z0-9_-]+$` required by OpenAI-compatible APIs.
/// Replaces any character outside that set with `_`.
pub fn sanitize_tool_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone)]
pub struct MCPOrchestrator {
    servers: Arc<RwLock<HashMap<String, McpServerEntry>>>,
    /// Cache of tools discovered from remote servers.
    tool_cache: Arc<RwLock<HashMap<String, Vec<McpToolInfo>>>>,
    /// Maps sanitized fn_name → (original_server, original_tool_name) for dispatch.
    fn_name_map: Arc<RwLock<HashMap<String, (String, String)>>>,
    config_path: Option<PathBuf>,
}

impl MCPOrchestrator {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            fn_name_map: Arc::new(RwLock::new(HashMap::new())),
            config_path: None,
        }
    }

    /// Load persisted server list from `path`; sets `config_path` for future saves.
    /// Tool discovery for all enabled servers is kicked off in the background.
    pub fn load(path: PathBuf) -> Self {
        let servers: HashMap<String, McpServerEntry> = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Vec<McpServerEntry>>(&t).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|e| (e.name.clone(), e))
                .collect()
        } else {
            HashMap::new()
        };

        let orchestrator = Self {
            servers: Arc::new(RwLock::new(servers.clone())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            fn_name_map: Arc::new(RwLock::new(HashMap::new())),
            config_path: Some(path),
        };

        // Background-discover tools for all enabled servers that were persisted
        let self_clone = orchestrator.clone();
        tokio::spawn(async move {
            for entry in servers.values().filter(|e| e.enabled) {
                match self_clone.discover_server_tools(entry).await {
                    Ok(tools) => {
                        debug!(
                            "[mcp] startup: discovered {} tools from '{}'",
                            tools.len(),
                            entry.name
                        );
                        self_clone
                            .tool_cache
                            .write()
                            .await
                            .insert(entry.name.clone(), tools);
                    }
                    Err(e) => warn!("[mcp] startup discovery for '{}' failed: {e}", entry.name),
                }
            }
        });

        orchestrator
    }

    // ── Server management ───────────────────────────────────────────────────────

    pub async fn add_server(&self, entry: McpServerEntry) -> Result<()> {
        self.servers
            .write()
            .await
            .insert(entry.name.clone(), entry.clone());
        self.persist().await?;
        // Kick off tool discovery in the background so the caller doesn't block.
        let self_clone = self.clone();
        tokio::spawn(async move {
            match self_clone.discover_server_tools(&entry).await {
                Ok(tools) => {
                    debug!(
                        "[mcp] auto-discovered {} tools from '{}'",
                        tools.len(),
                        entry.name
                    );
                    self_clone
                        .tool_cache
                        .write()
                        .await
                        .insert(entry.name.clone(), tools);
                }
                Err(e) => warn!("[mcp] auto-discovery for '{}' failed: {e}", entry.name),
            }
        });
        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> Result<()> {
        self.servers.write().await.remove(name);
        self.tool_cache.write().await.remove(name);
        self.persist().await
    }

    pub async fn toggle_server(&self, name: &str, enabled: bool) -> Result<()> {
        let entry_opt = {
            let mut guard = self.servers.write().await;
            if let Some(e) = guard.get_mut(name) {
                e.enabled = enabled;
                Some(e.clone())
            } else {
                None
            }
        };
        if !enabled {
            self.tool_cache.write().await.remove(name);
        } else if let Some(entry) = entry_opt {
            // Re-enabled — re-discover tools in background
            let self_clone = self.clone();
            tokio::spawn(async move {
                match self_clone.discover_server_tools(&entry).await {
                    Ok(tools) => {
                        self_clone
                            .tool_cache
                            .write()
                            .await
                            .insert(entry.name.clone(), tools);
                    }
                    Err(e) => warn!("[mcp] re-discovery for '{}' failed: {e}", entry.name),
                }
            });
        }
        self.persist().await
    }

    pub async fn list_servers(&self) -> Vec<McpServerEntry> {
        let guard = self.servers.read().await;
        let mut v: Vec<McpServerEntry> = guard.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Legacy helper kept for main.rs env-var path.
    pub async fn connect_to_server(&self, cfg: McpServerEntry) -> Result<()> {
        self.add_server(cfg).await
    }

    // ── Tool discovery ──────────────────────────────────────────────────────────

    /// Refresh the tool cache for all enabled remote servers.
    pub async fn refresh_all_tools(&self) {
        let servers = self.list_servers().await;
        for server in servers {
            if server.enabled {
                match self.discover_server_tools(&server).await {
                    Ok(tools) => {
                        self.tool_cache
                            .write()
                            .await
                            .insert(server.name.clone(), tools);
                    }
                    Err(e) => {
                        warn!("MCP tool discovery for '{}' failed: {e}", server.name);
                    }
                }
            }
        }
    }

    /// Discover tools from a remote MCP server via streamable HTTP transport.
    ///
    /// `base_url` is treated as the full MCP endpoint URL configured by the user.
    async fn discover_server_tools(&self, entry: &McpServerEntry) -> Result<Vec<McpToolInfo>> {
        rmcp_list_tools(&entry.base_url, entry).await
    }

    /// Return all discovered tools (remote cache + built-in local tools).
    pub async fn list_all_tools(&self) -> Vec<McpToolInfo> {
        let mut tools = vec![
            McpToolInfo {
                server: "local".into(),
                name: "read_file".into(),
                description: "Read a local file".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
            },
            McpToolInfo {
                server: "local".into(),
                name: "write_file".into(),
                description: "Write content to a local file".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" } }, "required": ["path", "content"] }),
            },
            McpToolInfo {
                server: "local".into(),
                name: "open_browser".into(),
                description: "Open a URL in the system browser".into(),
                input_schema: serde_json::json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }),
            },
        ];
        let cache = self.tool_cache.read().await;
        for tool_list in cache.values() {
            tools.extend(tool_list.iter().cloned());
        }
        tools
    }

    // ── Deferred tool pattern ──────────────────────────────────────────────────

    /// Return a lightweight catalog of MCP tools: just name + one-line description.
    /// This costs ~10-20 tokens per tool instead of ~100-200 for full schemas.
    /// Use with `deferred_tool_schema()` to fetch the full schema on demand.
    pub async fn deferred_tool_catalog(&self, task: &str, max: usize) -> Vec<serde_json::Value> {
        let all_tools = self.list_all_tools().await;
        if all_tools.is_empty() {
            return vec![];
        }

        // Score tools by task relevance (same as tools_for_task)
        let task_words: std::collections::HashSet<String> = task
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_lowercase())
            .collect();

        let mut scored: Vec<(usize, &McpToolInfo)> = all_tools
            .iter()
            .map(|t| {
                let haystack = format!("{} {}", t.name, t.description).to_lowercase();
                let score = task_words
                    .iter()
                    .filter(|w| haystack.contains(w.as_str()))
                    .count();
                (score, t)
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        let selected: Vec<&McpToolInfo> = scored.into_iter().take(max).map(|(_, t)| t).collect();
        if selected.is_empty() {
            return vec![];
        }

        // Build catalog lines: "mcp__server__tool — description"
        let catalog_lines: Vec<String> = selected
            .iter()
            .map(|t| {
                let fn_name = format!(
                    "mcp__{}__{}",
                    sanitize_tool_name(&t.server),
                    sanitize_tool_name(&t.name)
                );
                let desc: String = t.description.chars().take(80).collect();
                format!("- {fn_name}: {desc}")
            })
            .collect();
        let catalog = catalog_lines.join("\n");

        // Return a single "fetch_mcp_tool" tool that the LLM calls to load the full schema
        vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "fetch_mcp_tool",
                "description": format!(
                    "Load the full schema of an MCP tool so you can call it. Available MCP tools:\n{catalog}\n\nCall this with the tool name to get the full parameter schema, then call the tool directly."
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "The MCP tool function name (e.g. mcp__server__tool) from the catalog above"
                        }
                    },
                    "required": ["tool_name"]
                }
            }
        })]
    }

    /// Fetch the full OpenAI-compatible schema for a specific MCP tool.
    /// Called when the LLM uses the `fetch_mcp_tool` tool.
    pub async fn deferred_tool_schema(&self, fn_name: &str) -> Result<serde_json::Value> {
        let all_specs = self.tools_as_openai_specs().await;
        all_specs
            .into_iter()
            .find(|s| s["function"]["name"].as_str() == Some(fn_name))
            .ok_or_else(|| anyhow!("MCP tool not found: '{fn_name}'"))
    }

    /// Return MCP tools filtered by relevance to `task`.
    ///
    /// When the total tool count is below `max`, all tools are returned.
    /// Otherwise we score each tool by keyword overlap between `task` and the
    /// tool's name + description, then return the top `max` hits.
    /// This keeps the context window tight and avoids hitting provider tool limits.
    pub async fn tools_for_task(&self, task: &str, max: usize) -> Vec<serde_json::Value> {
        let all = self.tools_as_openai_specs().await;
        if all.len() <= max {
            return all;
        }

        // Tokenise task into lowercase words (≥ 3 chars)
        let task_words: std::collections::HashSet<String> = task
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_lowercase())
            .collect();

        let mut scored: Vec<(usize, &serde_json::Value)> = all
            .iter()
            .map(|spec| {
                let name = spec["function"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase();
                let desc = spec["function"]["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase();
                let haystack = format!("{name} {desc}");
                let score = task_words
                    .iter()
                    .filter(|w| haystack.contains(w.as_str()))
                    .count();
                (score, spec)
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        debug!(
            "[mcp] tools_for_task: total={} → selecting top {max} (best_score={})",
            all.len(),
            scored.first().map(|(s, _)| *s).unwrap_or(0)
        );
        scored
            .into_iter()
            .take(max)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Return all cached remote tools as OpenAI-compatible function tool specs.
    ///
    /// Each tool is named `mcp__{server}__{tool}` (sanitized) so the executor can route it
    /// back to the right MCP server without ambiguity.
    pub async fn tools_as_openai_specs(&self) -> Vec<serde_json::Value> {
        let cache = self.tool_cache.read().await;
        let mut specs = Vec::new();
        let mut map = self.fn_name_map.write().await;
        for tools in cache.values() {
            for t in tools {
                let fn_name = format!(
                    "mcp__{}__{}",
                    sanitize_tool_name(&t.server),
                    sanitize_tool_name(&t.name)
                );
                // Store reverse mapping so dispatch can find the original names
                map.insert(fn_name.clone(), (t.server.clone(), t.name.clone()));
                let schema = if t.input_schema.is_null() {
                    serde_json::json!({ "type": "object", "properties": {} })
                } else {
                    t.input_schema.clone()
                };
                specs.push(serde_json::json!({
                  "type": "function",
                  "function": {
                    "name": fn_name,
                    "description": format!("[{}] {}", t.server, t.description),
                    "parameters": schema,
                  }
                }));
            }
        }
        specs
    }

    /// Resolve a sanitized fn_name (as returned by the LLM) back to the original
    /// (server, tool_name) pair needed to call the remote MCP server.
    pub async fn resolve_fn_name(&self, fn_name: &str) -> Option<(String, String)> {
        self.fn_name_map.read().await.get(fn_name).cloned()
    }

    // ── Tool dispatch ───────────────────────────────────────────────────────────

    pub async fn call_tool(&self, server: &str, tool: &str, params: &Value) -> Result<Value> {
        if server == "local" {
            return LocalMcpServer::call(tool, params).await;
        }
        let guard = self.servers.read().await;
        let entry = guard
            .get(server)
            .ok_or_else(|| anyhow!("MCP server not registered: '{server}'"))?;
        if !entry.enabled {
            return Err(anyhow!("MCP server '{server}' is disabled"));
        }
        let entry = entry.clone();
        drop(guard);
        call_remote(&entry, tool, params).await
    }

    // ── Internal ────────────────────────────────────────────────────────────────

    async fn persist(&self) -> Result<()> {
        let Some(ref path) = self.config_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let guard = self.servers.read().await;
        let list: Vec<&McpServerEntry> = guard.values().collect();
        let json = serde_json::to_string_pretty(&list)?;
        drop(guard);
        fs::write(path, json)?;
        Ok(())
    }
}

/// Call a remote MCP tool via streamable HTTP transport.
async fn call_remote(entry: &McpServerEntry, tool: &str, params: &Value) -> Result<Value> {
    rmcp_call_tool(&entry.base_url, entry, tool, params).await
}

/// Build transport config, injecting Bearer auth when the entry has a non-empty token.
fn build_transport_config(
    url: &str,
    entry: &McpServerEntry,
) -> rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig {
    let config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url);
    let token = entry.token.trim();
    if token.is_empty() {
        config
    } else {
        config.auth_header(token)
    }
}

/// Streamable HTTP-based tool listing via rmcp.
async fn rmcp_list_tools(mcp_url: &str, entry: &McpServerEntry) -> Result<Vec<McpToolInfo>> {
    let transport =
        StreamableHttpClientTransport::from_config(build_transport_config(mcp_url, entry));

    let client = ClientInfo::default()
        .serve(transport)
        .await
        .map_err(|e| anyhow!("MCP client init for '{}': {e}", mcp_url))?;

    let result = client
        .list_tools(Default::default())
        .await
        .map_err(|e| anyhow!("list_tools: {e}"))?;

    Ok(result
        .tools
        .into_iter()
        .map(|t| {
            // Capture the full input_schema so we can build proper OpenAI tool specs
            let schema = serde_json::to_value(&*t.input_schema)
                .unwrap_or_else(|_| serde_json::json!({ "type": "object", "properties": {} }));
            McpToolInfo {
                server: entry.name.clone(),
                name: t.name.to_string(),
                description: t.description.clone().unwrap_or_default().to_string(),
                input_schema: schema,
            }
        })
        .collect())
}

/// Streamable HTTP-based tool call via rmcp.
async fn rmcp_call_tool(
    mcp_url: &str,
    entry: &McpServerEntry,
    tool: &str,
    params: &Value,
) -> Result<Value> {
    let transport =
        StreamableHttpClientTransport::from_config(build_transport_config(mcp_url, entry));

    let client = ClientInfo::default()
        .serve(transport)
        .await
        .map_err(|e| anyhow!("MCP client init: {e}"))?;

    let mut req = CallToolRequestParams::new(tool.to_string());
    if let Some(obj) = params.as_object().cloned() {
        req = req.with_arguments(obj.into_iter().collect::<serde_json::Map<_, _>>());
    }

    let result = client
        .call_tool(req)
        .await
        .map_err(|e| anyhow!("call_tool '{tool}': {e}"))?;

    // Extract text content from MCP result
    let text = result
        .content
        .iter()
        .filter_map(|c| {
            if let Some(t) = c.as_text() {
                Some(t.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Value::String(text))
}
