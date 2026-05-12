use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::llm::client::{AbortFlag, LlmClient, ToolCall};

#[derive(Debug, Clone, Copy)]
struct TrimCandidate {
    start: usize,
    end: usize,
    priority: u8,
    estimated_tokens: u64,
}

impl TrimCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        self.priority < other.priority
            || (self.priority == other.priority
                && (self.estimated_tokens > other.estimated_tokens
                    || (self.estimated_tokens == other.estimated_tokens
                        && self.start < other.start)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    context_limit: u64,
    response_reserve: u64,
    target_budget: u64,
}

impl ContextBudget {
    pub fn new(context_limit: u64) -> Self {
        let response_reserve = Self::response_token_reserve(context_limit);
        let target_budget = context_limit
            .saturating_sub(response_reserve)
            .saturating_mul(92)
            / 100;
        Self {
            context_limit,
            response_reserve,
            target_budget,
        }
    }

    pub fn context_limit(&self) -> u64 {
        self.context_limit
    }

    pub fn response_reserve(&self) -> u64 {
        self.response_reserve
    }

    pub fn target_budget(&self) -> u64 {
        self.target_budget
    }

    pub fn estimate_context_tokens(&self, msgs: &[Value], tools: &[Value]) -> u64 {
        let message_tokens: u64 = msgs.iter().map(Self::estimate_message_tokens).sum();
        let tool_tokens: u64 = tools.iter().map(Self::estimate_tool_spec_tokens).sum();
        message_tokens + tool_tokens
    }

    pub fn needs_trim(&self, estimated_tokens: u64) -> bool {
        estimated_tokens > self.target_budget
    }

    pub fn trim_messages_to_budget(&self, msgs: &mut Vec<Value>, tools: &[Value]) {
        while self.estimate_context_tokens(msgs, tools) > self.target_budget && msgs.len() > 2 {
            let preserve_from = msgs
                .iter()
                .rposition(|m| message_role(m) == Some("user"))
                .unwrap_or_else(|| msgs.len().saturating_sub(1));
            let Some(candidate) = best_trim_candidate(msgs, preserve_from) else {
                break;
            };
            remove_context_entry_preserving_tool_groups(msgs, candidate.start, preserve_from);
        }
    }

    pub fn tool_result_max_chars(&self) -> usize {
        let max = (self.response_reserve.saturating_mul(4) / 3) as usize;
        max.clamp(2_000, 32_768)
    }

    pub fn truncate_tool_result(&self, text: &str) -> String {
        let max = self.tool_result_max_chars();
        let count = text.chars().count();
        if count <= max {
            return text.to_string();
        }
        let tail_budget = max * 3 / 10;
        let head_budget = max.saturating_sub(tail_budget).saturating_sub(80);
        let head: String = text.chars().take(head_budget).collect();
        let tail: String = text.chars().skip(count - tail_budget).collect();
        let omitted = count - head_budget - tail_budget;
        format!("{head}\n\n… [truncated {omitted} chars of {count} total] …\n\n{tail}")
    }

    fn estimate_message_tokens(message: &Value) -> u64 {
        let mut tokens = 8;
        if let Some(role) = message_role(message) {
            tokens += match role {
                "system" => 12,
                "user" => 10,
                "assistant" => 10,
                "tool" => 12,
                _ => 8,
            };
        }

        if let Some(content) = message.get("content") {
            tokens += Self::estimate_json_tokens(content);
        }
        if let Some(reasoning) = message.get("reasoning_content") {
            tokens += Self::estimate_json_tokens(reasoning);
        }
        if let Some(name) = message.get("name") {
            tokens += Self::estimate_json_tokens(name);
        }
        if let Some(tool_call_id) = message.get("tool_call_id") {
            tokens += Self::estimate_json_tokens(tool_call_id);
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
            tokens += 24;
            for call in tool_calls {
                tokens += 16;
                tokens += Self::estimate_json_tokens(&call["id"]);
                tokens += Self::estimate_json_tokens(&call["function"]["name"]);
                tokens += Self::estimate_json_tokens(&call["function"]["arguments"]);
            }
        }

        tokens
    }

    fn estimate_tool_spec_tokens(tool: &Value) -> u64 {
        12 + Self::estimate_json_tokens(tool)
    }

    fn estimate_json_tokens(value: &Value) -> u64 {
        match value {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Number(_) => 2,
            Value::String(text) => {
                let chars = text.chars().count() as u64;
                chars.div_ceil(4) + (text.matches('\n').count() as u64 * 2)
            }
            Value::Array(items) => {
                4 + items.iter().map(Self::estimate_json_tokens).sum::<u64>()
                    + items.len() as u64 * 2
            }
            Value::Object(map) => {
                6 + map
                    .iter()
                    .map(|(key, value)| {
                        (key.chars().count() as u64).div_ceil(4)
                            + 1
                            + Self::estimate_json_tokens(value)
                    })
                    .sum::<u64>()
            }
        }
    }

    fn response_token_reserve(limit: u64) -> u64 {
        (limit / 5).clamp(1_024, 12_288)
    }
}

pub fn message_role(message: &Value) -> Option<&str> {
    message.get("role").and_then(|role| role.as_str())
}

pub fn assistant_has_tool_calls(message: &Value) -> bool {
    message_role(message) == Some("assistant")
        && message
            .get("tool_calls")
            .and_then(|tool_calls| tool_calls.as_array())
            .is_some_and(|tool_calls| !tool_calls.is_empty())
}

pub fn tool_group_start_for_tool_message(msgs: &[Value], tool_idx: usize) -> Option<usize> {
    if message_role(msgs.get(tool_idx)?) != Some("tool") {
        return None;
    }

    let mut first_tool = tool_idx;
    while first_tool > 0 && message_role(&msgs[first_tool - 1]) == Some("tool") {
        first_tool -= 1;
    }

    first_tool
        .checked_sub(1)
        .filter(|idx| assistant_has_tool_calls(&msgs[*idx]))
}

pub fn remove_context_entry_preserving_tool_groups(
    msgs: &mut Vec<Value>,
    idx: usize,
    preserve_from: usize,
) {
    if idx >= preserve_from || idx >= msgs.len() {
        return;
    }

    let start = tool_group_start_for_tool_message(msgs, idx).unwrap_or(idx);
    let mut end = start + 1;
    if assistant_has_tool_calls(&msgs[start]) {
        while end < preserve_from && message_role(&msgs[end]) == Some("tool") {
            end += 1;
        }
    }

    msgs.drain(start..end);
}

fn best_trim_candidate(msgs: &[Value], preserve_from: usize) -> Option<TrimCandidate> {
    let mut idx = 0usize;
    let mut best: Option<TrimCandidate> = None;

    while idx < preserve_from {
        if message_role(&msgs[idx]) == Some("system") {
            idx += 1;
            continue;
        }

        let candidate = trim_candidate_at(msgs, idx, preserve_from)?;
        idx = candidate.end.max(idx + 1);
        if best
            .as_ref()
            .is_none_or(|current| candidate.is_better_than(current))
        {
            best = Some(candidate);
        }
    }

    best
}

fn trim_candidate_at(msgs: &[Value], idx: usize, preserve_from: usize) -> Option<TrimCandidate> {
    if idx >= preserve_from || idx >= msgs.len() {
        return None;
    }
    if message_role(&msgs[idx]) == Some("system") {
        return None;
    }

    let start = tool_group_start_for_tool_message(msgs, idx).unwrap_or(idx);
    if start >= preserve_from {
        return None;
    }
    let mut end = start + 1;
    let priority = if assistant_has_tool_calls(&msgs[start]) {
        while end < preserve_from && message_role(&msgs[end]) == Some("tool") {
            end += 1;
        }
        0
    } else {
        match message_role(&msgs[start]) {
            Some("assistant") => 1,
            Some("tool") => 1,
            Some("user") => 2,
            _ => 3,
        }
    };

    let estimated_tokens = msgs[start..end]
        .iter()
        .map(ContextBudget::estimate_message_tokens)
        .sum();

    Some(TrimCandidate {
        start,
        end,
        priority,
        estimated_tokens,
    })
}

pub struct PreparedToolLoopIteration {
    pub tools: Vec<Value>,
}

pub enum ToolLoopControl<T> {
    AppendToolResult { content: String },
    Return(T),
}

#[async_trait]
pub trait ToolLoopDelegate {
    type Output;

    fn loop_label(&self) -> &str;
    fn messages(&self) -> &Vec<Value>;
    fn messages_mut(&mut self) -> &mut Vec<Value>;
    fn max_tool_rounds(&self) -> usize;
    fn context_budget(&self) -> ContextBudget;

    async fn prepare_iteration(&mut self, iteration: usize) -> Result<PreparedToolLoopIteration>;
    async fn handle_tool_call(
        &mut self,
        tool_call: &ToolCall,
        budget: &ContextBudget,
    ) -> Result<ToolLoopControl<Self::Output>>;
    async fn finalize_text_response(&mut self, text: String) -> Result<Self::Output>;
    async fn on_round_limit_exceeded(
        &mut self,
        llm: Arc<LlmClient>,
        abort: &AbortFlag,
    ) -> Result<Self::Output>;

    fn should_truncate_tool_result(&self, _tool_name: &str) -> bool {
        true
    }

    async fn after_tool_result_appended(&mut self, _tool_name: &str) -> Result<()> {
        Ok(())
    }

    fn log_context(&self, _iteration: usize, _estimated_tokens: u64, _tools: &[Value]) {}

    fn aborted_output(&self) -> Self::Output;
}

pub async fn run_tool_loop<D>(
    delegate: &mut D,
    llm: Arc<LlmClient>,
    abort: &AbortFlag,
    on_token: &(dyn Fn(&str) + Send + Sync),
) -> Result<D::Output>
where
    D: ToolLoopDelegate + Send,
    D::Output: Send,
{
    let budget = delegate.context_budget();

    for iteration in 0..delegate.max_tool_rounds() {
        if abort.load(Ordering::Relaxed) {
            return Ok(delegate.aborted_output());
        }

        let prepared = delegate.prepare_iteration(iteration).await?;
        let estimated_tokens = budget.estimate_context_tokens(delegate.messages(), &prepared.tools);
        if budget.needs_trim(estimated_tokens) {
            budget.trim_messages_to_budget(delegate.messages_mut(), &prepared.tools);
        }
        let estimated_tokens = budget.estimate_context_tokens(delegate.messages(), &prepared.tools);
        delegate.log_context(iteration, estimated_tokens, &prepared.tools);

        let response = match llm
            .chat_with_tools_stream(delegate.messages().clone(), prepared.tools, on_token, abort)
            .await?
        {
            Some(response) => response,
            None => return Ok(delegate.aborted_output()),
        };

        if response.tool_calls.is_empty() {
            return delegate
                .finalize_text_response(response.content.unwrap_or_default())
                .await;
        }

        delegate.messages_mut().push(response.raw_message);
        for tool_call in &response.tool_calls {
            match delegate.handle_tool_call(tool_call, &budget).await? {
                ToolLoopControl::AppendToolResult { content } => {
                    let content = if delegate.should_truncate_tool_result(&tool_call.name) {
                        budget.truncate_tool_result(&content)
                    } else {
                        content
                    };
                    delegate.messages_mut().push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_call.id,
                        "content": content,
                    }));
                    delegate.after_tool_result_appended(&tool_call.name).await?;
                }
                ToolLoopControl::Return(output) => return Ok(output),
            }

            if abort.load(Ordering::Relaxed) {
                return Ok(delegate.aborted_output());
            }
        }
    }

    delegate.on_round_limit_exceeded(llm, abort).await
}
