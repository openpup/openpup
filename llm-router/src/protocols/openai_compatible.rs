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
pub struct OpenAiCompatibleProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

#[derive(serde::Deserialize)]
struct EmbeddingResponsePayload {
    data: Vec<EmbeddingData>,
    #[serde(default)]
    usage: Option<UsagePayload>,
}

#[derive(serde::Deserialize)]
struct EmbeddingData {
    #[serde(default)]
    index: usize,
    embedding: Vec<f32>,
}

#[derive(serde::Deserialize, Default)]
struct UsagePayload {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: ProviderConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = self.endpoint("chat/completions");
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
        let url = self.endpoint("chat/completions");
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
            (byte_stream, String::new(), Vec::<StreamEvent>::new(), false),
            |(mut bytes, mut buffer, mut pending, mut done)| async move {
                loop {
                    if let Some(event) = pending.pop() {
                        return Ok(Some((event, (bytes, buffer, pending, done))));
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
                                if !line.starts_with("data: ") {
                                    continue;
                                }
                                let data = &line["data: ".len()..];
                                if data == "[DONE]" {
                                    pending.push(StreamEvent::Done);
                                    done = true;
                                    break;
                                }
                                let val: serde_json::Value = serde_json::from_str(data)
                                    .map_err(|e| RouterError::Parse(e.to_string()))?;
                                let mut emitted = Self::parse_stream_events(&val)?;
                                emitted.reverse();
                                pending.extend(emitted);
                            }
                        }
                        Some(Err(err)) => return Err(RouterError::Request(err.to_string())),
                        None => {
                            pending.push(StreamEvent::Done);
                            done = true;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(s))
    }

