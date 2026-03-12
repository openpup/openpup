//! LLM 客户端：从 config.toml + env 加载统一配置，通过 OpenAI 兼容 /chat/completions 调用。
//! 提供 chat_once、chat_with_memory、tool_planner 等高层 API，供 Agent / Planner 使用。
//!
//! ## 内建 LLM 角色（LLM 增强）
//! 提供若干**内建角色**（固定 system prompt），用于安全审查、摘要、校验等增强能力。
//! 通过 `complete_as_builtin_role(cfg, role_id, user_message)` 调用，使用低 temperature 以保证稳定输出。

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
                        // `String::truncate` expects a UTF-8 char boundary; max_len is bytes.
                        // Walk backwards to the nearest boundary to avoid panic on multi-byte chars.
                        let mut new_len = max_len;
                        while new_len > 0 && !content.is_char_boundary(new_len) {
                            new_len -= 1;
                        }
                        content.truncate(new_len);
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

// ============== 内建 LLM 角色（用于安全审查、摘要等增强） ==============

/// 内建角色 ID，用于 `complete_as_builtin_role`。
pub const ROLE_SECURITY_REVIEWER: &str = "security_reviewer";

fn builtin_role_system_prompt(role_id: &str) -> Option<&'static str> {
    match role_id {
        "security_reviewer" => Some(
            r#"You are a security reviewer for shell commands. Given one shell command string, you must respond with exactly one line: L2, L3, or L4.

- L4: High risk — e.g. delete root (rm -rf /), sudo, write to /etc or system paths, pipe to sh/bash, mkfs, dd to device, chmod 4755, fork bomb.
- L3: Medium risk — e.g. curl/wget (network), nohup, background &, write to /tmp /var /usr.
- L2: Low risk — read-only or only writes under workspace; simple commands.

Reply with only the level (L2, L3, or L4), optionally followed by a single space and a very short reason. Example: "L2" or "L4 dangerous: rm -rf /"."#,
        ),
        _ => None,
    }
}

/// 使用内建角色的 system prompt 做一次补全（低 temperature，便于解析）。
/// 用于安全审查官、摘要等「LLM 增强」场景。
pub async fn complete_as_builtin_role(
    cfg: &OpenpupLlmConfig,
    role_id: &str,
    user_message: &str,
) -> Result<String> {
    let system = builtin_role_system_prompt(role_id)
        .ok_or_else(|| anyhow!("unknown builtin role: {}", role_id))?;
    let mut low_temp_cfg = cfg.clone();
    low_temp_cfg.temperature = 0.1;
    openai_complete(&low_temp_cfg, system, user_message).await
}

/// 从 security_reviewer 角色回复中解析出等级 L2/L3/L4（取第一个出现的等级标记）。
pub fn parse_security_level_from_review(reply: &str) -> Option<String> {
    let reply = reply.trim().to_uppercase();
    if reply.starts_with("L2") {
        Some("L2".to_string())
    } else if reply.starts_with("L3") {
        Some("L3".to_string())
    } else if reply.starts_with("L4") {
        Some("L4".to_string())
    } else {
        None
    }
}

/// 同步封装：仅在**当前线程没有** tokio runtime 时新建 runtime 并调用 `complete_as_builtin_role`。
/// 若当前已在 runtime 内（如 agent REPL 的 worker 线程），直接返回 Err，避免在 async 上下文中创建/销毁 runtime 导致 panic；调用方（tools）会回退到规则审查。
pub fn complete_as_builtin_role_blocking(
    cfg: &OpenpupLlmConfig,
    role_id: &str,
    user_message: &str,
) -> Result<String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(anyhow!(
            "LLM builtin role cannot run blocking here (already inside tokio runtime); use rule-based review"
        ));
    }
    let cfg = cfg.clone();
    let role_id = role_id.to_string();
    let user_message = user_message.to_string();
    let rt = tokio::runtime::Runtime::new().map_err(|e| anyhow!("runtime: {}", e))?;
    rt.block_on(async move {
        complete_as_builtin_role(&cfg, &role_id, &user_message).await
    })
}

// ============== 以上为内建 LLM 角色 ==============

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
