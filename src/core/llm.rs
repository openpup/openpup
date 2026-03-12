//! LLM 客户端：从 config.toml + env 加载统一配置，通过 OpenAI 兼容 /chat/completions 调用。
//! 提供 chat_once、chat_with_memory、tool_planner 等高层 API，供 Agent / Planner 使用。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{LlmConfigDisk, OpenpupConfig};
use crate::core::memory;
use crate::tools::integrations::net;

/// LLM 运行时配置（供 agent 使用），从 config.toml + env 加载。
#[derive(Debug, Clone)]
pub struct OpenpupLlmConfig {
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub api_key: Option<String>,
}

/// 从 config.toml + env 加载 LLM 配置。
///
/// 环境变量（作为 config 的后备）：  
/// - OPENPUP_LLM_BASE_URL  
/// - OPENPUP_LLM_MODEL  
/// - OPENPUP_LLM_TEMPERATURE  
/// - OPENPUP_LLM_API_KEY
pub fn load_openai_from_config(cfg: &OpenpupConfig) -> Result<OpenpupLlmConfig> {
    // 1) 首选 config.llm（只含非敏感字段）
    let from_cfg: Option<LlmConfigDisk> = cfg.llm.clone();

    let base_url = if let Some(ref llm_cfg) = from_cfg {
        llm_cfg.base_url.clone()
    } else {
        std::env::var("OPENPUP_LLM_BASE_URL")
            .map_err(|_| anyhow!("OPENPUP_LLM_BASE_URL is required for OpenAI provider"))?
    };

    let model = if let Some(ref llm_cfg) = from_cfg {
        llm_cfg.model.clone()
    } else {
        std::env::var("OPENPUP_LLM_MODEL").unwrap_or_else(|_| "gpt-4.1-mini".to_string())
    };

    let temperature = if let Some(ref llm_cfg) = from_cfg {
        llm_cfg.temperature
    } else {
        std::env::var("OPENPUP_LLM_TEMPERATURE")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.7)
    };

    // API key：优先使用配置文件中的 llm.api_key，其次回退到环境变量。
    let api_key = if let Some(ref llm_cfg) = from_cfg {
        llm_cfg
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENPUP_LLM_API_KEY").ok())
    } else {
        std::env::var("OPENPUP_LLM_API_KEY").ok()
    };

    Ok(OpenpupLlmConfig {
        base_url,
        model,
        temperature,
        api_key,
    })
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageOut,
}

#[derive(Debug, Deserialize)]
struct ChatMessageOut {
    content: String,
}

/// 最小 LLM 调用：给定 system + user，返回一次回复文本（OpenAI 兼容 HTTP）。
pub async fn openai_complete(
    cfg: &OpenpupLlmConfig,
    system_prompt: &str,
    user_message: &str,
) -> Result<String> {
    // 统一走 /chat/completions，适配 SiliconFlow / OpenAI 兼容网关。
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_message.to_string(),
    });

    let req_body = ChatRequest {
        model: cfg.model.clone(),
        messages,
        temperature: cfg.temperature,
    };

    let client = net::async_client()?;
    let mut builder = client.post(&url).json(&req_body);
    if let Some(key) = &cfg.api_key {
        builder = builder.bearer_auth(key);
    }

    let resp = match builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!(
                "openpup agent llm transport error: url={} model={} error={e:#}",
                url, cfg.model
            );
            return Err(anyhow!("LLM HTTP request failed before response: {e:#}"));
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        eprintln!(
            "openpup agent llm error: url={} model={} status={} body={}",
            url, cfg.model, status, text
        );
        return Err(anyhow!(
            "LLM request failed: status = {}, body = {}",
            status,
            text
        ));
    }

    let parsed: ChatResponse = resp.json().await?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("LLM response has no choices"))?;

    Ok(choice.message.content)
}

/// 带本地语义记忆检索的补全接口：
/// - 根据用户输入从 `memory::semantic_items` 中检索若干条相关记忆；
/// - 将这些记忆追加到 system prompt 末尾，再调用 `openai_complete`。
pub async fn openai_complete_with_memory(
    cfg: &OpenpupLlmConfig,
    system_prompt: &str,
    user_message: &str,
    semantic_kind: Option<&str>,
    memory_limit: usize,
) -> Result<String> {
    let mut extended_system = system_prompt.to_string();

    if memory_limit > 0 {
        if let Ok(items) = memory::search_semantic_items(semantic_kind, user_message, memory_limit)
        {
            if !items.is_empty() {
                let mut buf = String::new();
                buf.push_str("### Relevant personal notes (from local memory)\n");
                for it in items {
                    let mut content = it.content.trim().to_string();
                    let max_len = 400usize;
                    if content.len() > max_len {
                        content.truncate(max_len);
                        content.push_str(" ...");
                    }
                    buf.push_str("- ");
                    buf.push_str(&content);
                    buf.push('\n');
                }
                extended_system.push_str("\n\n");
                extended_system.push_str(&buf);
            }
        }
    }

    openai_complete(cfg, &extended_system, user_message).await
}

/// 高层封装：单轮对话（不带本地语义记忆检索）。
pub async fn chat_once(
    cfg: &OpenpupLlmConfig,
    system_prompt: &str,
    user_message: &str,
) -> Result<String> {
    openai_complete(cfg, system_prompt, user_message).await
}

/// 高层封装：带本地语义记忆检索的单轮对话。
pub async fn chat_with_memory(
    cfg: &OpenpupLlmConfig,
    system_prompt: &str,
    user_message: &str,
    semantic_kind: Option<&str>,
    memory_limit: usize,
) -> Result<String> {
    openai_complete_with_memory(
        cfg,
        system_prompt,
        user_message,
        semantic_kind,
        memory_limit,
    )
    .await
}

/// 专用于 planner 的补全接口：
/// - 要求 LLM 返回 **JSON 数组**，形如 `[{"tool": "...", "args": {...}}, ...]`；
/// - 在此处统一做 JSON 解析与错误包装，避免上层重复解析。
pub async fn tool_planner(
    cfg: &OpenpupLlmConfig,
    system_prompt: &str,
    goal: &str,
) -> Result<Value> {
    let raw = openai_complete(cfg, system_prompt, goal).await?;
    let trimmed = raw.trim();
    let v: Value = serde_json::from_str(trimmed).map_err(|e| {
        anyhow!(
            "planner reply is not valid JSON array: parse error = {}, reply = {}",
            e,
            trimmed
        )
    })?;
    if !v.is_array() {
        return Err(anyhow!(
            "planner reply is not a JSON array: reply = {}",
            trimmed
        ));
    }
    Ok(v)
}
