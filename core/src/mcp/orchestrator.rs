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

impl MCPOrchestrator {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            fn_name_map: Arc::new(RwLock::new(HashMap::new())),
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
                .map(|e| (e.name.clone(), e))
                .collect()
        } else {
            HashMap::new()
        };

        let orchestrator = Self {
            servers: Arc::new(RwLock::new(servers.clone())),
            tool_cache: Arc::new(RwLock::new(HashMap::new())),
            fn_name_map: Arc::new(RwLock::new(HashMap::new())),
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
                        self_clone
                            .tool_cache
                            .write()
                            .await
                            .insert(entry.name.clone(), tools);
                        self_clone.spawn_warm_tool_embeddings_from_cache().await;
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
                    self_clone.spawn_warm_tool_embeddings_from_cache().await;
                }
                Err(e) => warn!("[mcp] auto-discovery for '{}' failed: {e}", entry.name),
            }
        });
        Ok(())
    }

    pub async fn update_server(&self, name: &str, entry: McpServerEntry) -> Result<()> {
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
        self.tool_cache.write().await.remove(name);
        self.tool_cache.write().await.remove(&entry.name);
        self.persist().await?;
        if entry.enabled {
            let self_clone = self.clone();
            tokio::spawn(async move {
                match self_clone.discover_server_tools(&entry).await {
                    Ok(tools) => {
                        self_clone
                            .tool_cache
                            .write()
                            .await
                            .insert(entry.name.clone(), tools);
                        self_clone.spawn_warm_tool_embeddings_from_cache().await;
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
                        self_clone.spawn_warm_tool_embeddings_from_cache().await;
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
                        self.spawn_warm_tool_embeddings_from_cache().await;
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
        let all_specs = self.tools_as_openai_specs().await;
        self.deferred_tool_catalog_best_effort_from_specs(&all_specs, task, max)
            .await
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
    /// Otherwise we score each tool by weighted keyword overlap between `task`
    /// and the tool's name, description, parameter names, and parameter
    /// descriptions, then return the top `max` hits. If nothing matches, we
    /// choose a deterministic server-diverse fallback instead of trusting cache
    /// iteration order.
    /// This keeps the context window tight and avoids hitting provider tool limits.
    pub async fn tools_for_task(&self, task: &str, max: usize) -> Vec<serde_json::Value> {
        let all = self.tools_as_openai_specs().await;
        self.tools_for_task_best_effort_from_specs(&all, task, max)
            .await
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

    async fn spawn_warm_tool_embeddings_from_cache(&self) {
        let specs = self.tools_as_openai_specs().await;
        self.spawn_warm_tool_embeddings_for_specs(specs);
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
}
