use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{anyhow, Result};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Shared abort flag passed into `chat_stream`.
pub type AbortFlag = Arc<AtomicBool>;

// ── Embed response types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}
#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

// ── Token usage tracking ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Thread-safe cumulative token counters.
#[derive(Debug, Default)]
pub struct CumulativeUsage {
    pub prompt_tokens: AtomicU64,
    pub completion_tokens: AtomicU64,
    pub total_tokens: AtomicU64,
}

impl CumulativeUsage {
    fn accumulate(&self, usage: &TokenUsage) {
        self.prompt_tokens
            .fetch_add(usage.prompt_tokens, Ordering::Relaxed);
        self.completion_tokens
            .fetch_add(usage.completion_tokens, Ordering::Relaxed);
        self.total_tokens
            .fetch_add(usage.total_tokens, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.prompt_tokens.load(Ordering::Relaxed),
            completion_tokens: self.completion_tokens.load(Ordering::Relaxed),
            total_tokens: self.total_tokens.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.prompt_tokens.store(0, Ordering::Relaxed);
        self.completion_tokens.store(0, Ordering::Relaxed);
        self.total_tokens.store(0, Ordering::Relaxed);
    }
}

// ── Public message type ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

// ── Tool-call types (used by chat_with_tools) ────────────────────────────────

/// A single tool call request returned by the LLM.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Response from a non-streaming tool-capable chat round.
pub struct ChatWithToolsResponse {
    /// Text content when the model chose to reply directly.
    pub content: Option<String>,
    /// Tool calls requested by the model (empty when the model replied with text).
    pub tool_calls: Vec<ToolCall>,
    /// The raw assistant message JSON to append to the conversation history.
    pub raw_message: serde_json::Value,
}

// ── Provider ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Ollama,
}

// ── Internal config ─────────────────────────────────────────────────────────

const CACHE_CAPACITY: usize = 64;

#[derive(Clone)]
struct LlmInnerConfig {
    provider: Provider,
    model: String,
    mini_model: String,
    embed_model: String,
    api_key: Option<String>,
    api_base: Option<String>,
}

// ── LlmClient ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LlmClient {
    config: Arc<RwLock<LlmInnerConfig>>,
    cache: Arc<Mutex<VecDeque<(String, String)>>>,
    /// Shared reqwest client — reuses connection pools across calls.
    http: reqwest::Client,
    /// Cumulative token usage across all API calls in this session.
    pub usage: Arc<CumulativeUsage>,
    /// Usage from the most recent API call (for per-pup tracking).
    last_call_usage: Arc<Mutex<Option<TokenUsage>>>,
    /// Local embedding fallback (fastembed) — lazy-initialized on first API failure.
    local_embedder: Arc<super::local_embed::LocalEmbedder>,
}

impl LlmClient {
    pub fn new_from_env() -> Self {
        let provider = match std::env::var("OPENPUP_LLM_PROVIDER")
            .unwrap_or_else(|_| "openai".to_string())
            .to_lowercase()
            .as_str()
        {
            "ollama" => Provider::Ollama,
            _ => Provider::OpenAI,
        };

        let (model, mini_model, embed_model, api_key, api_base) = match provider {
            Provider::OpenAI => {
                let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
                let mini_model =
                    std::env::var("OPENAI_MINI_MODEL").unwrap_or_else(|_| model.clone());
                let embed_model = std::env::var("OPENAI_EMBED_MODEL")
                    .unwrap_or_else(|_| "BAAI/bge-m3".to_string());
                let api_key = std::env::var("OPENAI_API_KEY")
                    .ok()
                    .or_else(|| std::env::var("OPENPUP_API_KEY").ok());
                let api_base = std::env::var("OPENAI_BASE_URL").ok();
                (model, mini_model, embed_model, api_key, api_base)
            }
            Provider::Ollama => {
                let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3".to_string());
                let mini_model =
                    std::env::var("OLLAMA_MINI_MODEL").unwrap_or_else(|_| model.clone());
                let embed_model = std::env::var("OLLAMA_EMBED_MODEL")
                    .unwrap_or_else(|_| "nomic-embed-text".to_string());
                let api_base = Some(
                    std::env::var("OLLAMA_BASE_URL")
                        .unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
                );
                (model, mini_model, embed_model, None, api_base)
            }
        };

        Self {
            config: Arc::new(RwLock::new(LlmInnerConfig {
                provider,
                model,
                mini_model,
                embed_model,
                api_key,
                api_base,
            })),
            cache: Arc::new(Mutex::new(VecDeque::new())),
            http: reqwest::Client::new(),
            usage: Arc::new(CumulativeUsage::default()),
            last_call_usage: Arc::new(Mutex::new(None)),
            local_embedder: Arc::new(super::local_embed::LocalEmbedder::new()),
        }
    }