    pub async fn embed(&self, req: EmbeddingRequest) -> Result<EmbeddingResponse> {
        let url = self.endpoint("embeddings");
        let response = self
            .request(
                self.http.post(url),
                &serde_json::json!({
                    "model": req.model,
                    "input": req.input,
                    "encoding_format": "float",
                }),
            )
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(RouterError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let mut payload: EmbeddingResponsePayload = response
            .json()
            .await
            .map_err(|e| RouterError::Parse(e.to_string()))?;
        payload.data.sort_by_key(|item| item.index);
        Ok(EmbeddingResponse {
            vectors: payload.data.into_iter().map(|item| item.embedding).collect(),
            usage: payload.usage.map(Self::usage_from_payload),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self
            .config
            .api_base
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
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
            .bearer_auth(api_key)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| RouterError::Request(e.to_string()))
    }

    fn build_chat_body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req.messages.iter().map(Self::message_to_json).collect();
        let tools: Vec<serde_json::Value> = req.tools.iter().map(Self::tool_to_json).collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
        });
        if stream {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools);
            body["tool_choice"] = serde_json::Value::String("auto".to_string());
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        body
    }

    fn message_to_json(message: &Message) -> serde_json::Value {
        let mut out = serde_json::json!({
            "role": match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            },
            "content": message.content.clone(),
        });
        if let Some(name) = &message.name {
            out["name"] = serde_json::Value::String(name.clone());
        }
        if let Some(tool_call_id) = &message.tool_call_id {
            out["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
        }
        if let Some(reasoning) = &message.reasoning_content {
            out["reasoning_content"] = serde_json::Value::String(reasoning.clone());
        }
        if !message.tool_calls.is_empty() {
            out["tool_calls"] = serde_json::Value::Array(
                message
                    .tool_calls
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
        out
    }

    fn tool_to_json(tool: &ToolDefinition) -> serde_json::Value {
        let tool_type = match tool.tool_type {
            ToolType::Function => "function",
        };
        serde_json::json!({
            "type": tool_type,
            "function": {
                "name": tool.function.name,
                "description": tool.function.description,
                "parameters": tool.function.parameters,
            }
        })
    }

    fn parse_chat_response(payload: serde_json::Value) -> Result<ChatResponse> {
        let message = payload["choices"][0]["message"].clone();
        let content = message["content"]
            .as_str()
            .filter(|item| !item.is_empty())
            .map(str::to_string);
        let reasoning_content = message["reasoning_content"].as_str().map(str::to_string);
        let tool_calls = message["tool_calls"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(ToolCall {
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
        let usage = payload.get("usage").map(Self::usage_from_value);
        Ok(ChatResponse {
            content,
            tool_calls,
            reasoning_content,
            usage,
            raw_message: message,
        })
    }

    fn parse_stream_events(payload: &serde_json::Value) -> Result<Vec<StreamEvent>> {
        let mut out = Vec::new();
        if let Some(usage) = payload.get("usage") {
            out.push(StreamEvent::Usage(Self::usage_from_value(usage)));
        }
        if let Some(delta) = payload["choices"][0].get("delta") {
            if let Some(content) = delta["content"].as_str() {
                out.push(StreamEvent::TextDelta(content.to_string()));
            }
            if let Some(reasoning) = delta["reasoning_content"].as_str() {
                out.push(StreamEvent::ReasoningDelta(reasoning.to_string()));
            }
            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for item in tool_calls {
                    out.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                        index: item["index"].as_u64().unwrap_or(0) as usize,
                        id: item["id"].as_str().map(str::to_string),
                        name: item["function"]["name"].as_str().map(str::to_string),
                        arguments_fragment: item["function"]["arguments"]
                            .as_str()
                            .map(str::to_string),
                    }));
                }
            }
        }
        Ok(out)
    }

    fn usage_from_value(value: &serde_json::Value) -> Usage {
        Usage {
            prompt_tokens: value["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: value["completion_tokens"].as_u64().unwrap_or(0),
            total_tokens: value["total_tokens"].as_u64().unwrap_or(0),
        }
    }

    fn usage_from_payload(value: UsagePayload) -> Usage {
        Usage {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDefinition, Message, MessageRole, ToolDefinition, ToolType};

    #[test]
    fn build_chat_body_includes_tools_and_stream_options() {
        let body = OpenAiCompatibleProvider::build_chat_body(
            &ChatRequest {
                model: "gpt-4o".to_string(),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: Some("hello".to_string()),
                    ..Default::default()
                }],
                tools: vec![ToolDefinition {
                    tool_type: ToolType::Function,
                    function: FunctionDefinition {
                        name: "get_weather".to_string(),
                        description: Some("weather".to_string()),
                        parameters: serde_json::json!({"type": "object"}),
                    },
                }],
                temperature: Some(0.4),
                max_tokens: Some(256),
            },
            true,
        );

        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(body["tool_choice"], "auto");
        let temperature = body["temperature"].as_f64().unwrap();
        assert!((temperature - 0.4).abs() < 1e-6);
        assert_eq!(body["max_tokens"], 256);
    }

    #[test]
    fn parse_chat_response_extracts_reasoning_and_tool_calls() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "final answer",
                    "reasoning_content": "hidden chain",
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Shanghai\"}"
                        }
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        });

        let response = OpenAiCompatibleProvider::parse_chat_response(payload).unwrap();
        assert_eq!(response.content.as_deref(), Some("final answer"));
        assert_eq!(response.reasoning_content.as_deref(), Some("hidden chain"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "get_weather");
        assert_eq!(response.tool_calls[0].arguments["city"], "Shanghai");
        assert_eq!(response.usage.unwrap().total_tokens, 30);
    }

    #[test]
    fn parse_stream_events_extracts_text_reasoning_tools_and_usage() {
        let payload = serde_json::json!({
            "choices": [{
                "delta": {
                    "content": "Hel",
                    "reasoning_content": "think",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": {
                            "name": "search",
                            "arguments": "{\"q\":\"hello\""
                        }
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 2,
                "total_tokens": 3
            }
        });

        let events = OpenAiCompatibleProvider::parse_stream_events(&payload).unwrap();
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], StreamEvent::Usage(usage) if usage.total_tokens == 3));
        assert!(matches!(&events[1], StreamEvent::TextDelta(text) if text == "Hel"));
        assert!(matches!(&events[2], StreamEvent::ReasoningDelta(text) if text == "think"));
        assert!(matches!(
            &events[3],
            StreamEvent::ToolCallDelta(delta)
                if delta.index == 0
                && delta.id.as_deref() == Some("call_1")
                && delta.name.as_deref() == Some("search")
        ));
    }

    #[test]
    fn embed_usage_payload_maps_cleanly() {
        let usage = OpenAiCompatibleProvider::usage_from_payload(UsagePayload {
            prompt_tokens: 4,
            completion_tokens: 0,
            total_tokens: 4,
        });
        assert_eq!(usage.prompt_tokens, 4);
        assert_eq!(usage.total_tokens, 4);
    }
}
