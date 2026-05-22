use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::config::{LlmProviderConfig, LlmRoutingConfig};
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt as _;
use llm_router::{
    ChatResponse as RouterChatResponse, Client as RouterClient, Message as RouterMessage,
    MessageRole as RouterMessageRole, ProviderConfig as RouterProviderConfig,
    ProviderProtocol as RouterProviderProtocol, RouteTarget as RouterRouteTarget,
    RoutingConfig as RouterRoutingConfig, StreamEvent, ToolCallDelta as RouterToolCallDelta,
    ToolDefinition as RouterToolDefinition, ToolType as RouterToolType, Usage as RouterUsage,
};
use serde::{Deserialize, Serialize};

pub type AbortFlag = Arc<AtomicBool>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl LlmMessage {
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "role": self.role.as_str(),
            "content": self.content.as_str(),
        });
        if let Some(name) = &self.name {
            value["name"] = serde_json::Value::String(name.clone());
        }
        value
    }
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

pub struct ChatWithToolsResponse {
    pub raw_message: serde_json::Value,
}

impl ChatWithToolsResponse {
    pub fn text(&self) -> Option<String> {
        extract_text_from_raw_message(&self.raw_message)
    }

    pub fn tool_calls(&self) -> Vec<ToolCall> {
        extract_tool_calls_from_raw_message(&self.raw_message)
    }
}

const CACHE_CAPACITY: usize = 64;
const EMBED_MAX_CHARS_PER_CHUNK: usize = 24_000;
const EMBED_CHUNK_OVERLAP_CHARS: usize = 512;
const EMBED_MAX_CHARS_PER_BATCH: usize = 48_000;
const EMBED_MAX_ITEMS_PER_BATCH: usize = 8;

#[derive(Clone)]
struct LlmInnerConfig {
    providers: Vec<LlmProviderConfig>,
    routing: LlmRoutingConfig,
}

#[derive(Clone)]
pub struct LlmClient {
    config: Arc<RwLock<LlmInnerConfig>>,
    router: RouterClient,
    cache: Arc<Mutex<VecDeque<(String, String)>>>,
    pub usage: Arc<CumulativeUsage>,
    last_call_usage: Arc<Mutex<Option<TokenUsage>>>,
}

impl LlmClient {
    pub fn new(providers: Vec<LlmProviderConfig>, routing: LlmRoutingConfig) -> Self {
        let router = RouterClient::new(
            providers.iter().map(map_provider_config).collect(),
            map_routing_config(&routing),
        );
        Self {
            config: Arc::new(RwLock::new(LlmInnerConfig { providers, routing })),
            router,
            cache: Arc::new(Mutex::new(VecDeque::new())),
            usage: Arc::new(CumulativeUsage::default()),
            last_call_usage: Arc::new(Mutex::new(None)),
        }
    }

    pub fn new_from_env() -> Self {
        let cfg = crate::config::load_with_env();
        Self::new(cfg.llm.providers, cfg.llm.routing)
    }

    pub fn take_last_call_usage(&self) -> Option<TokenUsage> {
        self.last_call_usage.lock().unwrap().take()
    }

    pub fn model_name(&self) -> String {
        self.config.read().unwrap().routing.primary.model.clone()
    }

    pub fn routing_config(&self) -> (Vec<LlmProviderConfig>, LlmRoutingConfig) {
        let g = self.config.read().unwrap();
        (g.providers.clone(), g.routing.clone())
    }

    pub fn has_primary_provider(&self) -> bool {
        self.router.has_primary_provider()
    }

    pub fn primary_provider_name(&self) -> String {
        self.router.primary_provider_name()
    }

    pub fn reconfigure(&self, providers: Vec<LlmProviderConfig>, routing: LlmRoutingConfig) {
        {
            let mut g = self.config.write().unwrap();
            g.providers = providers.clone();
            g.routing = routing.clone();
        }
        self.router.reconfigure(
            providers.iter().map(map_provider_config).collect(),
            map_routing_config(&routing),
        );
    }

    pub async fn chat(&self, messages: Vec<LlmMessage>) -> Result<String> {
        self.chat_impl(messages, false).await
    }

    pub async fn chat_mini(&self, messages: Vec<LlmMessage>) -> Result<String> {
        self.chat_impl(messages, true).await
    }

    async fn chat_impl(&self, messages: Vec<LlmMessage>, mini: bool) -> Result<String> {
        let cache_key = format!(
            "{}:{}",
            if mini { "mini" } else { "primary" },
            serde_json::to_string(&messages)?
        );
        if let Some(hit) = self.lookup_cache(&cache_key) {
            return Ok(hit);
        }

        let response = if mini {
            self.router
                .chat_mini(messages.into_iter().map(map_message).collect())
                .await?
        } else {
            self.router
                .chat(messages.into_iter().map(map_message).collect())
                .await?
        };
        self.record_usage(response.usage.as_ref());
        let text = response.content.unwrap_or_default();
        self.insert_cache(cache_key, text.clone());
        Ok(text)
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<LlmMessage>,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        self.chat_stream_impl(messages, false, on_token, abort)
            .await
    }