    /// Returns the token usage from the most recent API call.
    pub fn take_last_call_usage(&self) -> Option<TokenUsage> {
        self.last_call_usage.lock().unwrap().take()
    }

    /// Returns the currently configured model name.
    pub fn model_name(&self) -> String {
        self.config.read().unwrap().model.clone()
    }

    pub fn provider(&self) -> Provider {
        self.config.read().unwrap().provider
    }

    pub fn reconfigure(
        &self,
        provider: Provider,
        model: String,
        mini_model: Option<String>,
        embed_model: Option<String>,
        api_key: Option<String>,
        api_base: Option<String>,
    ) {
        let mut g = self.config.write().unwrap();
        g.provider = provider;
        if let Some(mm) = mini_model {
            g.mini_model = mm;
        }
        if let Some(em) = embed_model {
            g.embed_model = em;
        }
        g.model = model;
        if let Some(k) = api_key {
            g.api_key = Some(k);
        }
        if let Some(b) = api_base {
            g.api_base = Some(b);
        }
    }

    pub fn current_config(
        &self,
    ) -> (
        Provider,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) {
        let g = self.config.read().unwrap();
        (
            g.provider,
            g.model.clone(),
            g.mini_model.clone(),
            g.embed_model.clone(),
            g.api_key.clone(),
            g.api_base.clone(),
        )
    }

    // ── Public chat API ───────────────────────────────────────────────────────

    pub async fn chat(&self, messages: Vec<LlmMessage>) -> Result<String> {
        let model = self.config.read().unwrap().model.clone();
        self.chat_with_model(&model, messages).await
    }

    pub async fn chat_mini(&self, messages: Vec<LlmMessage>) -> Result<String> {
        let model = {
            let g = self.config.read().unwrap();
            if g.mini_model.is_empty() {
                g.model.clone()
            } else {
                g.mini_model.clone()
            }
        };
        self.chat_with_model(&model, messages).await
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<LlmMessage>,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        let model = self.config.read().unwrap().model.clone();
        self.stream_with_model(&model, messages, on_token, abort)
            .await
    }

    pub async fn chat_stream_mini(
        &self,
        messages: Vec<LlmMessage>,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        let model = {
            let g = self.config.read().unwrap();
            if g.mini_model.is_empty() {
                g.model.clone()
            } else {
                g.mini_model.clone()
            }
        };
        self.stream_with_model(&model, messages, on_token, abort)
            .await
    }

    // ── Non-streaming (direct reqwest) ────────────────────────────────────────

