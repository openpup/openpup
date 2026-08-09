use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo},
    transport::StreamableHttpClientTransport,
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::llm::client::LlmClient;
use crate::mcp::server::LocalMcpServer;

/// A single configured MCP server (serialisable → persisted to JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub base_url: String,
    pub token: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
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
    /// Productized MCP catalog snapshot containing raw tools plus derived views.
    catalog: Arc<RwLock<Arc<McpCatalogSnapshot>>>,
    /// Optional embedder used to rank MCP tools semantically.
    embedder: Arc<RwLock<Option<Arc<LlmClient>>>>,
    /// In-memory semantic retrieval cache keyed by OpenAI function name.
    tool_embedding_cache: Arc<RwLock<HashMap<String, ToolEmbedding>>>,
    config_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ToolEmbedding {
    doc_hash: u64,
    vector: Vec<f32>,
}

#[derive(Debug, Clone)]
struct CachedMcpTool {
    raw: McpToolInfo,
    fn_name: String,
    openai_spec: serde_json::Value,
    retrieval_doc: String,
    parameter_names: String,
    parameter_descriptions: String,
    doc_hash: u64,
}

#[derive(Debug, Clone)]
struct FinanceCapabilityAlias {
    actual_server: String,
    actual_tool: String,
    alias_server: String,
    alias_tool: String,
    description: String,
}

#[derive(Debug, Default)]
struct McpCatalogSnapshot {
    entries: Vec<Arc<CachedMcpTool>>,
    by_fn_name: HashMap<String, Arc<CachedMcpTool>>,
    fn_name_map: HashMap<String, (String, String)>,
    openai_specs: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolSelectionSource {
    Semantic,
    Lexical,
    DiverseFallback,
}

impl ToolSelectionSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Lexical => "lexical",
            Self::DiverseFallback => "diverse_fallback",
        }
    }
}

struct ToolSelection<'a> {
    tools: Vec<&'a serde_json::Value>,
    best_lexical_score: usize,
    source: ToolSelectionSource,
}

struct CatalogToolSelection<'a> {
    tools: Vec<&'a CachedMcpTool>,
    best_lexical_score: usize,
    source: ToolSelectionSource,
}