    pub async fn chat_stream_mini(
        &self,
        messages: Vec<LlmMessage>,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        self.chat_stream_impl(messages, true, on_token, abort).await
    }

    async fn chat_stream_impl(
        &self,
        messages: Vec<LlmMessage>,
        mini: bool,
        on_token: impl Fn(&str, bool) + Send,
        abort: &AbortFlag,
    ) -> Result<String> {
        let mut stream = if mini {
            self.router
                .chat_stream_mini(messages.into_iter().map(map_message).collect())
                .await?
        } else {
            self.router
                .chat_stream(messages.into_iter().map(map_message).collect())
                .await?
        };

        let mut full = String::new();
        loop {
            let next =
                tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;
            if abort.load(Ordering::Relaxed) {
                break;
            }
            match next {
                Err(_) => continue,
                Ok(None) => break,
                Ok(Some(event)) => match event? {
                    StreamEvent::TextDelta(text) => {
                        full.push_str(&text);
                        on_token(&text, false);
                    }
                    StreamEvent::ReasoningDelta(text) => {
                        on_token(&text, true);
                    }
                    StreamEvent::Usage(usage) => self.record_usage(Some(&usage)),
                    StreamEvent::ToolCallDelta(_)
                    | StreamEvent::RawContentBlockStart { .. }
                    | StreamEvent::RawContentBlockDelta { .. }
                    | StreamEvent::RawOutputItemAdded { .. }
                    | StreamEvent::RawOutputItemDelta { .. }
                    | StreamEvent::RawAssistantMessageDelta { .. }
                    | StreamEvent::Done => {}
                },
            }
        }

        if full.is_empty() && !abort.load(Ordering::Relaxed) {
            let config_path = crate::config::config_path()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "the app config file".to_string());
            return Err(anyhow!(
                "LLM returned an empty response.\n\
         • Verify the model name and api_key in {config_path}"
            ));
        }

        Ok(full)
    }

    pub async fn chat_with_tools(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
    ) -> Result<ChatWithToolsResponse> {
        let response = self
            .router
            .chat_with_tools(
                messages
                    .into_iter()
                    .map(router_message_from_raw)
                    .collect::<Result<Vec<_>>>()?,
                tools
                    .into_iter()
                    .map(router_tool_from_raw)
                    .collect::<Result<Vec<_>>>()?,
            )
            .await?;
        self.record_usage(response.usage.as_ref());
        Ok(map_chat_with_tools_response(response))
    }

    pub async fn chat_with_tools_abortable(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        abort: &AbortFlag,
    ) -> Result<Option<ChatWithToolsResponse>> {
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
                Ok(None)
            }
        }
    }

    pub async fn chat_with_tools_stream(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        on_token: impl Fn(&str) + Send,
        abort: &AbortFlag,
    ) -> Result<Option<ChatWithToolsResponse>> {
        let mut stream = self
            .router
            .chat_with_tools_stream(
                messages
                    .into_iter()
                    .map(router_message_from_raw)
                    .collect::<Result<Vec<_>>>()?,
                tools
                    .into_iter()
                    .map(router_tool_from_raw)
                    .collect::<Result<Vec<_>>>()?,
            )
            .await?;

        let mut full_content = String::new();
        let mut full_reasoning_content = String::new();
        let mut tool_call_acc: Vec<(String, String, String)> = Vec::new();
        let mut raw_blocks: Vec<serde_json::Value> = Vec::new();
        let mut raw_block_json_acc: Vec<String> = Vec::new();
        let mut raw_output_items: Vec<serde_json::Value> = Vec::new();
        let mut raw_assistant_message: Option<serde_json::Value> = None;

        loop {
            let next =
                tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;
            if abort.load(Ordering::Relaxed) {
                return Ok(None);
            }
            match next {
                Err(_) => continue,
                Ok(None) => break,
                Ok(Some(event)) => match event? {
                    StreamEvent::TextDelta(text) => {
                        full_content.push_str(&text);
                        on_token(&text);
                    }
                    StreamEvent::ReasoningDelta(text) => {
                        full_reasoning_content.push_str(&text);
                    }
                    StreamEvent::ToolCallDelta(delta) => {
                        apply_tool_call_delta(&mut tool_call_acc, delta);
                    }
                    StreamEvent::RawContentBlockStart { index, block } => {
                        set_raw_content_block(&mut raw_blocks, index, block);
                    }
                    StreamEvent::RawContentBlockDelta { index, delta } => {
                        apply_raw_content_block_delta(
                            &mut raw_blocks,
                            &mut raw_block_json_acc,
                            index,
                            delta,
                        );
                    }
                    StreamEvent::RawOutputItemAdded { index, item } => {
                        set_raw_output_item(&mut raw_output_items, index, item);
                    }
                    StreamEvent::RawOutputItemDelta { index, delta } => {
                        apply_raw_output_item_delta(&mut raw_output_items, index, delta);
                    }
                    StreamEvent::RawAssistantMessageDelta { delta } => {
                        apply_raw_assistant_message_delta(&mut raw_assistant_message, delta);
                    }
                    StreamEvent::Usage(usage) => self.record_usage(Some(&usage)),
                    StreamEvent::Done => break,
                },
            }
        }

        let tool_calls = finalize_tool_calls(tool_call_acc);
        let raw_message = finalize_stream_assistant_message(
            &full_content,
            &full_reasoning_content,
            &tool_calls,
            raw_blocks,
            raw_output_items,
            raw_assistant_message,
        );

        Ok(Some(ChatWithToolsResponse { raw_message }))
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let chunks = split_embedding_input(text);
        self.embed_remote_chunks(&chunks).await
    }

    async fn embed_remote_chunks(&self, chunks: &[String]) -> Result<Vec<f32>> {
        if chunks.is_empty() {
            bail!("embed: no input chunks");
        }
        let batches = batch_embedding_inputs(chunks);
        let mut embeddings = Vec::with_capacity(chunks.len());
        for batch in batches {
            let response = self.router.embed(batch).await?;
            self.record_usage(response.usage.as_ref());
            embeddings.extend(response.vectors);
        }
        if embeddings.len() != chunks.len() {
            bail!(
                "embed: response count mismatch (expected {}, got {})",
                chunks.len(),
                embeddings.len()
            );
        }
        if embeddings.len() == 1 {
            return embeddings
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("embed: missing embedding"));
        }
        average_embeddings(&embeddings)
    }

    fn record_usage(&self, usage: Option<&RouterUsage>) {
        if let Some(usage) = usage {
            let usage = TokenUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            };
            self.usage.accumulate(&usage);
            *self.last_call_usage.lock().unwrap() = Some(usage);
        }
    }

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

