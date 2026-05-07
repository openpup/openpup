//! Intent classification and pup routing.
//!
//! Extracted from alpha.rs to separate the routing decision logic from
//! the orchestration and execution concerns.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;

use crate::agents::alpha::PupConfig;
use crate::llm::client::{LlmClient, LlmMessage};
use crate::memory::system::MemorySystem;
use crate::skills::executor::SkillExecutor;

/// Router handles intent classification and @mention extraction.
pub struct Router {
    pub llm_client: Arc<LlmClient>,
    pub pup_configs: Arc<RwLock<HashMap<String, PupConfig>>>,
    pub skill_executor: Arc<SkillExecutor>,
    pub memory: Arc<MemorySystem>,
}

impl Router {
    /// Classify a user message into a routing key.
    ///
    /// Returns one of:
    /// - `"alpha"` — general chat
    /// - `"dev"` / `"writer"` / etc — specialist pup key
    /// - `"skill:<name>"` — activate a skill
    /// - `"channel:pup1,pup2"` — multi-pup parallel dispatch
    pub async fn classify_intent(
        &self,
        msg: &str,
        owner_summary: &str,
        history: &[LlmMessage],
    ) -> String {
        let trimmed = msg.trim();
        if trimmed.len() < 8 {
            debug!("[router] classify_intent: short msg → alpha");
            return "alpha".to_string();
        }

        let enabled_pups = self.enabled_configured_pup_keys().await;
        let skill_entries = self
            .skill_executor
            .registry
            .enabled_skill_names_and_triggers()
            .await;

        let skill_lines: Vec<String> = skill_entries
            .iter()
            .map(|(name, triggers)| {
                if triggers.is_empty() {
                    format!("  - skill:{name}")
                } else {
                    format!("  - skill:{name} → {}", triggers.join(", "))
                }
            })
            .collect();
        let skills_block = if skill_lines.is_empty() {
            String::new()
        } else {
            format!("\nInstalled skills:\n{}", skill_lines.join("\n"))
        };

        let snippet = if owner_summary.chars().count() > 400 {
            owner_summary.chars().take(400).collect::<String>()
        } else {
            owner_summary.to_string()
        };
        let pup_hints: String = {
            let cfgs = self.pup_configs.read().await;
            enabled_pups
                .iter()
                .filter_map(|k| cfgs.get(k))
                .map(|c| format!("  - {} → {}", c.key, c.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let channel_hint = if enabled_pups.len() >= 2 {
            format!(
        "\n- channel:<pup1>,<pup2> → 任务同时需要多个专业 pup 并行协作完成（输出一个 token，例如 channel:research,writer）\
         \n  适用场景举例：\
         \n    · 「调研 XX 并写成报告」→ channel:research,writer\
         \n    · 「分析财报数据并写摘要」→ channel:finance,writer\
         \n    · 「写一个爬虫脚本并附使用文档」→ channel:dev,writer\
         \n  pup 列表（只能用已有 key）：{}\
         \n  注意：channel 本身是一个 token，不含空格。",
        enabled_pups.join(", ")
      )
        } else {
            String::new()
        };

        let system_prompt = format!(
            "Owner profile (excerpt):\n{snippet}\n\n\
       你是任务路由器。根据用户消息，输出以下选项之一（单个 token，无多余内容）：\
       \n- alpha → 闲聊、问答、或其他\
       \n{pup_hints}\n\
       {skills_block}{channel_hint}\n\
       直接输出 token，不要解释。"
        );

        let mut classifier_msgs = vec![LlmMessage {
            role: "system".into(),
            content: system_prompt,
            name: None,
        }];
        if let Some(last) = history.last() {
            classifier_msgs.push(last.clone());
        }
        classifier_msgs.push(LlmMessage {
            role: "user".into(),
            content: format!("Message to classify: \"{msg}\""),
            name: None,
        });

        let raw = match self.llm_client.chat_mini(classifier_msgs).await {
            Ok(r) => r,
            Err(e) => {
                debug!("[router] classify_intent chat_mini error: {e}");
                return "alpha".to_string();
            }
        };
        let key = raw.trim().to_lowercase();
        let key = key.split_whitespace().next().unwrap_or("alpha");
        debug!("[router] classify_intent: raw={raw:?} → key={key:?}");

        if key == "alpha" || enabled_pups.iter().any(|p| p == key) {
            return key.to_string();
        }
        if let Some(skill_name) = key.strip_prefix("skill:") {
            if skill_entries.iter().any(|(n, _)| n == skill_name) {
                return key.to_string();
            }
        }
        if let Some(pups_str) = key.strip_prefix("channel:") {
            let valid_pups: Vec<String> = pups_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|p| enabled_pups.contains(p))
                .collect();
            if valid_pups.len() >= 2 {
                let canonical = format!("channel:{}", valid_pups.join(","));
                debug!("[router] classify_intent: multi-pup channel → {canonical}");
                return canonical;
            }
        }
        debug!("[router] classify_intent: unrecognised key {key:?} → alpha");
        "alpha".to_string()
    }

    /// Extract an @mention from the start of a message.
    pub async fn extract_at_mention(&self, msg: &str) -> Option<String> {
        let trimmed = msg.trim_start();
        if !trimmed.starts_with('@') {
            return None;
        }
        let rest = &trimmed[1..];
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let candidate = rest[..end].to_lowercase();
        let cfgs = self.pup_configs.read().await;
        if cfgs.get(&candidate).map(|c| c.enabled).unwrap_or(false) {
            Some(candidate)
        } else {
            None
        }
    }

    /// Brief global history (last 4 turns across all pups) — used for intent classification.
    pub async fn build_classify_history(&self) -> Vec<LlmMessage> {
        let recent = self
            .memory
            .recent_conversations_global(4)
            .await
            .unwrap_or_default();
        recent
            .into_iter()
            .rev()
            .map(|(role, content)| LlmMessage {
                role,
                content,
                name: None,
            })
            .collect()
    }

    async fn enabled_configured_pup_keys(&self) -> Vec<String> {
        let guard = self.pup_configs.read().await;
        let mut keys: Vec<String> = guard
            .values()
            .filter(|cfg| cfg.enabled)
            .map(|cfg| cfg.key.clone())
            .collect();
        keys.sort();
        keys
    }
}