    async fn chat_with_model(&self, model: &str, messages: Vec<LlmMessage>) -> Result<String> {
        let cache_key = format!("{model}:{}", serde_json::to_string(&messages)?);
        if let Some(hit) = self.lookup_cache(&cache_key) {
            return Ok(hit);
        }

        let (api_key, api_base) = {
            let g = self.config.read().unwrap();
            (g.api_key.clone(), g.api_base.clone())
        };

        let url = chat_url(&api_base);
        debug!("[llm] chat: model={model:?} url={url}");

        let body = serde_json::json!({
          "model": model,
          "messages": messages_json(&messages),
        });

        let mut req = self.http.post(&url).json(&body);
        if let Some(k) = &api_key {
            req = req.bearer_auth(k);
        }

        let resp = req.send().await.map_err(|e| {
            debug!("[llm] chat send error: {e}");
            anyhow!("request failed: {e}")
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            debug!("[llm] chat API error {status}: {body}");
            return Err(anyhow!("API error {status}: {body}"));
        }

        let val: serde_json::Value = resp.json().await.map_err(|e| anyhow!("parse: {e}"))?;
        let text = val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Track token usage from the response
        if let Some(u) = parse_usage(&val) {
            self.usage.accumulate(&u);
            *self.last_call_usage.lock().unwrap() = Some(u);
        }

        debug!("[llm] chat done: {} chars", text.len());
        self.insert_cache(cache_key, text.clone());
        Ok(text)
    }

    // ── Streaming (direct reqwest + SSE parser) ────────────────────────────────

    async fn stream_with_model(
        &self,
        model: &str,
        messages: Vec<LlmMessage>,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        let (api_key, api_base) = {
            let g = self.config.read().unwrap();
            debug!(
                "[llm] stream: model={model:?} base={:?} has_key={}",
                g.api_base,
                g.api_key.is_some()
            );
            (g.api_key.clone(), g.api_base.clone())
        };

        let url = chat_url(&api_base);
        let body = serde_json::json!({
          "model": model,
          "messages": messages_json(&messages),
          "stream": true,
          "stream_options": { "include_usage": true },
        });

        let mut req = self.http.post(&url).json(&body);
        if let Some(k) = &api_key {
            req = req.bearer_auth(k);
        }

        let resp = req.send().await.map_err(|e| {
            debug!("[llm] stream send error: {e}");
            anyhow!("stream request failed: {e}")
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            debug!("[llm] stream API error {status}: {body}");
            return Err(anyhow!("API error {status}: {body}"));
        }

        debug!("[llm] stream opened, reading SSE…");
        let mut byte_stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut full = String::new();
        let mut chunk_count: usize = 0;
        let mut stream_usage: Option<TokenUsage> = None;

        'outer: loop {
            // Timeout so the abort flag is checked even while waiting for the next byte chunk.
            let next =
                tokio::time::timeout(std::time::Duration::from_millis(200), byte_stream.next())
                    .await;

            if abort.load(Ordering::Relaxed) {
                debug!(
                    "[llm] stream aborted after {chunk_count} tokens ({} chars)",
                    full.len()
                );
                break;
            }

            match next {
                Err(_timeout) => continue,
                Ok(None) => {
                    debug!(
                        "[llm] stream complete: {chunk_count} tokens, {} chars",
                        full.len()
                    );
                    break;
                }
                Ok(Some(bytes_result)) => {
                    let bytes = bytes_result.map_err(|e| {
                        debug!("[llm] stream read error: {e}");
                        anyhow!("stream read error: {e}")
                    })?;
                    buf.push_str(&String::from_utf8_lossy(&bytes));

                    // Process all complete SSE lines in the buffer.
                    while let Some(pos) = buf.find('\n') {
                        let line = buf[..pos].trim_end_matches('\r').to_string();
                        buf.drain(..=pos);

                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let data = &line["data: ".len()..];
                        if data == "[DONE]" {
                            break 'outer;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            // Surface API-level errors embedded in SSE (some providers send these)
                            if let Some(err) = val.get("error") {
                                debug!("[llm] SSE error payload: {err}");
                                return Err(anyhow!("API error in stream: {err}"));
                            }
                            // Capture usage from the final chunk (sent when stream_options.include_usage is true)
                            if let Some(u) = parse_usage(&val) {
                                stream_usage = Some(u);
                            }
                            let delta = &val["choices"][0]["delta"];
                            // Standard content token
                            if let Some(tok) = delta["content"].as_str() {
                                if !tok.is_empty() {
                                    chunk_count += 1;
                                    full.push_str(tok);
                                    on_token(tok, false);
                                }
                            }
                            // DeepSeek Reasoner: reasoning tokens come in `reasoning_content`.
                            // Passed with is_reasoning=true so callers can handle them separately.
                            if let Some(tok) = delta["reasoning_content"].as_str() {
                                if !tok.is_empty() {
                                    chunk_count += 1;
                                    on_token(tok, true);
                                    // Do NOT append to `full` — reasoning is not the final answer.
                                }
                            }
                        }
                    }
                }
            }
        }

        // Accumulate token usage if the API provided it
        if let Some(ref u) = stream_usage {
            debug!(
                "[llm] stream usage: prompt={} completion={} total={}",
                u.prompt_tokens, u.completion_tokens, u.total_tokens
            );
            self.usage.accumulate(u);
            *self.last_call_usage.lock().unwrap() = Some(u.clone());
        }

        if full.is_empty() && !abort.load(Ordering::Relaxed) {
            warn!("[llm] WARNING: empty response from model={model:?}");
            return Err(anyhow!(
                "LLM returned an empty response.\n\
         • Model: {model}\n\
         • Verify the model name and api_key in ~/.openpup/config.toml"
            ));
        }

        Ok(full)
    }

    // ── Tool-call API (non-streaming) ─────────────────────────────────────────

    /// Send a message with tool definitions; returns the model's text reply OR
    /// a list of tool calls to execute.  Messages are raw `serde_json::Value`
    /// so callers can include `role:"tool"` entries that `LlmMessage` can't.
    pub async fn chat_with_tools(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<ChatWithToolsResponse> {
        let (api_key, api_base, model) = {
            let g = self.config.read().unwrap();
            (g.api_key.clone(), g.api_base.clone(), g.model.clone())
        };

        let url = chat_url(&api_base);
        debug!(
            "[llm] chat_with_tools: model={model:?} tools={}",
            tools.len()
        );

        let body = serde_json::json!({
          "model": model,
          "messages": messages,
          "tools": tools,
          "tool_choice": "auto",
        });

        let mut req = self.http.post(&url).json(&body);
        if let Some(k) = &api_key {
            req = req.bearer_auth(k);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("chat_with_tools request: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            debug!("[llm] chat_with_tools error {status}: {body}");
            return Err(anyhow!("API error {status}: {body}"));
        }

        let val: serde_json::Value = resp.json().await.map_err(|e| anyhow!("parse: {e}"))?;

        // Track token usage
        if let Some(u) = parse_usage(&val) {
            self.usage.accumulate(&u);
            *self.last_call_usage.lock().unwrap() = Some(u);
        }

        let message = val["choices"][0]["message"].clone();

        let content = message["content"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let tool_calls: Vec<ToolCall> = message["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let id = tc["id"].as_str()?.to_string();
                        let name = tc["function"]["name"].as_str()?.to_string();
                        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(args_str)
                            .unwrap_or(serde_json::Value::Object(Default::default()));
                        Some(ToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        debug!(
            "[llm] chat_with_tools done: text={} tool_calls={}",
            content.is_some(),
            tool_calls.len()
        );

        Ok(ChatWithToolsResponse {
            content,
            tool_calls,
            raw_message: message,
        })
    }

    /// Same as `chat_with_tools` but cancels the HTTP request if `abort` is set.
    /// Polls the abort flag every 100 ms via `tokio::select!`.
    pub async fn chat_with_tools_abortable(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        abort: &AbortFlag,
    ) -> anyhow::Result<Option<ChatWithToolsResponse>> {
        let abort_clone = abort.clone();
        tokio::select! {
            result = self.chat_with_tools(messages, tools) => {
                Ok(Some(result?))
            }
            _ = async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if abort_clone.load(Ordering::Relaxed) {
                        break;
                    }
                }
            } => {
                debug!("[llm] chat_with_tools cancelled by abort flag");
                Ok(None)
            }
        }
    }

    // ── Streaming tool-call API ────────────────────────────────────────────────

    /// Streaming variant of `chat_with_tools`.  Text tokens are emitted
    /// incrementally via `on_token` while tool-call fragments are accumulated
    /// internally.  Returns the same `ChatWithToolsResponse` as the
    /// non-streaming version once the stream finishes.
    pub async fn chat_with_tools_stream(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        on_token: impl Fn(&str) + Send,
        abort: &AbortFlag,
    ) -> anyhow::Result<Option<ChatWithToolsResponse>> {
        let (api_key, api_base, model) = {
            let g = self.config.read().unwrap();
            (g.api_key.clone(), g.api_base.clone(), g.model.clone())
        };

        let url = chat_url(&api_base);
        debug!(
            "[llm] chat_with_tools_stream: model={model:?} tools={}",
            tools.len()
        );

        let body = serde_json::json!({
          "model": model,
          "messages": messages,
          "tools": tools,
          "tool_choice": "auto",
          "stream": true,
          "stream_options": { "include_usage": true },
        });

        let mut req = self.http.post(&url).json(&body);
        if let Some(k) = &api_key {
            req = req.bearer_auth(k);
        }

        // Race the HTTP request against the abort flag so users can cancel
        // even during slow TTFB (before the SSE stream opens).
        let abort_clone = abort.clone();
        let resp = tokio::select! {
            result = req.send() => {
                result.map_err(|e| anyhow!("chat_with_tools_stream request: {e}"))?
            }
            _ = async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if abort_clone.load(Ordering::Relaxed) {
                        break;
                    }
                }
            } => {
                debug!("[llm] chat_with_tools_stream aborted during connect");
                return Ok(None);
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            debug!("[llm] chat_with_tools_stream error {status}: {body}");
            return Err(anyhow!("API error {status}: {body}"));
        }

        debug!("[llm] chat_with_tools_stream opened, reading SSE…");
        let mut byte_stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut full_content = String::new();
        let mut chunk_count: usize = 0;
        let mut stream_usage: Option<TokenUsage> = None;

        // Accumulate tool calls: Vec of (id, function_name, arguments_buffer)
        let mut tool_call_acc: Vec<(String, String, String)> = Vec::new();

        'outer: loop {
            let next =
                tokio::time::timeout(std::time::Duration::from_millis(200), byte_stream.next())
                    .await;

            if abort.load(Ordering::Relaxed) {
                debug!("[llm] chat_with_tools_stream aborted after {chunk_count} chunks",);
                return Ok(None);
            }

            match next {
                Err(_timeout) => continue,
                Ok(None) => {
                    debug!(
                        "[llm] chat_with_tools_stream complete: {chunk_count} chunks, {} content chars, {} tool_calls",
                        full_content.len(),
                        tool_call_acc.len(),
                    );
                    break;
                }
                Ok(Some(bytes_result)) => {
                    let bytes = bytes_result.map_err(|e| {
                        debug!("[llm] chat_with_tools_stream read error: {e}");
                        anyhow!("stream read error: {e}")
                    })?;
                    buf.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(pos) = buf.find('\n') {
                        let line = buf[..pos].trim_end_matches('\r').to_string();
                        buf.drain(..=pos);

                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let data = &line["data: ".len()..];
                        if data == "[DONE]" {
                            break 'outer;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(err) = val.get("error") {
                                debug!("[llm] SSE error payload: {err}");
                                return Err(anyhow!("API error in stream: {err}"));
                            }
                            if let Some(u) = parse_usage(&val) {
                                stream_usage = Some(u);
                            }
                            let delta = &val["choices"][0]["delta"];

                            // Text content tokens — emit immediately
                            if let Some(tok) = delta["content"].as_str() {
                                if !tok.is_empty() {
                                    chunk_count += 1;
                                    full_content.push_str(tok);
                                    on_token(tok);
                                }
                            }

                            // Reasoning tokens (DeepSeek etc.) — skip for now
                            if let Some(tok) = delta["reasoning_content"].as_str() {
                                if !tok.is_empty() {
                                    chunk_count += 1;
                                }
                            }

                            // Tool call deltas — accumulate per index
                            if let Some(tcs) = delta["tool_calls"].as_array() {
                                for tc_delta in tcs {
                                    let idx = tc_delta["index"].as_u64().unwrap_or(0) as usize;
                                    // Grow the accumulator if needed
                                    while tool_call_acc.len() <= idx {
                                        tool_call_acc.push((
                                            String::new(),
                                            String::new(),
                                            String::new(),
                                        ));
                                    }
                                    if let Some(id) = tc_delta["id"].as_str() {
                                        tool_call_acc[idx].0 = id.to_string();
                                    }
                                    if let Some(name) = tc_delta["function"]["name"].as_str() {
                                        tool_call_acc[idx].1 = name.to_string();
                                    }
                                    if let Some(args_frag) =
                                        tc_delta["function"]["arguments"].as_str()
                                    {
                                        tool_call_acc[idx].2.push_str(args_frag);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Accumulate token usage
        if let Some(ref u) = stream_usage {
            debug!(
                "[llm] chat_with_tools_stream usage: prompt={} completion={} total={}",
                u.prompt_tokens, u.completion_tokens, u.total_tokens
            );
            self.usage.accumulate(u);
            *self.last_call_usage.lock().unwrap() = Some(u.clone());
        }

        // Build tool calls
        let tool_calls: Vec<ToolCall> = tool_call_acc
            .into_iter()
            .filter(|(id, name, _)| !id.is_empty() && !name.is_empty())
            .map(|(id, name, args_buf)| {
                let arguments = serde_json::from_str(&args_buf)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                ToolCall {
                    id,
                    name,
                    arguments,
                }
            })
            .collect();

        // Reconstruct the raw assistant message for conversation history
        let content_val = if full_content.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(full_content.clone())
        };
        let mut raw_message = serde_json::json!({
            "role": "assistant",
            "content": content_val,
        });
        if !tool_calls.is_empty() {
            let tc_arr: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        }
                    })
                })
                .collect();
            raw_message["tool_calls"] = serde_json::Value::Array(tc_arr);
        }

        let content = if full_content.is_empty() {
            None
        } else {
            Some(full_content)
        };

        debug!(
            "[llm] chat_with_tools_stream done: text={} tool_calls={}",
            content.is_some(),
            tool_calls.len()
        );

        Ok(Some(ChatWithToolsResponse {
            content,
            tool_calls,
            raw_message,
        }))
    }

    // ── Embeddings ─────────────────────────────────────────────────────────────

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Try remote API first
        match self.embed_remote(text).await {
            Ok(v) => Ok(v),
            Err(api_err) => {
                // Fallback to local fastembed
                warn!(
                    "[embed] API failed ({}), falling back to local fastembed",
                    api_err
                );
                let embedder = self.local_embedder.clone();
                let input = text.to_string();
                tokio::task::spawn_blocking(move || embedder.embed(&input))
                    .await
                    .map_err(|e| anyhow!("local embed join: {e}"))?
            }
        }
    }

    async fn embed_remote(&self, text: &str) -> Result<Vec<f32>> {
        let (api_key, api_base, embed_model) = {
            let g = self.config.read().unwrap();
            (g.api_key.clone(), g.api_base.clone(), g.embed_model.clone())
        };

        let base = api_base.as_deref().unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/embeddings", base.trim_end_matches('/'));

        let mut req = self.http.post(&url).json(&serde_json::json!({
          "model": embed_model,
          "input": text,
          "encoding_format": "float",
        }));
        if let Some(k) = &api_key {
            req = req.bearer_auth(k);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("embed request: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("embed API {status}: {body}"));
        }
        let parsed: EmbedResponse = resp.json().await.map_err(|e| anyhow!("embed parse: {e}"))?;
        parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow!("embed: empty data array"))
    }

    // ── Cache helpers ─────────────────────────────────────────────────────────

    fn lookup_cache(&self, key: &str) -> Option<String> {
        if let Ok(mut g) = self.cache.lock() {
            if let Some(pos) = g.iter().position(|(k, _)| k == key) {
                let (_, v) = g.remove(pos)?;
                g.push_back((key.to_string(), v.clone()));
                return Some(v);
            }
        }
        None
    }

    fn insert_cache(&self, key: String, value: String) {
        if let Ok(mut g) = self.cache.lock() {
            g.push_back((key, value));
            if g.len() > CACHE_CAPACITY {
                g.pop_front();
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn chat_url(api_base: &Option<String>) -> String {
    let base = api_base.as_deref().unwrap_or("https://api.openai.com/v1");
    format!("{}/chat/completions", base.trim_end_matches('/'))
}

fn messages_json(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect()
}

/// Extract token usage from an OpenAI-compatible API response JSON.
fn parse_usage(val: &serde_json::Value) -> Option<TokenUsage> {
    let u = val.get("usage")?;
    Some(TokenUsage {
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
        completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
    })
}