fn map_provider_config(provider: &LlmProviderConfig) -> RouterProviderConfig {
    RouterProviderConfig {
        id: provider.id.clone(),
        name: provider.name.clone(),
        protocol: match provider.kind.to_ascii_lowercase().as_str() {
            "ollama" => RouterProviderProtocol::Ollama,
            "anthropic_messages" => RouterProviderProtocol::AnthropicMessages,
            "openai_responses" => RouterProviderProtocol::OpenAiResponses,
            _ => RouterProviderProtocol::OpenAiCompatible,
        },
        provider_key: provider.provider.clone(),
        api_base: if provider.api_base.trim().is_empty() {
            None
        } else {
            Some(provider.api_base.clone())
        },
        api_key: if provider.api_key.trim().is_empty() {
            None
        } else {
            Some(provider.api_key.clone())
        },
        enabled: provider.enabled,
        models: provider.models.clone(),
        extra: serde_json::Map::new(),
    }
}

fn map_routing_config(routing: &LlmRoutingConfig) -> RouterRoutingConfig {
    RouterRoutingConfig {
        primary: map_route_target(&routing.primary),
        mini: map_route_target(&routing.mini),
        embedding: map_route_target(&routing.embedding),
    }
}

fn map_route_target(target: &crate::config::LlmRouteTarget) -> RouterRouteTarget {
    RouterRouteTarget {
        provider_id: target.provider_id.clone(),
        model: target.model.clone(),
    }
}

fn map_message(message: LlmMessage) -> RouterMessage {
    RouterMessage {
        role: match message.role.as_str() {
            "system" => RouterMessageRole::System,
            "assistant" => RouterMessageRole::Assistant,
            "tool" => RouterMessageRole::Tool,
            _ => RouterMessageRole::User,
        },
        content: Some(message.content),
        raw_message: None,
        name: message.name,
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning_content: None,
    }
}