impl MCPOrchestrator {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(Arc::new(McpCatalogSnapshot::default()))),
            embedder: Arc::new(RwLock::new(None)),
            tool_embedding_cache: Arc::new(RwLock::new(HashMap::new())),
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
                .map(|e| {
                    let normalized = Self::normalize_server_entry(e);
                    (normalized.name.clone(), normalized)
                })
                .collect()
        } else {
            HashMap::new()
        };

        let orchestrator = Self {
            servers: Arc::new(RwLock::new(servers.clone())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(Arc::new(McpCatalogSnapshot::default()))),
            embedder: Arc::new(RwLock::new(None)),
            tool_embedding_cache: Arc::new(RwLock::new(HashMap::new())),
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
                        self_clone.replace_server_tools(&entry.name, tools).await;
                    }
                    Err(e) => warn!("[mcp] startup discovery for '{}' failed: {e}", entry.name),
                }
            }
        });

        orchestrator
    }

    pub async fn set_embedder(&self, embedder: Arc<LlmClient>) {
        *self.embedder.write().await = Some(embedder);
        self.spawn_warm_tool_embeddings_from_cache().await;
    }

    // ── Server management ───────────────────────────────────────────────────────

    pub async fn add_server(&self, entry: McpServerEntry) -> Result<()> {
        let entry = Self::normalize_server_entry(entry);
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
                    self_clone.replace_server_tools(&entry.name, tools).await;
                }
                Err(e) => warn!("[mcp] auto-discovery for '{}' failed: {e}", entry.name),
            }
        });
        Ok(())
    }

    pub async fn update_server(&self, name: &str, entry: McpServerEntry) -> Result<()> {
        let entry = Self::normalize_server_entry(entry);
        {
            let mut guard = self.servers.write().await;
            if !guard.contains_key(name) {
                return Err(anyhow!("MCP server '{name}' not found"));
            }
            if name != entry.name && guard.contains_key(&entry.name) {
                return Err(anyhow!("MCP server '{}' already exists", entry.name));
            }
            guard.remove(name);
            guard.insert(entry.name.clone(), entry.clone());
        }
        self.remove_server_tools(name).await;
        if name != entry.name {
            self.remove_server_tools(&entry.name).await;
        }
        self.persist().await?;
        if entry.enabled {
            let self_clone = self.clone();
            tokio::spawn(async move {
                match self_clone.discover_server_tools(&entry).await {
                    Ok(tools) => {
                        self_clone.replace_server_tools(&entry.name, tools).await;
                    }
                    Err(e) => warn!(
                        "[mcp] auto-discovery for updated '{}' failed: {e}",
                        entry.name
                    ),
                }
            });
        }
        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> Result<()> {
        self.servers.write().await.remove(name);
        self.remove_server_tools(name).await;
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
            self.remove_server_tools(name).await;
        } else if let Some(entry) = entry_opt {
            // Re-enabled — re-discover tools in background
            let self_clone = self.clone();
            tokio::spawn(async move {
                match self_clone.discover_server_tools(&entry).await {
                    Ok(tools) => {
                        self_clone.replace_server_tools(&entry.name, tools).await;
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
                        self.replace_server_tools(&server.name, tools).await;
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
        let tools = rmcp_list_tools(&entry.base_url, entry).await?;
        Ok(Self::filter_tools_by_allowlist(entry, tools))
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
        let catalog = self.catalog_snapshot().await;
        let selection = self
            .select_catalog_tools_best_effort(&catalog.entries, task, max)
            .await;
        self.build_deferred_tool_catalog_from_entries(catalog.entries.len(), selection)
    }

    pub async fn deferred_tool_catalog_best_effort_from_specs(
        &self,
        specs: &[serde_json::Value],
        task: &str,
        max: usize,
    ) -> Vec<serde_json::Value> {
        let selection = self
            .select_tools_for_task_best_effort(specs, task, max)
            .await;
        self.build_deferred_tool_catalog(specs.len(), selection)
    }

    pub fn deferred_tool_catalog_from_specs(
        &self,
        specs: &[serde_json::Value],
        task: &str,
        max: usize,
    ) -> Vec<serde_json::Value> {
        let selection = Self::select_tools_for_task(specs, task, max);
        self.build_deferred_tool_catalog(specs.len(), selection)
    }

    fn build_deferred_tool_catalog(
        &self,
        total_tools: usize,
        selection: ToolSelection<'_>,
    ) -> Vec<serde_json::Value> {
        if selection.tools.is_empty() {
            return vec![];
        }
        debug!(
            "[mcp] deferred_tool_catalog: total={} → catalog entries={} (source={}, best_lexical_score={})",
            total_tools,
            selection.tools.len(),
            selection.source.as_str(),
            selection.best_lexical_score
        );

        // Build catalog lines: "mcp__server__tool — description"
        let catalog_lines: Vec<String> = selection
            .tools
            .iter()
            .map(|t| {
                let fn_name = Self::tool_name(t);
                let desc: String = Self::tool_description(t).chars().take(80).collect();
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

    fn build_deferred_tool_catalog_from_entries(
        &self,
        total_tools: usize,
        selection: CatalogToolSelection<'_>,
    ) -> Vec<serde_json::Value> {
        if selection.tools.is_empty() {
            return vec![];
        }
        debug!(
            "[mcp] deferred_tool_catalog: total={} → catalog entries={} (source={}, best_lexical_score={})",
            total_tools,
            selection.tools.len(),
            selection.source.as_str(),
            selection.best_lexical_score
        );

        let catalog_lines: Vec<String> = selection
            .tools
            .iter()
            .map(|entry| {
                let desc: String = entry.raw.description.chars().take(80).collect();
                format!("- {}: {}", entry.fn_name, desc)
            })
            .collect();
        let catalog = catalog_lines.join("\n");

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
        self.catalog_snapshot()
            .await
            .by_fn_name
            .get(fn_name)
            .map(|entry| entry.openai_spec.clone())
            .ok_or_else(|| anyhow!("MCP tool not found: '{fn_name}'"))
    }

    /// Return MCP tools filtered by relevance to `task`.
    ///
    /// When the total tool count is below `max`, all tools are returned.
    /// Otherwise we score each tool by weighted keyword overlap between `task`
    /// and the tool's name, description, parameter names, and parameter
    /// descriptions, then return the top `max` hits. If nothing matches, we
    /// choose a deterministic server-diverse fallback instead of trusting cache
    /// iteration order.
    /// This keeps the context window tight and avoids hitting provider tool limits.
    pub async fn tools_for_task(&self, task: &str, max: usize) -> Vec<serde_json::Value> {
        let catalog = self.catalog_snapshot().await;
        let selection = self
            .select_catalog_tools_best_effort(&catalog.entries, task, max)
            .await;
        debug!(
            "[mcp] tools_for_task: total={} → selecting top {max} (source={}, best_lexical_score={})",
            catalog.entries.len(),
            selection.source.as_str(),
            selection.best_lexical_score
        );
        selection
            .tools
            .into_iter()
            .map(|entry| entry.openai_spec.clone())
            .collect()
    }

    pub async fn tools_for_task_best_effort_from_specs(
        &self,
        specs: &[serde_json::Value],
        task: &str,
        max: usize,
    ) -> Vec<serde_json::Value> {
        if specs.len() <= max {
            return specs.to_vec();
        }

        let selection = self
            .select_tools_for_task_best_effort(specs, task, max)
            .await;
        debug!(
            "[mcp] tools_for_task: total={} → selecting top {max} (source={}, best_lexical_score={})",
            specs.len(),
            selection.source.as_str(),
            selection.best_lexical_score
        );
        selection.tools.into_iter().cloned().collect()
    }

    pub fn tools_for_task_from_specs(
        &self,
        specs: &[serde_json::Value],
        task: &str,
        max: usize,
    ) -> Vec<serde_json::Value> {
        if specs.len() <= max {
            return specs.to_vec();
        }

        let selection = Self::select_tools_for_task(specs, task, max);
        debug!(
            "[mcp] tools_for_task: total={} → selecting top {max} (source={}, best_lexical_score={})",
            specs.len(),
            selection.source.as_str(),
            selection.best_lexical_score
        );
        selection.tools.into_iter().cloned().collect()
    }

    fn select_tools_for_task<'a>(
        specs: &'a [serde_json::Value],
        task: &str,
        max: usize,
    ) -> ToolSelection<'a> {
        if specs.is_empty() || max == 0 {
            return ToolSelection {
                tools: vec![],
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        let task_words = Self::task_words(task);
        if task_words.is_empty() {
            return ToolSelection {
                tools: Self::select_diverse_tools(specs, max),
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        let mut scored: Vec<(usize, String, &serde_json::Value)> = specs
            .iter()
            .map(|spec| {
                (
                    Self::tool_score(spec, &task_words),
                    Self::tool_name(spec).to_string(),
                    spec,
                )
            })
            .collect();
        let best_score = scored.iter().map(|(score, _, _)| *score).max().unwrap_or(0);
        if best_score == 0 {
            return ToolSelection {
                tools: Self::select_diverse_tools(specs, max),
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        ToolSelection {
            tools: scored
                .into_iter()
                .take(max)
                .map(|(_, _, spec)| spec)
                .collect(),
            best_lexical_score: best_score,
            source: ToolSelectionSource::Lexical,
        }
    }

    async fn select_tools_for_task_best_effort<'a>(
        &self,
        specs: &'a [serde_json::Value],
        task: &str,
        max: usize,
    ) -> ToolSelection<'a> {
        if specs.is_empty() || max == 0 {
            return ToolSelection {
                tools: vec![],
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        match self
            .try_select_tools_for_task_semantic(specs, task, max)
            .await
        {
            Ok(selection) => selection,
            Err(e) => {
                debug!("[mcp] semantic tool retrieval unavailable: {e}; falling back to lexical");
                Self::select_tools_for_task(specs, task, max)
            }
        }
    }

    async fn select_catalog_tools_best_effort<'a>(
        &self,
        entries: &'a [Arc<CachedMcpTool>],
        task: &str,
        max: usize,
    ) -> CatalogToolSelection<'a> {
        if entries.is_empty() || max == 0 {
            return CatalogToolSelection {
                tools: vec![],
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        match self
            .try_select_catalog_tools_semantic(entries, task, max)
            .await
        {
            Ok(selection) => selection,
            Err(e) => {
                debug!("[mcp] semantic tool retrieval unavailable: {e}; falling back to lexical");
                Self::select_catalog_tools(entries, task, max)
            }
        }
    }

    fn select_catalog_tools<'a>(
        entries: &'a [Arc<CachedMcpTool>],
        task: &str,
        max: usize,
    ) -> CatalogToolSelection<'a> {
        if entries.is_empty() || max == 0 {
            return CatalogToolSelection {
                tools: vec![],
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        let task_words = Self::task_words(task);
        if task_words.is_empty() {
            return CatalogToolSelection {
                tools: Self::select_diverse_catalog_tools(entries, max),
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        let mut scored: Vec<(usize, String, &CachedMcpTool)> = entries
            .iter()
            .map(|entry| {
                (
                    Self::catalog_tool_score(entry, &task_words),
                    entry.fn_name.clone(),
                    entry.as_ref(),
                )
            })
            .collect();
        let best_score = scored.iter().map(|(score, _, _)| *score).max().unwrap_or(0);
        if best_score == 0 {
            return CatalogToolSelection {
                tools: Self::select_diverse_catalog_tools(entries, max),
                best_lexical_score: 0,
                source: ToolSelectionSource::DiverseFallback,
            };
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        CatalogToolSelection {
            tools: scored
                .into_iter()
                .take(max)
                .map(|(_, _, entry)| entry)
                .collect(),
            best_lexical_score: best_score,
            source: ToolSelectionSource::Lexical,
        }
    }

    async fn try_select_tools_for_task_semantic<'a>(
        &self,
        specs: &'a [serde_json::Value],
        task: &str,
        max: usize,
    ) -> Result<ToolSelection<'a>> {
        if task.trim().is_empty() {
            bail!("empty task");
        }

        let embedder = self
            .embedder
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("embedder not configured"))?;

        let missing = {
            let cache = self.tool_embedding_cache.read().await;
            Self::count_missing_tool_embeddings(specs, &cache)
        };
        if missing > 0 {
            self.spawn_warm_tool_embeddings_for_specs(specs.to_vec());
            bail!(
                "tool embedding cache incomplete: {missing}/{} missing or stale",
                specs.len()
            );
        }

        let query_vector = Self::embed_text_with_timeout(embedder, task).await?;
        let cache = self.tool_embedding_cache.read().await;
        Self::select_tools_by_embedding_cache(specs, task, max, &query_vector, &cache)
    }

    fn select_tools_by_embedding_cache<'a>(
        specs: &'a [serde_json::Value],
        task: &str,
        max: usize,
        query_vector: &[f32],
        cache: &HashMap<String, ToolEmbedding>,
    ) -> Result<ToolSelection<'a>> {
        if query_vector.is_empty() {
            bail!("empty query embedding");
        }

        let task_words = Self::task_words(task);
        let mut rows: Vec<(f32, String, usize, &'a serde_json::Value)> = Vec::new();
        let mut best_lexical_score = 0;

        for spec in specs {
            let fn_name = Self::tool_name(spec);
            let doc = Self::tool_retrieval_document(spec);
            let doc_hash = Self::hash_text(&doc);
            let Some(embedding) = cache.get(fn_name) else {
                bail!("missing cached embedding for {fn_name}");
            };
            if embedding.doc_hash != doc_hash {
                bail!("stale cached embedding for {fn_name}");
            }
            if embedding.vector.len() != query_vector.len() {
                bail!(
                    "embedding dimension mismatch for {fn_name}: query={} tool={}",
                    query_vector.len(),
                    embedding.vector.len()
                );
            }

            let lexical_score = Self::tool_score(spec, &task_words);
            best_lexical_score = best_lexical_score.max(lexical_score);
            let semantic_score = Self::cosine_similarity(query_vector, &embedding.vector);
            rows.push((semantic_score, fn_name.to_string(), lexical_score, spec));
        }

        if rows.is_empty() {
            bail!("no semantic tool candidates");
        }

        rows.sort_by(|a, b| {
            let a_score = Self::hybrid_semantic_score(a.0, a.2, best_lexical_score);
            let b_score = Self::hybrid_semantic_score(b.0, b.2, best_lexical_score);
            b_score.total_cmp(&a_score).then_with(|| a.1.cmp(&b.1))
        });

        Ok(ToolSelection {
            tools: rows
                .into_iter()
                .take(max)
                .map(|(_, _, _, spec)| spec)
                .collect(),
            best_lexical_score,
            source: ToolSelectionSource::Semantic,
        })
    }

    fn hybrid_semantic_score(
        semantic_score: f32,
        lexical_score: usize,
        best_lexical_score: usize,
    ) -> f32 {
        let semantic = semantic_score.max(0.0);
        let lexical = if best_lexical_score == 0 {
            0.0
        } else {
            lexical_score as f32 / best_lexical_score as f32
        };
        semantic * 0.85 + lexical * 0.15
    }

    fn select_diverse_tools(specs: &[serde_json::Value], max: usize) -> Vec<&serde_json::Value> {
        let mut by_server: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
        for spec in specs {
            by_server
                .entry(Self::tool_server_name(spec))
                .or_default()
                .push(spec);
        }
        for tools in by_server.values_mut() {
            tools.sort_by(|a, b| Self::tool_name(a).cmp(Self::tool_name(b)));
        }

        let mut selected = Vec::new();
        loop {
            let before = selected.len();
            for tools in by_server.values_mut() {
                if selected.len() >= max {
                    return selected;
                }
                if !tools.is_empty() {
                    selected.push(tools.remove(0));
                }
            }
            if selected.len() == before {
                break;
            }
        }
        selected
    }

    fn select_diverse_catalog_tools(
        entries: &[Arc<CachedMcpTool>],
        max: usize,
    ) -> Vec<&CachedMcpTool> {
        let mut by_server: BTreeMap<String, Vec<&CachedMcpTool>> = BTreeMap::new();
        for entry in entries {
            by_server
                .entry(entry.raw.server.clone())
                .or_default()
                .push(entry.as_ref());
        }
        for tools in by_server.values_mut() {
            tools.sort_by(|a, b| a.fn_name.cmp(&b.fn_name));
        }

        let mut selected = Vec::new();
        loop {
            let before = selected.len();
            for tools in by_server.values_mut() {
                if selected.len() >= max {
                    return selected;
                }
                if !tools.is_empty() {
                    selected.push(tools.remove(0));
                }
            }
            if selected.len() == before {
                break;
            }
        }
        selected
    }

    fn task_words(task: &str) -> HashSet<String> {
        task.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(|w| w.to_lowercase())
            .collect()
    }

    fn tool_score(spec: &serde_json::Value, task_words: &HashSet<String>) -> usize {
        let fn_name = Self::tool_name(spec).to_lowercase();
        let description = Self::tool_description(spec).to_lowercase();
        let (parameter_names, parameter_descriptions) =
            Self::parameter_retrieval_text(&spec["function"]["parameters"]);

        Self::match_count(&fn_name, task_words) * 5
            + Self::match_count(&description, task_words) * 3
            + Self::match_count(&parameter_names, task_words) * 2
            + Self::match_count(&parameter_descriptions, task_words)
    }

    fn catalog_tool_score(entry: &CachedMcpTool, task_words: &HashSet<String>) -> usize {
        Self::match_count(&entry.fn_name.to_lowercase(), task_words) * 5
            + Self::match_count(&entry.raw.description.to_lowercase(), task_words) * 3
            + Self::match_count(&entry.parameter_names, task_words) * 2
            + Self::match_count(&entry.parameter_descriptions, task_words)
    }

    fn match_count(text: &str, task_words: &HashSet<String>) -> usize {
        task_words
            .iter()
            .filter(|w| text.contains(w.as_str()))
            .count()
    }

    fn parameter_retrieval_text(parameters: &serde_json::Value) -> (String, String) {
        let mut names = Vec::new();
        let mut descriptions = Vec::new();
        Self::collect_parameter_retrieval_text(parameters, &mut names, &mut descriptions);
        (
            names.join(" ").to_lowercase(),
            descriptions.join(" ").to_lowercase(),
        )
    }

    fn collect_parameter_retrieval_text(
        value: &serde_json::Value,
        names: &mut Vec<String>,
        descriptions: &mut Vec<String>,
    ) {
        let Some(object) = value.as_object() else {
            return;
        };

        if let Some(description) = object.get("description").and_then(|v| v.as_str()) {
            descriptions.push(description.to_string());
        }
        if let Some(title) = object.get("title").and_then(|v| v.as_str()) {
            descriptions.push(title.to_string());
        }

        if let Some(properties) = object.get("properties").and_then(|v| v.as_object()) {
            for (name, schema) in properties {
                names.push(name.clone());
                Self::collect_parameter_retrieval_text(schema, names, descriptions);
            }
        }

        for key in ["items", "oneOf", "anyOf", "allOf", "$defs", "definitions"] {
            match object.get(key) {
                Some(serde_json::Value::Array(items)) => {
                    for item in items {
                        Self::collect_parameter_retrieval_text(item, names, descriptions);
                    }
                }
                Some(next) => Self::collect_parameter_retrieval_text(next, names, descriptions),
                None => {}
            }
        }
    }

    fn tool_name(spec: &serde_json::Value) -> &str {
        spec["function"]["name"].as_str().unwrap_or("")
    }

    fn tool_description(spec: &serde_json::Value) -> &str {
        spec["function"]["description"].as_str().unwrap_or("")
    }

    fn tool_server_name(spec: &serde_json::Value) -> String {
        let description = Self::tool_description(spec);
        if description.starts_with('[') {
            if let Some(end) = description.find(']') {
                return description[1..end].to_string();
            }
        }

        Self::tool_name(spec)
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__").map(|(server, _)| server.to_string()))
            .unwrap_or_else(|| "unknown".to_string())
    }

    async fn try_select_catalog_tools_semantic<'a>(
        &self,
        entries: &'a [Arc<CachedMcpTool>],
        task: &str,
        max: usize,
    ) -> Result<CatalogToolSelection<'a>> {
        if task.trim().is_empty() {
            bail!("empty task");
        }

        let embedder = self
            .embedder
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("embedder not configured"))?;

        let missing = {
            let cache = self.tool_embedding_cache.read().await;
            entries
                .iter()
                .filter(|entry| {
                    !matches!(
                        cache.get(entry.fn_name.as_str()),
                        Some(existing) if existing.doc_hash == entry.doc_hash
                    )
                })
                .count()
        };
        if missing > 0 {
            self.spawn_warm_tool_embeddings_from_catalog_entries(entries.to_vec());
            bail!(
                "tool embedding cache incomplete: {missing}/{} missing or stale",
                entries.len()
            );
        }

        let query_vector = Self::embed_text_with_timeout(embedder, task).await?;
        let cache = self.tool_embedding_cache.read().await;
        Self::select_catalog_tools_by_embedding_cache(entries, task, max, &query_vector, &cache)
    }

    fn select_catalog_tools_by_embedding_cache<'a>(
        entries: &'a [Arc<CachedMcpTool>],
        task: &str,
        max: usize,
        query_vector: &[f32],
        cache: &HashMap<String, ToolEmbedding>,
    ) -> Result<CatalogToolSelection<'a>> {
        if query_vector.is_empty() {
            bail!("empty query embedding");
        }

        let task_words = Self::task_words(task);
        let mut rows: Vec<(f32, String, usize, &'a CachedMcpTool)> = Vec::new();
        let mut best_lexical_score = 0;

        for entry in entries {
            let Some(embedding) = cache.get(entry.fn_name.as_str()) else {
                bail!("missing cached embedding for {}", entry.fn_name);
            };
            if embedding.doc_hash != entry.doc_hash {
                bail!("stale cached embedding for {}", entry.fn_name);
            }
            if embedding.vector.len() != query_vector.len() {
                bail!(
                    "embedding dimension mismatch for {}: query={} tool={}",
                    entry.fn_name,
                    query_vector.len(),
                    embedding.vector.len()
                );
            }

            let lexical_score = Self::catalog_tool_score(entry, &task_words);
            best_lexical_score = best_lexical_score.max(lexical_score);
            let semantic_score = Self::cosine_similarity(query_vector, &embedding.vector);
            rows.push((semantic_score, entry.fn_name.clone(), lexical_score, entry));
        }

        if rows.is_empty() {
            bail!("no semantic tool candidates");
        }

        rows.sort_by(|a, b| {
            let a_score = Self::hybrid_semantic_score(a.0, a.2, best_lexical_score);
            let b_score = Self::hybrid_semantic_score(b.0, b.2, best_lexical_score);
            b_score.total_cmp(&a_score).then_with(|| a.1.cmp(&b.1))
        });

        Ok(CatalogToolSelection {
            tools: rows
                .into_iter()
                .take(max)
                .map(|(_, _, _, entry)| entry)
                .collect(),
            best_lexical_score,
            source: ToolSelectionSource::Semantic,
        })
    }

    async fn spawn_warm_tool_embeddings_from_cache(&self) {
        let entries = self.catalog_snapshot().await.entries.clone();
        self.spawn_warm_tool_embeddings_from_catalog_entries(entries);
    }

    fn spawn_warm_tool_embeddings_for_specs(&self, specs: Vec<serde_json::Value>) {
        if specs.is_empty() {
            return;
        }

        let orchestrator = self.clone();
        tokio::spawn(async move {
            if let Err(e) = orchestrator.warm_tool_embeddings_for_specs(&specs).await {
                debug!("[mcp] tool embedding warmup skipped: {e}");
            }
        });
    }

    fn spawn_warm_tool_embeddings_from_catalog_entries(&self, entries: Vec<Arc<CachedMcpTool>>) {
        if entries.is_empty() {
            return;
        }

        let orchestrator = self.clone();
        tokio::spawn(async move {
            if let Err(e) = orchestrator
                .warm_tool_embeddings_for_entries(&entries)
                .await
            {
                debug!("[mcp] tool embedding warmup skipped: {e}");
            }
        });
    }

    async fn warm_tool_embeddings_for_specs(&self, specs: &[serde_json::Value]) -> Result<()> {
        let embedder = self
            .embedder
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("embedder not configured"))?;

        let missing: Vec<(String, u64, String)> = {
            let cache = self.tool_embedding_cache.read().await;
            specs
                .iter()
                .filter_map(|spec| {
                    let fn_name = Self::tool_name(spec);
                    if fn_name.is_empty() {
                        return None;
                    }
                    let doc = Self::tool_retrieval_document(spec);
                    let doc_hash = Self::hash_text(&doc);
                    match cache.get(fn_name) {
                        Some(existing) if existing.doc_hash == doc_hash => None,
                        _ => Some((fn_name.to_string(), doc_hash, doc)),
                    }
                })
                .collect()
        };

        if missing.is_empty() {
            return Ok(());
        }

        debug!("[mcp] warming {} MCP tool embeddings", missing.len());
        for (fn_name, doc_hash, doc) in missing {
            match Self::embed_text_with_timeout(embedder.clone(), &doc).await {
                Ok(vector) if !vector.is_empty() => {
                    self.tool_embedding_cache
                        .write()
                        .await
                        .insert(fn_name, ToolEmbedding { doc_hash, vector });
                }
                Ok(_) => debug!("[mcp] empty embedding for tool {fn_name}"),
                Err(e) => debug!("[mcp] embedding failed for tool {fn_name}: {e}"),
            }
        }

        Ok(())
    }

    async fn warm_tool_embeddings_for_entries(&self, entries: &[Arc<CachedMcpTool>]) -> Result<()> {
        let embedder = self
            .embedder
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("embedder not configured"))?;

        let missing: Vec<(String, u64, String)> = {
            let cache = self.tool_embedding_cache.read().await;
            entries
                .iter()
                .filter_map(|entry| match cache.get(entry.fn_name.as_str()) {
                    Some(existing) if existing.doc_hash == entry.doc_hash => None,
                    _ => Some((
                        entry.fn_name.clone(),
                        entry.doc_hash,
                        entry.retrieval_doc.clone(),
                    )),
                })
                .collect()
        };

        if missing.is_empty() {
            return Ok(());
        }

        debug!("[mcp] warming {} MCP tool embeddings", missing.len());
        for (fn_name, doc_hash, doc) in missing {
            match Self::embed_text_with_timeout(embedder.clone(), &doc).await {
                Ok(vector) if !vector.is_empty() => {
                    self.tool_embedding_cache
                        .write()
                        .await
                        .insert(fn_name, ToolEmbedding { doc_hash, vector });
                }
                Ok(_) => debug!("[mcp] empty embedding for tool {fn_name}"),
                Err(e) => debug!("[mcp] embedding failed for tool {fn_name}: {e}"),
            }
        }

        Ok(())
    }

    fn count_missing_tool_embeddings(
        specs: &[serde_json::Value],
        cache: &HashMap<String, ToolEmbedding>,
    ) -> usize {
        specs
            .iter()
            .filter(|spec| {
                let fn_name = Self::tool_name(spec);
                let doc = Self::tool_retrieval_document(spec);
                let doc_hash = Self::hash_text(&doc);
                !matches!(cache.get(fn_name), Some(existing) if existing.doc_hash == doc_hash)
            })
            .count()
    }

    async fn embed_text_with_timeout(embedder: Arc<LlmClient>, text: &str) -> Result<Vec<f32>> {
        timeout(Duration::from_secs(6), embedder.embed(text))
            .await
            .map_err(|_| anyhow!("embedding request timed out"))?
    }

    fn tool_retrieval_document(spec: &serde_json::Value) -> String {
        let (parameter_names, parameter_descriptions) =
            Self::parameter_retrieval_text(&spec["function"]["parameters"]);
        format!(
            "name: {}\nserver: {}\ndescription: {}\nparameter_names: {}\nparameter_descriptions: {}",
            Self::tool_name(spec),
            Self::tool_server_name(spec),
            Self::tool_description(spec),
            parameter_names,
            parameter_descriptions
        )
    }

    fn hash_text(text: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    /// Return all cached remote tools as OpenAI-compatible function tool specs.
    ///
    /// Each tool is named `mcp__{server}__{tool}` (sanitized) so the executor can route it
    /// back to the right MCP server without ambiguity.
    pub async fn tools_as_openai_specs(&self) -> Vec<serde_json::Value> {
        self.catalog_snapshot().await.openai_specs.clone()
    }

    pub async fn refresh_catalog_aliases(&self) {
        self.rebuild_catalog_snapshot().await;
        self.spawn_warm_tool_embeddings_from_cache().await;
    }

    /// Resolve a sanitized fn_name (as returned by the LLM) back to the original
    /// (server, tool_name) pair needed to call the remote MCP server.
    pub async fn resolve_fn_name(&self, fn_name: &str) -> Option<(String, String)> {
        self.catalog_snapshot()
            .await
            .fn_name_map
            .get(fn_name)
            .cloned()
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
        Self::ensure_tool_allowed(&entry, tool)?;
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

    async fn catalog_snapshot(&self) -> Arc<McpCatalogSnapshot> {
        self.catalog.read().await.clone()
    }

    async fn replace_server_tools(&self, server_name: &str, tools: Vec<McpToolInfo>) {
        self.tool_cache
            .write()
            .await
            .insert(server_name.to_string(), tools);
        self.rebuild_catalog_snapshot().await;
        self.spawn_warm_tool_embeddings_from_cache().await;
    }

    async fn remove_server_tools(&self, server_name: &str) {
        self.tool_cache.write().await.remove(server_name);
        self.rebuild_catalog_snapshot().await;
    }

    async fn rebuild_catalog_snapshot(&self) {
        let cache = self.tool_cache.read().await.clone();
        let snapshot = Self::build_catalog_snapshot(&cache);
        self.prune_embedding_cache_for_snapshot(&snapshot).await;
        *self.catalog.write().await = Arc::new(snapshot);
    }

    async fn prune_embedding_cache_for_snapshot(&self, snapshot: &McpCatalogSnapshot) {
        let valid_hashes: HashMap<&str, u64> = snapshot
            .entries
            .iter()
            .map(|entry| (entry.fn_name.as_str(), entry.doc_hash))
            .collect();
        self.tool_embedding_cache
            .write()
            .await
            .retain(|fn_name, embedding| {
                valid_hashes
                    .get(fn_name.as_str())
                    .is_some_and(|hash| *hash == embedding.doc_hash)
            });
    }

    fn build_catalog_snapshot(cache: &HashMap<String, Vec<McpToolInfo>>) -> McpCatalogSnapshot {
        let finance_aliases = Self::finance_capability_alias_targets();
        let mut tools: Vec<McpToolInfo> = cache
            .values()
            .flat_map(|tool_list| tool_list.iter().cloned())
            .collect();
        tools.sort_by(|a, b| a.server.cmp(&b.server).then_with(|| a.name.cmp(&b.name)));

        let mut entries = Vec::with_capacity(tools.len());
        let mut by_fn_name = HashMap::with_capacity(tools.len());
        let mut fn_name_map = HashMap::with_capacity(tools.len());
        let mut openai_specs = Vec::with_capacity(tools.len());

        for tool in tools {
            let actual_server = tool.server.clone();
            let original_entry = Arc::new(Self::catalog_entry_from_tool(tool.clone(), &actual_server));
            openai_specs.push(original_entry.openai_spec.clone());
            fn_name_map.insert(
                original_entry.fn_name.clone(),
                (actual_server.clone(), original_entry.raw.name.clone()),
            );
            by_fn_name.insert(original_entry.fn_name.clone(), original_entry.clone());
            entries.push(original_entry);

            for alias in finance_aliases
                .iter()
                .filter(|alias| alias.actual_server == actual_server && alias.actual_tool == tool.name)
            {
                let alias_entry = Arc::new(Self::catalog_entry_from_alias(tool.clone(), alias));
                if by_fn_name.contains_key(&alias_entry.fn_name) {
                    continue;
                }
                openai_specs.push(alias_entry.openai_spec.clone());
                fn_name_map.insert(
                    alias_entry.fn_name.clone(),
                    (actual_server.clone(), alias_entry.raw.name.clone()),
                );
                by_fn_name.insert(alias_entry.fn_name.clone(), alias_entry.clone());
                entries.push(alias_entry);
            }
        }

        McpCatalogSnapshot {
            entries,
            by_fn_name,
            fn_name_map,
            openai_specs,
        }
    }

    fn catalog_entry_from_tool(tool: McpToolInfo, exposed_server: &str) -> CachedMcpTool {
        let exposed_tool_name = tool.name.clone();
        Self::catalog_entry_from_parts(tool, exposed_server, &exposed_tool_name, None)
    }

    fn catalog_entry_from_alias(tool: McpToolInfo, alias: &FinanceCapabilityAlias) -> CachedMcpTool {
        Self::catalog_entry_from_parts(
            tool,
            &alias.alias_server,
            &alias.alias_tool,
            Some(alias.description.as_str()),
        )
    }

    fn catalog_entry_from_parts(
        tool: McpToolInfo,
        exposed_server: &str,
        exposed_tool_name: &str,
        alias_description: Option<&str>,
    ) -> CachedMcpTool {
        let fn_name = format!(
            "mcp__{}__{}",
            sanitize_tool_name(exposed_server),
            sanitize_tool_name(exposed_tool_name)
        );
        let schema = Self::normalize_input_schema(&tool.input_schema);
        let description = if let Some(alias_description) = alias_description {
            format!(
                "[{}→{}.{}] {}",
                exposed_server, tool.server, tool.name, alias_description
            )
        } else if exposed_server == tool.server {
            format!("[{}] {}", tool.server, tool.description)
        } else {
            format!("[{}→{}] {}", exposed_server, tool.server, tool.description)
        };
        let openai_spec = serde_json::json!({
          "type": "function",
          "function": {
            "name": fn_name,
            "description": description,
            "parameters": schema,
          }
        });
        let (parameter_names, parameter_descriptions) =
            Self::parameter_retrieval_text(&openai_spec["function"]["parameters"]);
        let retrieval_doc = format!(
            "name: {}\nserver: {}\ndescription: {}\nparameter_names: {}\nparameter_descriptions: {}",
            openai_spec["function"]["name"].as_str().unwrap_or(""),
            tool.server,
            openai_spec["function"]["description"].as_str().unwrap_or(""),
            parameter_names,
            parameter_descriptions
        );
        let doc_hash = Self::hash_text(&retrieval_doc);

        CachedMcpTool {
            raw: tool,
            fn_name,
            openai_spec,
            retrieval_doc,
            parameter_names,
            parameter_descriptions,
            doc_hash,
        }
    }

    fn finance_capability_alias_targets() -> Vec<FinanceCapabilityAlias> {
        crate::config::load_fast()
            .scenario
            .finance
            .connector_bindings
            .into_iter()
            .flat_map(|binding| {
                let Some(server_name) = binding.server_name else {
                    return Vec::new();
                };
                binding
                    .capability_bindings
                    .into_iter()
                    .filter_map(move |capability| {
                        let tool_name = capability.tool_name?;
                        if capability.capability.trim().is_empty() || tool_name.trim().is_empty() {
                            return None;
                        }
                        Some(FinanceCapabilityAlias {
                            actual_server: server_name.clone(),
                            actual_tool: tool_name.clone(),
                            alias_server: binding.connector.clone(),
                            alias_tool: capability.capability.clone(),
                            description: Self::finance_capability_description(
                                &binding.connector,
                                &capability.capability,
                            )
                            .unwrap_or_else(|| format!("Finance capability alias for {}", capability.capability)),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn finance_capability_description(connector: &str, capability: &str) -> Option<String> {
        let description = match (connector, capability) {
            ("intel", "search_news") => "Research news, filings, and catalyst flow".to_string(),
            ("intel", "get_quote") => "Fetch the latest quote and top-of-book snapshot".to_string(),
            ("intel", "get_candles") => "Load recent candles or bars for validation and review".to_string(),
            ("intel", "screen_symbols") => "Screen symbols by market conditions or filters".to_string(),
            ("intel", "list_watchlist") => "Read the current watchlist".to_string(),
            ("intel", "add_watchlist") => "Add symbols to the watchlist".to_string(),
            ("intel", "remove_watchlist") => "Remove symbols from the watchlist".to_string(),
            ("risk", "review_trade_intent") => "Approve, reject, or reduce a TradeIntent".to_string(),
            ("risk", "validate_order") => "Validate order-level constraints before execution".to_string(),
            ("risk", "validate_positions") => "Inspect current positions for risk checks".to_string(),
            ("risk", "validate_market_status") => "Check market status, trading window, and venue constraints".to_string(),
            ("risk", "validate_exposure") => "Check aggregate exposure such as single-name and sector caps".to_string(),
            ("exec", "get_account") => "Read account summary and buying power".to_string(),
            ("exec", "get_positions") => "Read current holdings and exposure".to_string(),
            ("exec", "list_orders") => "Read live and recent orders".to_string(),
            ("exec", "get_order_status") => "Inspect a specific order status".to_string(),
            ("exec", "place_order") => "Submit a broker order for execution".to_string(),
            ("exec", "cancel_order") => "Cancel an existing broker order".to_string(),
            _ => return None,
        };
        Some(description)
    }

    fn normalize_input_schema(schema: &serde_json::Value) -> serde_json::Value {
        if schema.is_null() {
            return serde_json::json!({ "type": "object", "properties": {} });
        }

        let mut normalized = schema.clone();
        if let Some(object) = normalized.as_object_mut() {
            let is_object_schema = object
                .get("type")
                .and_then(|value| match value {
                    serde_json::Value::String(kind) => Some(kind == "object"),
                    serde_json::Value::Array(kinds) => Some(
                        kinds
                            .iter()
                            .any(|kind| kind.as_str().is_some_and(|kind| kind == "object")),
                    ),
                    _ => None,
                })
                .unwrap_or(false);

            if is_object_schema && !object.contains_key("properties") {
                object.insert("properties".into(), serde_json::json!({}));
            }
        }

        normalized
    }

    fn normalize_server_entry(mut entry: McpServerEntry) -> McpServerEntry {
        let mut seen = HashSet::new();
        entry.allowed_tools = entry
            .allowed_tools
            .into_iter()
            .map(|tool| tool.trim().to_string())
            .filter(|tool| !tool.is_empty())
            .filter(|tool| seen.insert(tool.clone()))
            .collect();
        entry
    }

    fn filter_tools_by_allowlist(
        entry: &McpServerEntry,
        tools: Vec<McpToolInfo>,
    ) -> Vec<McpToolInfo> {
        if entry.allowed_tools.is_empty() {
            return tools;
        }

        let allowed: HashSet<&str> = entry
            .allowed_tools
            .iter()
            .map(|tool| tool.as_str())
            .collect();
        let total = tools.len();
        let filtered: Vec<McpToolInfo> = tools
            .into_iter()
            .filter(|tool| allowed.contains(tool.name.as_str()))
            .collect();

        debug!(
            "[mcp] allowlist '{}' retained {} of {} discovered tools",
            entry.name,
            filtered.len(),
            total
        );

        filtered
    }

    fn ensure_tool_allowed(entry: &McpServerEntry, tool_name: &str) -> Result<()> {
        if entry.allowed_tools.is_empty()
            || entry.allowed_tools.iter().any(|tool| tool == tool_name)
        {
            return Ok(());
        }

        bail!(
            "MCP tool '{}' is not allowlisted for server '{}'",
            tool_name,
            entry.name
        )
    }
}

impl Default for MCPOrchestrator {
    fn default() -> Self {
        Self::new()
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
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Value::String(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn spec(name: &str, description: &str, parameters: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters,
            }
        })
    }

    #[test]
    fn tools_for_task_scores_parameter_names_and_descriptions() {
        let orchestrator = MCPOrchestrator::new();
        let specs = vec![
            spec(
                "mcp__chat__send_message",
                "[chat] Send a message",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string" },
                        "text": { "type": "string" }
                    }
                }),
            ),
            spec(
                "mcp__database__run",
                "[database] Execute a query",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL statement to execute"
                        }
                    }
                }),
            ),
        ];

        let selected = orchestrator.tools_for_task_from_specs(&specs, "run sql", 1);

        assert_eq!(
            selected[0]["function"]["name"].as_str(),
            Some("mcp__database__run")
        );
    }

    #[test]
    fn tools_for_task_uses_server_diversity_when_scores_are_zero() {
        let orchestrator = MCPOrchestrator::new();
        let specs = vec![
            spec(
                "mcp__alpha__second",
                "[alpha] Second alpha tool",
                serde_json::json!({ "type": "object", "properties": {} }),
            ),
            spec(
                "mcp__alpha__first",
                "[alpha] First alpha tool",
                serde_json::json!({ "type": "object", "properties": {} }),
            ),
            spec(
                "mcp__beta__second",
                "[beta] Second beta tool",
                serde_json::json!({ "type": "object", "properties": {} }),
            ),
            spec(
                "mcp__beta__first",
                "[beta] First beta tool",
                serde_json::json!({ "type": "object", "properties": {} }),
            ),
        ];

        let selected = orchestrator.tools_for_task_from_specs(&specs, "完全无关", 2);
        let names: Vec<&str> = selected
            .iter()
            .filter_map(|s| s["function"]["name"].as_str())
            .collect();

        assert_eq!(names, vec!["mcp__alpha__first", "mcp__beta__first"]);
    }

    #[test]
    fn semantic_selection_can_rank_without_lexical_overlap() {
        let specs = vec![
            spec(
                "mcp__browser__open",
                "[browser] Open a URL",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" }
                    }
                }),
            ),
            spec(
                "mcp__git__diff",
                "[git] Show unstaged changes in the working tree",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
            ),
        ];
        let mut cache = HashMap::new();
        for spec in &specs {
            let doc = MCPOrchestrator::tool_retrieval_document(spec);
            let vector = if MCPOrchestrator::tool_name(spec) == "mcp__git__diff" {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            };
            cache.insert(
                MCPOrchestrator::tool_name(spec).to_string(),
                ToolEmbedding {
                    doc_hash: MCPOrchestrator::hash_text(&doc),
                    vector,
                },
            );
        }

        let selection = MCPOrchestrator::select_tools_by_embedding_cache(
            &specs,
            "查看未缓存的修改",
            1,
            &[1.0, 0.0],
            &cache,
        )
        .unwrap();

        assert_eq!(selection.source, ToolSelectionSource::Semantic);
        assert_eq!(
            selection.tools[0]["function"]["name"].as_str(),
            Some("mcp__git__diff")
        );
    }

    #[tokio::test]
    async fn best_effort_falls_back_to_lexical_without_embedder() {
        let orchestrator = MCPOrchestrator::new();
        let specs = vec![
            spec(
                "mcp__chat__send_message",
                "[chat] Send a message",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "channel": { "type": "string" }
                    }
                }),
            ),
            spec(
                "mcp__database__run",
                "[database] Execute a query",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL statement to execute"
                        }
                    }
                }),
            ),
        ];

        let selected = orchestrator
            .tools_for_task_best_effort_from_specs(&specs, "run sql", 1)
            .await;

        assert_eq!(
            selected[0]["function"]["name"].as_str(),
            Some("mcp__database__run")
        );
    }

    #[tokio::test]
    async fn catalog_snapshot_serves_schema_and_dispatch_views() {
        let orchestrator = MCPOrchestrator::new();
        orchestrator
            .replace_server_tools(
                "git",
                vec![McpToolInfo {
                    server: "git".into(),
                    name: "diff".into(),
                    description: "Show unstaged changes".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Repository path" }
                        }
                    }),
                }],
            )
            .await;

        let fn_name = "mcp__git__diff";
        let schema = orchestrator.deferred_tool_schema(fn_name).await.unwrap();
        let specs = orchestrator.tools_as_openai_specs().await;
        let resolved = orchestrator.resolve_fn_name(fn_name).await;

        assert_eq!(specs.len(), 1);
        assert_eq!(schema["function"]["name"].as_str(), Some(fn_name));
        assert_eq!(resolved, Some(("git".into(), "diff".into())));
        assert_eq!(
            schema["function"]["parameters"]["properties"]["path"]["description"].as_str(),
            Some("Repository path")
        );
    }

    #[tokio::test]
    async fn catalog_snapshot_prunes_stale_embedding_cache_entries() {
        let orchestrator = MCPOrchestrator::new();
        let before = spec(
            "mcp__git__diff",
            "[git] Show changes",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        );
        let after = spec(
            "mcp__git__diff",
            "[git] Show changes",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Repository path"
                    }
                }
            }),
        );
        let stale_hash =
            MCPOrchestrator::hash_text(&MCPOrchestrator::tool_retrieval_document(&before));
        let fresh_hash =
            MCPOrchestrator::hash_text(&MCPOrchestrator::tool_retrieval_document(&after));

        orchestrator.tool_embedding_cache.write().await.insert(
            "mcp__git__diff".into(),
            ToolEmbedding {
                doc_hash: stale_hash,
                vector: vec![1.0, 2.0],
            },
        );

        orchestrator
            .replace_server_tools(
                "git",
                vec![McpToolInfo {
                    server: "git".into(),
                    name: "diff".into(),
                    description: "Show changes".into(),
                    input_schema: after["function"]["parameters"].clone(),
                }],
            )
            .await;

        let cache = orchestrator.tool_embedding_cache.read().await;
        assert_ne!(stale_hash, fresh_hash);
        assert!(cache.get("mcp__git__diff").is_none());
    }

    #[tokio::test]
    async fn catalog_snapshot_normalizes_object_schema_without_properties() {
        let orchestrator = MCPOrchestrator::new();
        orchestrator
            .replace_server_tools(
                "jin10",
                vec![McpToolInfo {
                    server: "jin10".into(),
                    name: "list_calendar".into(),
                    description: "Get this week's calendar".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "additionalProperties": false
                    }),
                }],
            )
            .await;

        let schema = orchestrator
            .deferred_tool_schema("mcp__jin10__list_calendar")
            .await
            .unwrap();

        assert_eq!(
            schema["function"]["parameters"],
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn tool_retrieval_document_hash_changes_when_schema_changes() {
        let before = spec(
            "mcp__git__diff",
            "[git] Show changes",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        );
        let after = spec(
            "mcp__git__diff",
            "[git] Show changes",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Repository path"
                    }
                }
            }),
        );

        let before_hash =
            MCPOrchestrator::hash_text(&MCPOrchestrator::tool_retrieval_document(&before));
        let after_hash =
            MCPOrchestrator::hash_text(&MCPOrchestrator::tool_retrieval_document(&after));

        assert_ne!(before_hash, after_hash);
    }

    #[test]
    fn allowlist_filters_discovered_tools() {
        let entry = McpServerEntry {
            name: "intel".into(),
            base_url: "http://localhost:9001/mcp".into(),
            token: String::new(),
            description: "intel".into(),
            enabled: true,
            allowed_tools: vec!["query_data".into(), "screen_stocks".into()],
        };
        let tools = vec![
            McpToolInfo {
                server: "intel".into(),
                name: "query_data".into(),
                description: String::new(),
                input_schema: serde_json::json!({}),
            },
            McpToolInfo {
                server: "intel".into(),
                name: "search_news".into(),
                description: String::new(),
                input_schema: serde_json::json!({}),
            },
        ];

        let filtered = MCPOrchestrator::filter_tools_by_allowlist(&entry, tools);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "query_data");
    }

    #[test]
    fn ensure_tool_allowed_rejects_non_allowlisted_tool() {
        let entry = McpServerEntry {
            name: "exec".into(),
            base_url: "http://localhost:9003/mcp".into(),
            token: String::new(),
            description: "exec".into(),
            enabled: true,
            allowed_tools: vec!["get_balance".into()],
        };

        let err = MCPOrchestrator::ensure_tool_allowed(&entry, "place_order").unwrap_err();

        assert!(err
            .to_string()
            .contains("is not allowlisted for server 'exec'"));
    }
}
