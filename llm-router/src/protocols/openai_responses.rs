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
pub struct OpenAiResponsesProvider {
    config: ProviderConfig,
    http: reqwest::Client,
}

impl OpenAiResponsesProvider {
    pub fn new(config: ProviderConfig, http: reqwest::Client) -> Self {
        Self { config, http }
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = self.endpoint("responses");
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
        let url = self.endpoint("responses");
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
                VecDeque::<StreamEvent>::new(),
                false,
            ),
            |(
                mut bytes,
                mut buffer,
                mut current_event,
                mut current_data,
                mut pending,
                mut done,
            )| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Ok(Some((
                            event,
                            (bytes, buffer, current_event, current_data, pending, done),
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
                                            let mut emitted =
                                                Self::parse_stream_payload(&event_name, &data)?;
                                            if event_name == "response.completed" {
                                                emitted.push(StreamEvent::Done);
                                                done = true;
                                            }
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
                            pending.push_back(StreamEvent::Done);
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
            protocol: "OpenAiResponses embeddings".to_string(),
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
        let input = Self::messages_to_input(&req.messages);
        let tools: Vec<serde_json::Value> = req.tools.iter().map(Self::tool_to_json).collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "input": input,
            "stream": stream,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_output_tokens"] = serde_json::json!(max_tokens);
        }
        body
    }

    fn messages_to_input(messages: &[Message]) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for message in messages {
            match message.role {
                MessageRole::Tool => {
                    out.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": message.tool_call_id.clone().unwrap_or_default(),
                        "output": message.content.clone().unwrap_or_default(),
                    }));
                }
                MessageRole::Assistant => {
                    if let Some(content) =
                        message.content.as_deref().filter(|text| !text.is_empty())
                    {
                        out.push(serde_json::json!({
                            "role": "assistant",
                            "content": [{
                                "type": "input_text",
                                "text": content,
                            }],
                        }));
                    }
                    for call in &message.tool_calls {
                        out.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": call.id,
                            "name": call.name,
                            "arguments": serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string()),
                        }));
                    }
                }
                MessageRole::System | MessageRole::User => {
                    out.push(serde_json::json!({
                        "role": match message.role {
                            MessageRole::System => "system",
                            _ => "user",
                        },
                        "content": [{
                            "type": "input_text",
                            "text": message.content.clone().unwrap_or_default(),
                        }],
                    }));
                }
            }
        }
        out
    }

    fn tool_to_json(tool: &ToolDefinition) -> serde_json::Value {
        let tool_type = match tool.tool_type {
            ToolType::Function => "function",
        };
        serde_json::json!({
            "type": tool_type,
            "name": tool.function.name,
            "description": tool.function.description,
            "parameters": tool.function.parameters,
        })
    }

    fn parse_chat_response(payload: serde_json::Value) -> Result<ChatResponse> {
        let (content, reasoning_content, tool_calls) =
            Self::parse_output_items(payload["output"].as_array())?;
        let content = content.or_else(|| payload["output_text"].as_str().map(str::to_string));
        let usage = payload.get("usage").map(Self::usage_from_value);
        let raw_message = Self::build_assistant_raw_message(
            content.as_deref(),
            reasoning_content.as_deref(),
            &tool_calls,
        );
        Ok(ChatResponse {
            content,
            tool_calls,
            reasoning_content,
            usage,
            raw_message,
        })
    }

    fn parse_output_items(
        items: Option<&Vec<serde_json::Value>>,
    ) -> Result<(Option<String>, Option<String>, Vec<ToolCall>)> {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();

        for item in items.into_iter().flatten() {
            match item["type"].as_str().unwrap_or_default() {
                "message" => {
                    if let Some(content_items) = item["content"].as_array() {
                        for content_item in content_items {
                            match content_item["type"].as_str().unwrap_or_default() {
                                "output_text" | "text" => {
                                    if let Some(text) = content_item["text"].as_str() {
                                        content.push_str(text);
                                    }
                                }
                                "reasoning" => {
                                    if let Some(text) = content_item["text"].as_str() {
                                        reasoning.push_str(text);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "reasoning" => {
                    if let Some(text) = item["summary"][0]["text"]
                        .as_str()
                        .or_else(|| item["text"].as_str())
                    {
                        reasoning.push_str(text);
                    }
                }
                "function_call" => {
                    let arguments = item["arguments"].as_str().unwrap_or("{}");
                    tool_calls.push(ToolCall {
                        id: item["call_id"]
                            .as_str()
                            .or_else(|| item["id"].as_str())
                            .unwrap_or_default()
                            .to_string(),
                        name: item["name"].as_str().unwrap_or_default().to_string(),
                        arguments: serde_json::from_str(arguments)
                            .unwrap_or_else(|_| serde_json::json!({})),
                    });
                }
                _ => {}
            }
        }

        Ok((
            if content.is_empty() {
                None
            } else {
                Some(content)
            },
            if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
            tool_calls,
        ))
    }

    fn parse_stream_payload(_event: &str, data: &str) -> Result<Vec<StreamEvent>> {
        let payload: serde_json::Value =
            serde_json::from_str(data).map_err(|e| RouterError::Parse(e.to_string()))?;
        let mut out = Vec::new();
        match payload["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                if let Some(text) = payload["delta"].as_str() {
                    out.push(StreamEvent::TextDelta(text.to_string()));
                }
            }
            "response.reasoning.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(text) = payload["delta"].as_str() {
                    out.push(StreamEvent::ReasoningDelta(text.to_string()));
                }
            }
            "response.output_item.added" => {
                let item = &payload["item"];
                if item["type"].as_str() == Some("function_call") {
                    out.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                        index: payload["output_index"].as_u64().unwrap_or(0) as usize,
                        id: item["call_id"]
                            .as_str()
                            .or_else(|| item["id"].as_str())
                            .map(str::to_string),
                        name: item["name"].as_str().map(str::to_string),
                        arguments_fragment: item["arguments"].as_str().map(str::to_string),
                    }));
                }
            }
            "response.function_call_arguments.delta" => {
                out.push(StreamEvent::ToolCallDelta(ToolCallDelta {
                    index: payload["output_index"].as_u64().unwrap_or(0) as usize,
                    id: None,
                    name: None,
                    arguments_fragment: payload["delta"].as_str().map(str::to_string),
                }));
            }
            "response.completed" => {
                if let Some(usage) = payload.get("response").and_then(|value| value.get("usage")) {
                    out.push(StreamEvent::Usage(Self::usage_from_value(usage)));
                }
            }
            _ => {}
        }
        Ok(out)
    }

    fn usage_from_value(value: &serde_json::Value) -> Usage {
        Usage {
            prompt_tokens: value["input_tokens"].as_u64().unwrap_or(0),
            completion_tokens: value["output_tokens"].as_u64().unwrap_or(0),
            total_tokens: value["total_tokens"].as_u64().unwrap_or_else(|| {
                value["input_tokens"].as_u64().unwrap_or(0)
                    + value["output_tokens"].as_u64().unwrap_or(0)
            }),
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
    fn build_chat_body_maps_messages_and_tools() {
        let body = OpenAiResponsesProvider::build_chat_body(
            &ChatRequest {
                model: "gpt-5".to_string(),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: Some("hello".to_string()),
                    ..Default::default()
                }],
                tools: vec![ToolDefinition {
                    tool_type: ToolType::Function,
                    function: FunctionDefinition {
                        name: "lookup".to_string(),
                        description: Some("lookup".to_string()),
                        parameters: serde_json::json!({"type":"object"}),
                    },
                }],
                temperature: Some(0.4),
                max_tokens: Some(256),
            },
            true,
        );
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["text"], "hello");
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn parse_chat_response_maps_text_reasoning_and_function_calls() {
        let payload = serde_json::json!({
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{ "text": "think" }]
                },
                {
                    "type": "message",
                    "content": [{ "type": "output_text", "text": "final answer" }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "lookup",
                    "arguments": "{\"city\":\"Shanghai\"}"
                }
            ],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 6,
                "total_tokens": 16
            }
        });

        let response = OpenAiResponsesProvider::parse_chat_response(payload).unwrap();
        assert_eq!(response.content.as_deref(), Some("final answer"));
        assert_eq!(response.reasoning_content.as_deref(), Some("think"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "lookup");
        assert_eq!(response.tool_calls[0].arguments["city"], "Shanghai");
        assert_eq!(response.usage.unwrap().total_tokens, 16);
    }

    #[test]
    fn parse_stream_payload_maps_text_reasoning_tool_and_usage() {
        let text = OpenAiResponsesProvider::parse_stream_payload(
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"Hel"}"#,
        )
        .unwrap();
        assert!(matches!(&text[0], StreamEvent::TextDelta(delta) if delta == "Hel"));

        let reasoning = OpenAiResponsesProvider::parse_stream_payload(
            "response.reasoning.delta",
            r#"{"type":"response.reasoning.delta","delta":"think"}"#,
        )
        .unwrap();
        assert!(matches!(&reasoning[0], StreamEvent::ReasoningDelta(delta) if delta == "think"));

        let tool = OpenAiResponsesProvider::parse_stream_payload(
            "response.output_item.added",
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"city\":\"Shanghai\"}"}}"#,
        )
        .unwrap();
        assert!(matches!(
            &tool[0],
            StreamEvent::ToolCallDelta(delta)
                if delta.id.as_deref() == Some("call_1")
                && delta.name.as_deref() == Some("lookup")
        ));

        let usage = OpenAiResponsesProvider::parse_stream_payload(
            "response.completed",
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":4,"output_tokens":5,"total_tokens":9}}}"#,
        )
        .unwrap();
        assert!(matches!(&usage[0], StreamEvent::Usage(tokens) if tokens.total_tokens == 9));
    }
}