fn router_message_from_raw(value: serde_json::Value) -> Result<RouterMessage> {
    let role = match value["role"].as_str().unwrap_or("user") {
        "system" => RouterMessageRole::System,
        "assistant" => RouterMessageRole::Assistant,
        "tool" => RouterMessageRole::Tool,
        _ => RouterMessageRole::User,
    };
    let tool_calls = value["tool_calls"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(llm_router::ToolCall {
                        id: item["id"].as_str()?.to_string(),
                        name: item["function"]["name"].as_str()?.to_string(),
                        arguments: serde_json::from_str(
                            item["function"]["arguments"].as_str().unwrap_or("{}"),
                        )
                        .unwrap_or_else(|_| serde_json::json!({})),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(RouterMessage {
        role,
        content: value.get("content").and_then(|item| match item {
            serde_json::Value::Null => None,
            serde_json::Value::String(text) => Some(text.clone()),
            _ => None,
        }),
        raw_message: Some(value.clone()),
        name: value["name"].as_str().map(str::to_string),
        tool_calls,
        tool_call_id: value["tool_call_id"].as_str().map(str::to_string),
        reasoning_content: value["reasoning_content"].as_str().map(str::to_string),
    })
}

fn router_tool_from_raw(value: serde_json::Value) -> Result<RouterToolDefinition> {
    Ok(RouterToolDefinition {
        tool_type: RouterToolType::Function,
        function: llm_router::FunctionDefinition {
            name: value["function"]["name"]
                .as_str()
                .ok_or_else(|| anyhow!("tool function missing name"))?
                .to_string(),
            description: value["function"]["description"]
                .as_str()
                .map(str::to_string),
            parameters: value["function"]["parameters"].clone(),
        },
    })
}

fn map_chat_with_tools_response(response: RouterChatResponse) -> ChatWithToolsResponse {
    ChatWithToolsResponse {
        raw_message: response.raw_message,
    }
}

fn apply_tool_call_delta(acc: &mut Vec<(String, String, String)>, delta: RouterToolCallDelta) {
    while acc.len() <= delta.index {
        acc.push((String::new(), String::new(), String::new()));
    }
    if let Some(id) = delta.id {
        acc[delta.index].0 = id;
    }
    if let Some(name) = delta.name {
        acc[delta.index].1 = name;
    }
    if let Some(arguments_fragment) = delta.arguments_fragment {
        acc[delta.index].2.push_str(&arguments_fragment);
    }
}

fn finalize_tool_calls(acc: Vec<(String, String, String)>) -> Vec<ToolCall> {
    acc.into_iter()
        .filter(|(id, name, _)| !id.is_empty() && !name.is_empty())
        .map(|(id, name, arguments)| ToolCall {
            id,
            name,
            arguments: serde_json::from_str(&arguments)
                .unwrap_or_else(|_| serde_json::Value::Object(Default::default())),
        })
        .collect()
}

fn set_raw_content_block(
    blocks: &mut Vec<serde_json::Value>,
    index: usize,
    block: serde_json::Value,
) {
    while blocks.len() <= index {
        blocks.push(serde_json::Value::Null);
    }
    blocks[index] = block;
}

fn apply_raw_content_block_delta(
    blocks: &mut Vec<serde_json::Value>,
    json_acc: &mut Vec<String>,
    index: usize,
    delta: serde_json::Value,
) {
    while blocks.len() <= index {
        blocks.push(serde_json::json!({}));
    }
    while json_acc.len() <= index {
        json_acc.push(String::new());
    }

    let block = blocks[index]
        .as_object_mut()
        .expect("raw content block must be an object");
    match delta["type"].as_str().unwrap_or_default() {
        "text_delta" => {
            let current = block
                .entry("text".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(text) = delta["text"].as_str() {
                current
                    .as_str()
                    .map(|existing| format!("{existing}{text}"))
                    .map(serde_json::Value::String)
                    .into_iter()
                    .for_each(|value| *current = value);
            }
        }
        "thinking_delta" => {
            let current = block
                .entry("thinking".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(text) = delta["thinking"].as_str() {
                current
                    .as_str()
                    .map(|existing| format!("{existing}{text}"))
                    .map(serde_json::Value::String)
                    .into_iter()
                    .for_each(|value| *current = value);
            }
        }
        "signature_delta" => {
            let current = block
                .entry("signature".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(text) = delta["signature"].as_str() {
                current
                    .as_str()
                    .map(|existing| format!("{existing}{text}"))
                    .map(serde_json::Value::String)
                    .into_iter()
                    .for_each(|value| *current = value);
            }
        }
        "input_json_delta" => {
            if let Some(fragment) = delta["partial_json"].as_str() {
                json_acc[index].push_str(fragment);
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json_acc[index]) {
                    block.insert("input".to_string(), value);
                }
            }
        }
        _ => {}
    }
}

fn set_raw_output_item(items: &mut Vec<serde_json::Value>, index: usize, item: serde_json::Value) {
    while items.len() <= index {
        items.push(serde_json::Value::Null);
    }
    items[index] = item;
}

fn apply_raw_output_item_delta(
    items: &mut Vec<serde_json::Value>,
    index: usize,
    delta: serde_json::Value,
) {
    while items.len() <= index {
        items.push(serde_json::json!({}));
    }

    let item = items[index]
        .as_object_mut()
        .expect("raw output item must be an object");
    match delta["type"].as_str().unwrap_or_default() {
        "response.output_text.delta" => {
            let content_index = delta["content_index"].as_u64().unwrap_or(0) as usize;
            let content = item
                .entry("content".to_string())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            let content = content
                .as_array_mut()
                .expect("message content must be an array");
            while content.len() <= content_index {
                content.push(serde_json::json!({"type": "output_text", "text": ""}));
            }
            let part = content[content_index]
                .as_object_mut()
                .expect("content part must be an object");
            part.entry("type".to_string())
                .or_insert_with(|| serde_json::Value::String("output_text".to_string()));
            let current = part
                .entry("text".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(fragment) = delta["delta"].as_str() {
                let next = format!("{}{}", current.as_str().unwrap_or_default(), fragment);
                *current = serde_json::Value::String(next);
            }
        }
        "response.reasoning.delta" => {
            let current = item
                .entry("text".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(fragment) = delta["delta"].as_str() {
                let next = format!("{}{}", current.as_str().unwrap_or_default(), fragment);
                *current = serde_json::Value::String(next);
            }
        }
        "response.reasoning_summary_text.delta" => {
            let summary_index = delta["summary_index"].as_u64().unwrap_or(0) as usize;
            let summary = item
                .entry("summary".to_string())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            let summary = summary
                .as_array_mut()
                .expect("reasoning summary must be an array");
            while summary.len() <= summary_index {
                summary.push(serde_json::json!({"type": "summary_text", "text": ""}));
            }
            let part = summary[summary_index]
                .as_object_mut()
                .expect("summary part must be an object");
            let current = part
                .entry("text".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(fragment) = delta["delta"].as_str() {
                let next = format!("{}{}", current.as_str().unwrap_or_default(), fragment);
                *current = serde_json::Value::String(next);
            }
        }
        "response.function_call_arguments.delta" => {
            let current = item
                .entry("arguments".to_string())
                .or_insert_with(|| serde_json::Value::String(String::new()));
            if let Some(fragment) = delta["delta"].as_str() {
                let next = format!("{}{}", current.as_str().unwrap_or_default(), fragment);
                *current = serde_json::Value::String(next);
            }
        }
        _ => {}
    }
}

fn apply_raw_assistant_message_delta(
    message: &mut Option<serde_json::Value>,
    delta: serde_json::Value,
) {
    if message.is_none() {
        *message = Some(serde_json::json!({
            "role": "assistant",
            "content": serde_json::Value::Null,
        }));
    }

    let root = message
        .as_mut()
        .and_then(|value| value.as_object_mut())
        .expect("raw assistant message must be an object");

    for (key, value) in delta.as_object().into_iter().flatten() {
        match key.as_str() {
            "content" => match value {
                serde_json::Value::String(fragment) => {
                    let existing = root
                        .entry("content".to_string())
                        .or_insert(serde_json::Value::Null);
                    match existing {
                        serde_json::Value::Null => {
                            *existing = serde_json::Value::String(fragment.clone())
                        }
                        serde_json::Value::String(current) => current.push_str(fragment),
                        other => *other = serde_json::Value::String(fragment.clone()),
                    }
                }
                other => {
                    root.insert("content".to_string(), other.clone());
                }
            },
            "reasoning_content" => {
                if let Some(fragment) = value.as_str() {
                    let existing = root
                        .entry("reasoning_content".to_string())
                        .or_insert_with(|| serde_json::Value::String(String::new()));
                    match existing {
                        serde_json::Value::String(current) => current.push_str(fragment),
                        other => *other = serde_json::Value::String(fragment.to_string()),
                    }
                } else {
                    root.insert("reasoning_content".to_string(), value.clone());
                }
            }
            "tool_calls" => {
                let existing = root
                    .entry("tool_calls".to_string())
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                let existing = existing
                    .as_array_mut()
                    .expect("tool_calls must be an array");
                if let Some(items) = value.as_array() {
                    for (fallback_index, item) in items.iter().enumerate() {
                        let index = item["index"]
                            .as_u64()
                            .map(|value| value as usize)
                            .unwrap_or(fallback_index);
                        while existing.len() <= index {
                            existing.push(serde_json::json!({}));
                        }
                        merge_tool_call_delta_value(&mut existing[index], item);
                    }
                }
            }
            _ => {
                root.insert(key.clone(), value.clone());
            }
        }
    }
}

fn merge_tool_call_delta_value(target: &mut serde_json::Value, item: &serde_json::Value) {
    let obj = target
        .as_object_mut()
        .expect("tool call delta target must be an object");
    if let Some(id) = item.get("id").and_then(|value| value.as_str()) {
        obj.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    }
    if let Some(item_type) = item.get("type") {
        obj.insert("type".to_string(), item_type.clone());
    }
    let function = obj
        .entry("function".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let function = function
        .as_object_mut()
        .expect("tool call function must be an object");
    if let Some(name) = item["function"]["name"].as_str() {
        function.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
    }
    if let Some(arguments) = item["function"].get("arguments") {
        let current = function
            .entry("arguments".to_string())
            .or_insert_with(|| serde_json::Value::String(String::new()));
        match (current, arguments) {
            (serde_json::Value::String(existing), serde_json::Value::String(fragment)) => {
                existing.push_str(fragment);
            }
            (slot, other) => *slot = other.clone(),
        }
    }
}

pub fn extract_text_from_raw_message(message: &serde_json::Value) -> Option<String> {
    if let Some(content) = message.get("content") {
        match content {
            serde_json::Value::String(text) if !text.is_empty() => return Some(text.clone()),
            serde_json::Value::Array(blocks) => {
                let mut text = String::new();
                for block in blocks {
                    match block {
                        serde_json::Value::String(part) => text.push_str(part),
                        serde_json::Value::Object(_) => {
                            match block["type"].as_str().unwrap_or_default() {
                                "text" | "output_text" | "input_text" => {
                                    if let Some(part) = block["text"].as_str() {
                                        text.push_str(part);
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                if !text.is_empty() {
                    return Some(text);
                }
            }
            _ => {}
        }
    }

    if let Some(output) = message.get("output").and_then(|value| value.as_array()) {
        let mut text = String::new();
        for item in output {
            match item["type"].as_str().unwrap_or_default() {
                "message" => {
                    if let Some(content_items) = item["content"].as_array() {
                        for content_item in content_items {
                            match content_item["type"].as_str().unwrap_or_default() {
                                "output_text" | "text" => {
                                    if let Some(part) = content_item["text"].as_str() {
                                        text.push_str(part);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

pub fn extract_tool_calls_from_raw_message(message: &serde_json::Value) -> Vec<ToolCall> {
    if let Some(content) = message.get("content").and_then(|value| value.as_array()) {
        let calls: Vec<ToolCall> = content
            .iter()
            .filter_map(|block| {
                if block["type"].as_str() != Some("tool_use") {
                    return None;
                }
                Some(ToolCall {
                    id: block["id"].as_str()?.to_string(),
                    name: block["name"].as_str()?.to_string(),
                    arguments: block["input"].clone(),
                })
            })
            .collect();
        if !calls.is_empty() {
            return calls;
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|value| value.as_array()) {
        return tool_calls
            .iter()
            .filter_map(|item| {
                Some(ToolCall {
                    id: item["id"].as_str()?.to_string(),
                    name: item["function"]["name"].as_str()?.to_string(),
                    arguments: match &item["function"]["arguments"] {
                        serde_json::Value::String(text) => {
                            serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({}))
                        }
                        value => value.clone(),
                    },
                })
            })
            .collect();
    }

    message
        .get("output")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item["type"].as_str() != Some("function_call") {
                        return None;
                    }
                    Some(ToolCall {
                        id: item["call_id"]
                            .as_str()
                            .or_else(|| item["id"].as_str())?
                            .to_string(),
                        name: item["name"].as_str()?.to_string(),
                        arguments: item["arguments"]
                            .as_str()
                            .and_then(|text| serde_json::from_str(text).ok())
                            .unwrap_or_else(|| serde_json::json!({})),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_assistant_raw_message(
    full_content: &str,
    full_reasoning_content: &str,
    tool_calls: &[ToolCall],
) -> serde_json::Value {
    let content_val = if full_content.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(full_content.to_string())
    };
    let mut raw_message = serde_json::json!({
        "role": "assistant",
        "content": content_val,
    });

    if !full_reasoning_content.is_empty() {
        raw_message["reasoning_content"] =
            serde_json::Value::String(full_reasoning_content.to_string());
    }
    if !tool_calls.is_empty() {
        raw_message["tool_calls"] = serde_json::Value::Array(
            tool_calls
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
                .collect(),
        );
    }
    raw_message
}

fn finalize_stream_assistant_message(
    full_content: &str,
    full_reasoning_content: &str,
    tool_calls: &[ToolCall],
    raw_blocks: Vec<serde_json::Value>,
    raw_output_items: Vec<serde_json::Value>,
    raw_assistant_message: Option<serde_json::Value>,
) -> serde_json::Value {
    let normalized = build_assistant_raw_message(full_content, full_reasoning_content, tool_calls);

    if !raw_output_items.is_empty() {
        return serde_json::json!({
            "role": "assistant",
            "output": raw_output_items,
        });
    }

    if let Some(message) = raw_assistant_message {
        return merge_normalized_assistant_message(message, &normalized);
    }

    if raw_blocks.is_empty() {
        normalized
    } else {
        merge_normalized_assistant_message(
            serde_json::json!({
                "role": "assistant",
                "content": raw_blocks,
            }),
            &normalized,
        )
    }
}

fn merge_normalized_assistant_message(
    mut raw_message: serde_json::Value,
    normalized: &serde_json::Value,
) -> serde_json::Value {
    let Some(raw) = raw_message.as_object_mut() else {
        return normalized.clone();
    };

    raw.insert(
        "role".to_string(),
        serde_json::Value::String("assistant".to_string()),
    );

    if !raw.contains_key("content") {
        if let Some(content) = normalized.get("content") {
            raw.insert("content".to_string(), content.clone());
        }
    }

    let has_reasoning = raw
        .get("reasoning_content")
        .and_then(|value| value.as_str())
        .is_some_and(|text| !text.is_empty());
    if !has_reasoning {
        if let Some(reasoning) = normalized.get("reasoning_content") {
            raw.insert("reasoning_content".to_string(), reasoning.clone());
        }
    }

    let has_tool_calls = raw
        .get("tool_calls")
        .and_then(|value| value.as_array())
        .is_some_and(|items| !items.is_empty());
    if !has_tool_calls {
        if let Some(tool_calls) = normalized.get("tool_calls") {
            raw.insert("tool_calls".to_string(), tool_calls.clone());
        }
    }

    raw_message
}

fn split_embedding_input(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![String::new()];
    }
    if trimmed.chars().count() <= EMBED_MAX_CHARS_PER_CHUNK {
        return vec![trimmed.to_string()];
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let len = chars.len();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < len {
        let mut end = (start + EMBED_MAX_CHARS_PER_CHUNK).min(len);
        if end < len {
            let search_start = start + EMBED_MAX_CHARS_PER_CHUNK.saturating_mul(7) / 10;
            if let Some(split_at) = find_embedding_boundary(&chars, search_start.min(end), end) {
                end = split_at;
            }
        }

        if end <= start {
            end = (start + EMBED_MAX_CHARS_PER_CHUNK).min(len);
        }

        let chunk: String = chars[start..end].iter().collect();
        let chunk = chunk.trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if end >= len {
            break;
        }

        start = end.saturating_sub(EMBED_CHUNK_OVERLAP_CHARS.min(end.saturating_sub(start)));
        while start < len && chars[start].is_whitespace() {
            start += 1;
        }
    }

    if chunks.is_empty() {
        vec![trimmed.to_string()]
    } else {
        chunks
    }
}

fn batch_embedding_inputs(chunks: &[String]) -> Vec<Vec<String>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;

    for chunk in chunks {
        let chunk_chars = chunk.chars().count();
        let exceeds_items = current.len() >= EMBED_MAX_ITEMS_PER_BATCH;
        let exceeds_chars =
            !current.is_empty() && current_chars + chunk_chars > EMBED_MAX_CHARS_PER_BATCH;

        if exceeds_items || exceeds_chars {
            batches.push(current);
            current = Vec::new();
            current_chars = 0;
        }

        current_chars += chunk_chars;
        current.push(chunk.clone());
    }

    if !current.is_empty() {
        batches.push(current);
    }

    batches
}

fn find_embedding_boundary(chars: &[char], search_start: usize, end: usize) -> Option<usize> {
    for idx in (search_start..end).rev() {
        let ch = chars[idx];
        if ch == '\n'
            || ch == '。'
            || ch == '！'
            || ch == '？'
            || ch == '.'
            || ch == '!'
            || ch == '?'
        {
            return Some(idx + 1);
        }
    }
    for idx in (search_start..end).rev() {
        if chars[idx].is_whitespace() {
            return Some(idx + 1);
        }
    }
    None
}

fn average_embeddings(embeddings: &[Vec<f32>]) -> Result<Vec<f32>> {
    let dim = embeddings
        .first()
        .map(|e| e.len())
        .ok_or_else(|| anyhow!("embed: missing embedding"))?;
    if dim == 0 {
        bail!("embed: empty embedding vector");
    }

    let mut out = vec![0.0f32; dim];
    for emb in embeddings {
        if emb.len() != dim {
            bail!("embed: inconsistent embedding dimensions");
        }
        for (dst, value) in out.iter_mut().zip(emb.iter()) {
            *dst += *value;
        }
    }

    let count = embeddings.len() as f32;
    for value in &mut out {
        *value /= count;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_raw_assistant_message_delta, apply_raw_output_item_delta, apply_tool_call_delta,
        average_embeddings, batch_embedding_inputs, build_assistant_raw_message,
        extract_text_from_raw_message, extract_tool_calls_from_raw_message,
        finalize_stream_assistant_message, finalize_tool_calls, split_embedding_input, ToolCall,
        EMBED_MAX_CHARS_PER_BATCH, EMBED_MAX_CHARS_PER_CHUNK, EMBED_MAX_ITEMS_PER_BATCH,
    };
    use llm_router::ToolCallDelta as RouterToolCallDelta;

    #[test]
    fn tool_call_delta_accumulates_arguments_and_finalizes() {
        let mut acc = Vec::new();
        apply_tool_call_delta(
            &mut acc,
            RouterToolCallDelta {
                index: 0,
                id: Some("call_1".to_string()),
                name: Some("search".to_string()),
                arguments_fragment: Some("{\"q\":\"hel".to_string()),
            },
        );
        apply_tool_call_delta(
            &mut acc,
            RouterToolCallDelta {
                index: 0,
                id: None,
                name: None,
                arguments_fragment: Some("lo\"}".to_string()),
            },
        );

        let calls = finalize_tool_calls(acc);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[0].arguments["q"], "hello");
    }

    #[test]
    fn assistant_raw_message_preserves_reasoning_and_tool_calls() {
        let raw = build_assistant_raw_message(
            "answer",
            "reason",
            &[ToolCall {
                id: "call_1".to_string(),
                name: "file_read".to_string(),
                arguments: serde_json::json!({"path": "/tmp/a.txt"}),
            }],
        );
        assert_eq!(raw["role"], "assistant");
        assert_eq!(raw["content"], "answer");
        assert_eq!(raw["reasoning_content"], "reason");
        assert_eq!(raw["tool_calls"][0]["id"], "call_1");
        assert_eq!(raw["tool_calls"][0]["function"]["name"], "file_read");
    }

    #[test]
    fn extract_helpers_read_anthropic_raw_blocks() {
        let raw = serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "plan", "signature": "sig"},
                {"type": "text", "text": "answer"},
                {"type": "tool_use", "id": "tool_1", "name": "search", "input": {"q": "hello"}}
            ]
        });

        assert_eq!(
            extract_text_from_raw_message(&raw).as_deref(),
            Some("answer")
        );
        let tool_calls = extract_tool_calls_from_raw_message(&raw);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "tool_1");
        assert_eq!(tool_calls[0].arguments["q"], "hello");
    }

    #[test]
    fn extract_helpers_read_openai_responses_raw_output() {
        let raw = serde_json::json!({
            "role": "assistant",
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "final answer"}
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "lookup",
                    "arguments": "{\"city\":\"Shanghai\"}"
                }
            ]
        });

        assert_eq!(
            extract_text_from_raw_message(&raw).as_deref(),
            Some("final answer")
        );
        let tool_calls = extract_tool_calls_from_raw_message(&raw);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "lookup");
        assert_eq!(tool_calls[0].arguments["city"], "Shanghai");
    }

    #[test]
    fn raw_output_item_delta_rebuilds_streamed_openai_responses_message() {
        let mut items = vec![serde_json::json!({
            "type": "message",
            "id": "msg_1",
            "content": []
        })];

        apply_raw_output_item_delta(
            &mut items,
            0,
            serde_json::json!({
                "type": "response.output_text.delta",
                "output_index": 0,
                "content_index": 0,
                "delta": "Hel"
            }),
        );
        apply_raw_output_item_delta(
            &mut items,
            0,
            serde_json::json!({
                "type": "response.output_text.delta",
                "output_index": 0,
                "content_index": 0,
                "delta": "lo"
            }),
        );

        let raw = serde_json::json!({
            "role": "assistant",
            "output": items
        });

        assert_eq!(
            extract_text_from_raw_message(&raw).as_deref(),
            Some("Hello")
        );
    }

    #[test]
    fn raw_assistant_message_delta_rebuilds_openai_compatible_stream() {
        let mut raw = None;
        apply_raw_assistant_message_delta(
            &mut raw,
            serde_json::json!({
                "content": "Hel",
                "reasoning_content": "think",
                "tool_calls": [{
                    "index": 0,
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"q\":\"hel"
                    }
                }]
            }),
        );
        apply_raw_assistant_message_delta(
            &mut raw,
            serde_json::json!({
                "content": "lo",
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": "lo\"}"
                    }
                }]
            }),
        );
        let raw = raw.unwrap();
        assert_eq!(
            extract_text_from_raw_message(&raw).as_deref(),
            Some("Hello")
        );
        let tool_calls = extract_tool_calls_from_raw_message(&raw);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].arguments["q"], "hello");
    }

    #[test]
    fn finalize_stream_message_merges_reasoning_back_into_raw_assistant_message() {
        let raw = finalize_stream_assistant_message(
            "done",
            "think",
            &[ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"q": "hello"}),
            }],
            Vec::new(),
            Vec::new(),
            Some(serde_json::json!({
                "role": "assistant",
                "content": "done"
            })),
        );

        assert_eq!(raw["content"], "done");
        assert_eq!(raw["reasoning_content"], "think");
        assert_eq!(raw["tool_calls"][0]["function"]["name"], "search");
    }

    #[test]
    fn raw_assistant_message_delta_merges_ollama_tool_calls_by_position() {
        let mut raw = None;
        apply_raw_assistant_message_delta(
            &mut raw,
            serde_json::json!({
                "content": "Hel",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "lookup",
                        "arguments": {"city": "Shanghai"}
                    }
                }]
            }),
        );
        apply_raw_assistant_message_delta(
            &mut raw,
            serde_json::json!({
                "content": "lo",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "lookup",
                        "arguments": {"city": "Shanghai"}
                    }
                }]
            }),
        );

        let raw = raw.unwrap();
        assert_eq!(
            extract_text_from_raw_message(&raw).as_deref(),
            Some("Hello")
        );
        let tool_calls = extract_tool_calls_from_raw_message(&raw);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].arguments["city"], "Shanghai");
    }

    #[test]
    fn split_embedding_input_keeps_small_text_single_chunk() {
        let chunks = split_embedding_input("hello world");
        assert_eq!(chunks, vec!["hello world".to_string()]);
    }

    #[test]
    fn split_embedding_input_splits_large_text() {
        let input = "a".repeat(EMBED_MAX_CHARS_PER_CHUNK + 2048);
        let chunks = split_embedding_input(&input);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn batch_embedding_inputs_respects_limits() {
        let items = vec![
            "a".repeat(EMBED_MAX_CHARS_PER_BATCH / 3),
            "b".repeat(EMBED_MAX_CHARS_PER_BATCH / 3),
            "c".repeat(EMBED_MAX_CHARS_PER_BATCH / 3),
            "d".repeat(EMBED_MAX_CHARS_PER_BATCH / 3),
        ];
        let batches = batch_embedding_inputs(&items);
        assert!(batches.len() >= 2);
        assert!(batches
            .iter()
            .all(|batch| batch.len() <= EMBED_MAX_ITEMS_PER_BATCH));
    }

    #[test]
    fn average_embeddings_averages_vectors() {
        let avg = average_embeddings(&[vec![1.0, 3.0], vec![3.0, 5.0]]).unwrap();
        assert_eq!(avg, vec![2.0, 4.0]);
    }
}
