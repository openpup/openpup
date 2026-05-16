use std::pin::Pin;

use futures_util::{stream, Stream, StreamExt};
use reqwest::header::CONTENT_TYPE;

use crate::config::ProviderConfig;
use crate::error::{Result, RouterError};
use crate::types::{
    ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse, Message, MessageRole,
    StreamEvent, ToolCall, ToolCallDelta, ToolDefinition, ToolType,
};
use crate::usage::Usage;

#[derive(Clone)]
pub struct AnthropicProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

#[derive(Default)]
struct StreamParseOutcome {
    events: Vec<StreamEvent>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    stop: bool,
}

impl AnthropicProvider {
    pub fn new(config: ProviderConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = self.endpoint("messages");
        let response = self
            .request(self.http.post(url), &Self::build_chat_body(&req, false))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(RouterError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RouterError::Parse(e.to_string()))?;
        Self::parse_chat_response(payload)
    }

    pub async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let url = self.endpoint("messages");
        let response = self
            .request(self.http.post(url), &Self::build_chat_body(&req, true))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(RouterError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let byte_stream = response.bytes_stream();
        let s = stream::try_unfold(
            (
                byte_stream,
                String::new(),
                None::<String>,
                Vec::<String>::new(),
                Vec::<StreamEvent>::new(),
                false,
                0_u64,
                0_u64,
            ),
            |(
                mut bytes,
                mut buffer,
                mut current_event,
                mut current_data,
                mut pending,
                mut done,
                mut input_tokens,
                mut output_tokens,
            )| async move {
                loop {
                    if let Some(event) = pending.pop() {
                        return Ok(Some((
                            event,
                            (
                                bytes,
                                buffer,
                                current_event,
                                current_data,
                                pending,
                                done,
                                input_tokens,
                                output_tokens,
                            ),
                        )));
                    }
                    if done {
                        return Ok(None);
                    }
                    match bytes.next().await {
                        Some(Ok(chunk)) => {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].trim_end_matches('\r').to_string();
                                buffer.drain(..=pos);

                                if line.is_empty() {
                                    if current_event.is_some() || !current_data.is_empty() {
                                        let event_name = current_event
                                            .take()
                                            .unwrap_or_else(|| "message".to_string());
                                        let data = current_data.join("\n");
                                        current_data.clear();

                                        if !data.is_empty() {
                                            let outcome =
                                                Self::parse_stream_payload(&event_name, &data)?;
                                            if let Some(tokens) = outcome.input_tokens {
                                                input_tokens = tokens;
                                            }
                                            if let Some(tokens) = outcome.output_tokens {
                                                output_tokens = tokens;
                                            }
                                            let mut emitted = outcome.events;
                                            if outcome.stop {
                                                if input_tokens > 0 || output_tokens > 0 {
                                                    emitted.push(StreamEvent::Usage(Usage {
                                                        prompt_tokens: input_tokens,
                                                        completion_tokens: output_tokens,
                                                        total_tokens: input_tokens
                                                            + output_tokens,
                                                    }));
                                                }
                                                emitted.push(StreamEvent::Done);
                                                done = true;
                                            }
                                            emitted.reverse();
                                            pending.extend(emitted);
                                            if done {
                                                break;
                                            }
                                        }
                                    }
                                    continue;
                                }

                                if let Some(value) = line.strip_prefix("event:") {
                                    current_event = Some(value.trim().to_string());
                                    continue;
                                }
                                if let Some(value) = line.strip_prefix("data:") {
                                    current_data.push(value.trim_start().to_string());
                                }
                            }
                        }
                        Some(Err(err)) => return Err(RouterError::Request(err.to_string())),
                        None => {
                            if input_tokens > 0 || output_tokens > 0 {
                                pending.push(StreamEvent::Done);
                                pending.push(StreamEvent::Usage(Usage {
                                    prompt_tokens: input_tokens,
                                    completion_tokens: output_tokens,
                                    total_tokens: input_tokens + output_tokens,
                                }));
                            } else {
                                pending.push(StreamEvent::Done);
                            }
                            done = true;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(s))
    }

    pub async fn embed(&self, _req: EmbeddingRequest) -> Result<EmbeddingResponse> {
        Err(RouterError::UnsupportedProtocol {
            provider: self.config.id.clone(),
            protocol: "AnthropicMessages embeddings".to_string(),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self
            .config
            .api_base
            .as_deref()
            .unwrap_or("https://api.anthropic.com/v1")
            .trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/{path}")
        } else {
            format!("{base}/v1/{path}")
        }
    }

    async fn request(
        &self,
        builder: reqwest::RequestBuilder,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let api_key = self
            .config
            .api_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| RouterError::MissingApiKey(self.config.id.clone()))?;
        builder
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| RouterError::Request(e.to_string()))
    }

    fn build_chat_body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        let (system, messages) = Self::messages_to_anthropic(&req.messages);
        let tools: Vec<serde_json::Value> = req.tools.iter().map(Self::tool_to_json).collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens.unwrap_or(2048),
            "stream": stream,
        });
        if let Some(system) = system {
            body["system"] = serde_json::Value::String(system);
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools);
            body["tool_choice"] = serde_json::json!({ "type": "auto" });
        }
        body
    }

    fn messages_to_anthropic(messages: &[Message]) -> (Option<String>, Vec<serde_json::Value>) {
        let mut system_parts = Vec::new();
        let mut out = Vec::new();

        for message in messages {
            match message.role {
                MessageRole::System => {
                    if let Some(content) = message.content.as_deref().filter(|text| !text.is_empty())
                    {
                        system_parts.push(content.to_string());
                    }
                }
                MessageRole::User => {
                    out.push(serde_json::json!({
                        "role": "user",
                        "content": Self::user_content_blocks(message),
                    }));
                }
                MessageRole::Assistant => {
                    let content = Self::assistant_content_blocks(message);
                    if !content.is_empty() {
                        out.push(serde_json::json!({
                            "role": "assistant",
                            "content": content,
                        }));
                    }
                }
                MessageRole::Tool => {
                    out.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
                            "content": message.content.clone().unwrap_or_default(),
                        }],
                    }));
                }
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };
        (system, out)
    }

    fn user_content_blocks(message: &Message) -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "type": "text",
            "text": message.content.clone().unwrap_or_default(),
        })]
    }

    fn assistant_content_blocks(message: &Message) -> Vec<serde_json::Value> {
        let mut blocks = Vec::new();
        if let Some(content) = message.content.as_deref().filter(|text| !text.is_empty()) {
            blocks.push(serde_json::json!({
                "type": "text",
                "text": content,
            }));
        }
        for call in &message.tool_calls {
            blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                "input": call.arguments,
            }));
        }
        blocks
    }

    fn tool_to_json(tool: &ToolDefinition) -> serde_json::Value {
        let tool_type = match tool.tool_type {
            ToolType::Function => "function",
        };
        serde_json::json!({
            "name": tool.function.name,
            "description": tool.function.description,
            "input_schema": tool.function.parameters,
            "type": tool_type,
        })
    }

    fn parse_chat_response(payload: serde_json::Value) -> Result<ChatResponse> {
        let (content, reasoning_content, tool_calls) =
            Self::parse_content_blocks(payload["content"].as_array())?;
        let raw_message = Self::build_assistant_raw_message(
            content.as_deref(),
            reasoning_content.as_deref(),
            &tool_calls,
        );
        Ok(ChatResponse {
            content,
            tool_calls,
            reasoning_content,
            usage: payload.get("usage").map(Self::usage_from_value),
            raw_message,
        })
    }

    fn parse_content_blocks(
        blocks: Option<&Vec<serde_json::Value>>,
    ) -> Result<(Option<String>, Option<String>, Vec<ToolCall>)> {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();

        for block in blocks.into_iter().flatten() {
            match block["type"].as_str().unwrap_or_default() {
                "text" => {
                    if let Some(text) = block["text"].as_str() {
                        content.push_str(text);
                    }
                }
                "thinking" | "redacted_thinking" => {
                    if let Some(text) = block["thinking"]
                        .as_str()
                        .or_else(|| block["text"].as_str())
                    {
                        reasoning.push_str(text);
                    }
                }
                "tool_use" => {
                    let id = block["id"]
                        .as_str()
                        .ok_or_else(|| RouterError::Parse("anthropic tool_use missing id".to_string()))?
                        .to_string();
                    let name = block["name"]
                        .as_str()
                        .ok_or_else(|| RouterError::Parse("anthropic tool_use missing name".to_string()))?
                        .to_string();
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: block["input"].clone(),
                    });
                }
                _ => {}
            }
        }

        Ok((
            if content.is_empty() { None } else { Some(content) },
            if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
            tool_calls,
        ))
    }

    fn parse_stream_payload(event: &str, data: &str) -> Result<StreamParseOutcome> {
        let payload: serde_json::Value =
            serde_json::from_str(data).map_err(|e| RouterError::Parse(e.to_string()))?;
        let mut out = StreamParseOutcome::default();

        match event {
            "message_start" => {
                out.input_tokens = payload["message"]["usage"]["input_tokens"].as_u64();
                out.output_tokens = payload["message"]["usage"]["output_tokens"].as_u64();
            }
            "message_delta" => {
                out.output_tokens = payload["usage"]["output_tokens"].as_u64();
            }
            "content_block_start" => {
                let block = &payload["content_block"];
                if block["type"].as_str() == Some("tool_use") {
                    out.events.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                        index: payload["index"].as_u64().unwrap_or(0) as usize,
                        id: block["id"].as_str().map(str::to_string),
                        name: block["name"].as_str().map(str::to_string),
                        arguments_fragment: Self::initial_tool_arguments(block.get("input")),
                    }));
                }
            }
            "content_block_delta" => {
                let delta = &payload["delta"];
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        if let Some(text) = delta["text"].as_str() {
                            out.events.push(StreamEvent::TextDelta(text.to_string()));
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta["thinking"].as_str() {
                            out.events.push(StreamEvent::ReasoningDelta(text.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        out.events.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                            index: payload["index"].as_u64().unwrap_or(0) as usize,
                            id: None,
                            name: None,
                            arguments_fragment: delta["partial_json"].as_str().map(str::to_string),
                        }));
                    }
                    _ => {}
                }
            }
            "message_stop" => out.stop = true,
            _ => {}
        }

        Ok(out)
    }

    fn initial_tool_arguments(input: Option<&serde_json::Value>) -> Option<String> {
        match input {
            Some(serde_json::Value::Object(map)) if !map.is_empty() => {
                serde_json::to_string(&serde_json::Value::Object(map.clone())).ok()
            }
            Some(serde_json::Value::Array(items)) if !items.is_empty() => {
                serde_json::to_string(&serde_json::Value::Array(items.clone())).ok()
            }
            Some(serde_json::Value::String(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        }
    }

    fn usage_from_value(value: &serde_json::Value) -> Usage {
        let prompt_tokens = value["input_tokens"].as_u64().unwrap_or(0);
        let completion_tokens = value["output_tokens"].as_u64().unwrap_or(0);
        Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }

    fn build_assistant_raw_message(
        content: Option<&str>,
        reasoning_content: Option<&str>,
        tool_calls: &[ToolCall],
    ) -> serde_json::Value {
        let content_val = content
            .filter(|text| !text.is_empty())
            .map(|text| serde_json::Value::String(text.to_string()))
            .unwrap_or(serde_json::Value::Null);

        let mut raw = serde_json::json!({
            "role": "assistant",
            "content": content_val,
        });
        if let Some(reasoning) = reasoning_content.filter(|text| !text.is_empty()) {
            raw["reasoning_content"] = serde_json::Value::String(reasoning.to_string());
        }
        if !tool_calls.is_empty() {
            raw["tool_calls"] = serde_json::Value::Array(
                tool_calls
                    .iter()
                    .map(|call| {
                        serde_json::json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string()),
                            }
                        })
                    })
                    .collect(),
            );
        }
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDefinition, Message, MessageRole, ToolDefinition, ToolType};

    #[test]
    fn build_chat_body_splits_system_and_tool_config() {
        let body = AnthropicProvider::build_chat_body(
            &ChatRequest {
                model: "claude-sonnet-4".to_string(),
                messages: vec![
                    Message {
                        role: MessageRole::System,
                        content: Some("be careful".to_string()),
                        ..Default::default()
                    },
                    Message {
                        role: MessageRole::User,
                        content: Some("hello".to_string()),
                        ..Default::default()
                    },
                ],
                tools: vec![ToolDefinition {
                    tool_type: ToolType::Function,
                    function: FunctionDefinition {
                        name: "get_weather".to_string(),
                        description: Some("weather".to_string()),
                        parameters: serde_json::json!({"type":"object"}),
                    },
                }],
                temperature: Some(0.2),
                max_tokens: Some(512),
            },
            true,
        );

        assert_eq!(body["system"], "be careful");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(body["tools"][0]["name"], "get_weather");
        assert_eq!(body["tool_choice"]["type"], "auto");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 512);
    }

    #[test]
    fn parse_chat_response_extracts_text_reasoning_and_tools() {
        let payload = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "step one" },
                { "type": "text", "text": "final answer" },
                { "type": "tool_use", "id": "tool_1", "name": "search", "input": { "q": "hello" } }
            ],
            "usage": {
                "input_tokens": 8,
                "output_tokens": 5
            }
        });

        let response = AnthropicProvider::parse_chat_response(payload).unwrap();
        assert_eq!(response.content.as_deref(), Some("final answer"));
        assert_eq!(response.reasoning_content.as_deref(), Some("step one"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "tool_1");
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments["q"], "hello");
        assert_eq!(response.usage.unwrap().total_tokens, 13);
        assert_eq!(response.raw_message["role"], "assistant");
    }

    #[test]
    fn parse_stream_payload_maps_text_reasoning_tool_and_usage() {
        let start = AnthropicProvider::parse_stream_payload(
            "message_start",
            r#"{"message":{"usage":{"input_tokens":9,"output_tokens":0}}}"#,
        )
        .unwrap();
        assert_eq!(start.input_tokens, Some(9));

        let thinking = AnthropicProvider::parse_stream_payload(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}"#,
        )
        .unwrap();
        assert!(matches!(
            &thinking.events[0],
            StreamEvent::ReasoningDelta(text) if text == "plan"
        ));

        let tool = AnthropicProvider::parse_stream_payload(
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"tool_1","name":"search","input":{}}}"#,
        )
        .unwrap();
        assert!(matches!(
            &tool.events[0],
            StreamEvent::ToolCallDelta(delta)
                if delta.index == 1
                && delta.id.as_deref() == Some("tool_1")
                && delta.name.as_deref() == Some("search")
        ));

        let json_delta = AnthropicProvider::parse_stream_payload(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"hello\"}"}}"#,
        )
        .unwrap();
        assert!(matches!(
            &json_delta.events[0],
            StreamEvent::ToolCallDelta(delta)
                if delta.arguments_fragment.as_deref() == Some("{\"q\":\"hello\"}")
        ));

        let stop = AnthropicProvider::parse_stream_payload("message_stop", r#"{}"#).unwrap();
        assert!(stop.stop);
    }
}
