use std::collections::VecDeque;
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
pub struct OllamaProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = self.endpoint("chat");
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
        let url = self.endpoint("chat");
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
                VecDeque::<StreamEvent>::new(),
                false,
            ),
            |(mut bytes, mut buffer, mut pending, mut done)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
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
                                if line.trim().is_empty() {
                                    continue;
                                }
                                let payload: serde_json::Value = serde_json::from_str(&line)
                                    .map_err(|e| RouterError::Parse(e.to_string()))?;
                                let mut emitted = Self::parse_stream_events(&payload)?;
                                if payload["done"].as_bool().unwrap_or(false) {
                                    emitted.push(StreamEvent::Done);
                                    done = true;
                                }
                                pending.extend(emitted);
                                if done {
                                    break;
                                }
                            }
                        }
                        Some(Err(err)) => return Err(RouterError::Request(err.to_string())),
                        None => {
                            pending.push_back(StreamEvent::Done);
                            done = true;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(s))
    }

    pub async fn embed(&self, req: EmbeddingRequest) -> Result<EmbeddingResponse> {
        let url = self.endpoint("embed");
        let response = self
            .request(
                self.http.post(url),
                &serde_json::json!({
                    "model": req.model,
                    "input": req.input,
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
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RouterError::Parse(e.to_string()))?;
        if let Some(items) = payload["embeddings"].as_array() {
            return Ok(EmbeddingResponse {
                vectors: items
                    .iter()
                    .filter_map(|item| {
                        Some(
                            item.as_array()?
                                .iter()
                                .filter_map(|value| value.as_f64().map(|number| number as f32))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect(),
                usage: None,
            });
        }
        if let Some(item) = payload["embedding"].as_array() {
            return Ok(EmbeddingResponse {
                vectors: vec![item
                    .iter()
                    .filter_map(|value| value.as_f64().map(|number| number as f32))
                    .collect()],
                usage: None,
            });
        }
        Err(RouterError::Parse(
            "ollama embed response missing embedding vector".to_string(),
        ))
    }

    fn endpoint(&self, path: &str) -> String {
        let base = self
            .config
            .api_base
            .as_deref()
            .unwrap_or("http://127.0.0.1:11434/api")
            .trim_end_matches('/');
        if base.ends_with("/api") {
            format!("{base}/{path}")
        } else {
            format!("{base}/api/{path}")
        }
    }

    async fn request(
        &self,
        builder: reqwest::RequestBuilder,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let mut builder = builder.header(CONTENT_TYPE, "application/json").json(body);
        if let Some(api_key) = self
            .config
            .api_key
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            builder = builder.bearer_auth(api_key);
        }
        builder
            .send()
            .await
            .map_err(|e| RouterError::Request(e.to_string()))
    }

    fn build_chat_body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> =
            req.messages.iter().map(Self::message_to_json).collect();
        let tools: Vec<serde_json::Value> = req.tools.iter().map(Self::tool_to_json).collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
        });
        let mut options = serde_json::Map::new();
        if let Some(temperature) = req.temperature {
            options.insert("temperature".to_string(), serde_json::json!(temperature));
        }
        if let Some(max_tokens) = req.max_tokens {
            options.insert("num_predict".to_string(), serde_json::json!(max_tokens));
        }
        if !options.is_empty() {
            body["options"] = serde_json::Value::Object(options);
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools);
        }
        body
    }

    fn message_to_json(message: &Message) -> serde_json::Value {
        if let Some(raw) = message.raw_message.as_ref().and_then(|raw| raw.as_object()) {
            return serde_json::Value::Object(raw.clone());
        }
        let mut out = serde_json::json!({
            "role": match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            },
            "content": message.content.clone().unwrap_or_default(),
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
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments,
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
        let message = payload["message"].clone();
        let content = Self::extract_text_content(message.get("content"));
        let reasoning_content = message["reasoning_content"].as_str().map(str::to_string);
        let tool_calls = Self::parse_tool_calls(message.get("tool_calls"))?;
        let usage = if payload["prompt_eval_count"].is_number() || payload["eval_count"].is_number()
        {
            Some(Usage {
                prompt_tokens: payload["prompt_eval_count"].as_u64().unwrap_or(0),
                completion_tokens: payload["eval_count"].as_u64().unwrap_or(0),
                total_tokens: payload["prompt_eval_count"].as_u64().unwrap_or(0)
                    + payload["eval_count"].as_u64().unwrap_or(0),
            })
        } else {
            None
        };
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
        if let Some(message) = payload.get("message") {
            out.push(StreamEvent::RawAssistantMessageDelta {
                delta: message.clone(),
            });
            if let Some(text) = message["content"].as_str().filter(|text| !text.is_empty()) {
                out.push(StreamEvent::TextDelta(text.to_string()));
            }
            if let Some(reasoning) = message["reasoning_content"].as_str() {
                out.push(StreamEvent::ReasoningDelta(reasoning.to_string()));
            }
            if let Some(tool_calls) = message.get("tool_calls").and_then(|value| value.as_array()) {
                for (index, item) in tool_calls.iter().enumerate() {
                    out.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                        index,
                        id: item["id"].as_str().map(str::to_string).or_else(|| {
                            item["function"]["name"]
                                .as_str()
                                .map(|name| format!("tool_call_{index}_{name}"))
                        }),
                        name: item["function"]["name"].as_str().map(str::to_string),
                        arguments_fragment: item
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                            .and_then(Self::serialize_arguments_value),
                    }));
                }
            }
        }
        if payload["done"].as_bool().unwrap_or(false)
            && (payload["prompt_eval_count"].is_number() || payload["eval_count"].is_number())
        {
            out.push(StreamEvent::Usage(Usage {
                prompt_tokens: payload["prompt_eval_count"].as_u64().unwrap_or(0),
                completion_tokens: payload["eval_count"].as_u64().unwrap_or(0),
                total_tokens: payload["prompt_eval_count"].as_u64().unwrap_or(0)
                    + payload["eval_count"].as_u64().unwrap_or(0),
            }));
        }
        Ok(out)
    }

    fn extract_text_content(content: Option<&serde_json::Value>) -> Option<String> {
        match content {
            Some(serde_json::Value::String(text)) if !text.is_empty() => Some(text.clone()),
            Some(serde_json::Value::Array(items)) => {
                let mut text = String::new();
                for item in items {
                    if let Some(part) = item["text"].as_str().or_else(|| item.as_str()) {
                        text.push_str(part);
                    }
                }
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        }
    }

    fn parse_tool_calls(value: Option<&serde_json::Value>) -> Result<Vec<ToolCall>> {
        Ok(value
            .and_then(|item| item.as_array())
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| ToolCall {
                        id: item["id"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("tool_call_{index}")),
                        name: item["function"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        arguments: item["function"]
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({})),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn serialize_arguments_value(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Null => None,
            serde_json::Value::String(text) => Some(text.clone()),
            other => serde_json::to_string(other).ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDefinition, Message, MessageRole, ToolDefinition, ToolType};

    #[test]
    fn build_chat_body_includes_tools_and_options() {
        let body = OllamaProvider::build_chat_body(
            &ChatRequest {
                model: "qwen3:8b".to_string(),
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
                        parameters: serde_json::json!({"type":"object"}),
                    },
                }],
                temperature: Some(0.5),
                max_tokens: Some(128),
            },
            true,
        );
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(body["options"]["temperature"], 0.5);
        assert_eq!(body["options"]["num_predict"], 128);
    }

    #[test]
    fn parse_chat_response_maps_tool_calls_and_usage() {
        let payload = serde_json::json!({
            "message": {
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "search",
                        "arguments": { "q": "hello" }
                    }
                }]
            },
            "prompt_eval_count": 11,
            "eval_count": 7
        });
        let response = OllamaProvider::parse_chat_response(payload).unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments["q"], "hello");
        assert_eq!(response.usage.unwrap().total_tokens, 18);
    }

    #[test]
    fn parse_stream_events_maps_text_tool_and_usage() {
        let payload = serde_json::json!({
            "message": {
                "content": "Hel",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {
                        "name": "lookup",
                        "arguments": { "city": "Shanghai" }
                    }
                }]
            },
            "done": true,
            "prompt_eval_count": 4,
            "eval_count": 3
        });
        let events = OllamaProvider::parse_stream_events(&payload).unwrap();
        assert!(matches!(
            &events[0],
            StreamEvent::RawAssistantMessageDelta { delta }
                if delta["content"] == "Hel"
        ));
        assert!(matches!(&events[1], StreamEvent::TextDelta(text) if text == "Hel"));
        assert!(matches!(
            &events[2],
            StreamEvent::ToolCallDelta(delta)
                if delta.name.as_deref() == Some("lookup")
        ));
        assert!(matches!(&events[3], StreamEvent::Usage(usage) if usage.total_tokens == 7));
    }
}
