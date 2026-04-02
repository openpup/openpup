//! Context building for pup conversations.
//!
//! Extracted from alpha.rs to isolate context assembly logic:
//! owner profile caching, per-pup history building, and memory injection.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::llm::client::{LlmClient, LlmMessage};
use crate::memory::injector::{MemoryBudget, MemoryInjector};
use crate::memory::retriever::MemorySearchResult;
use crate::memory::system::MemorySystem;

/// Builds and caches context for pup conversations.
pub struct ContextBuilder {
    pub memory: Arc<MemorySystem>,
    pub llm_client: Arc<LlmClient>,
    pub memory_injector: Arc<MemoryInjector>,
    /// Cached summarised OWNER.md with TTL.
    owner_summary_cache: RwLock<Option<(String, std::time::Instant)>>,
}

/// How many recent turns to keep verbatim per pup.
const VERBATIM_ROWS: i64 = 20;

impl ContextBuilder {
    pub fn new(
        memory: Arc<MemorySystem>,
        llm_client: Arc<LlmClient>,
        memory_injector: Arc<MemoryInjector>,
    ) -> Self {
        Self {
            memory,
            llm_client,
            memory_injector,
            owner_summary_cache: RwLock::new(None),
        }
    }

    /// Get or compute a cached owner summary (TTL: 5 minutes).
    pub async fn get_owner_summary(&self, owner_md: &str) -> String {
        {
            let guard = self.owner_summary_cache.read().await;
            if let Some((ref cached, ref instant)) = *guard {
                if instant.elapsed().as_secs() < 300 {
                    return cached.clone();
                }
            }
        }
        let summary = if owner_md.len() <= 800 {
            owner_md.to_string()
        } else {
            let prompt = format!(
                "Summarize this OWNER.md in under 150 words, keeping: name, key boundaries/forbidden actions, \
                 language preference, work schedule, top pain points.\n\n{owner_md}"
            );
            match self
                .llm_client
                .chat_mini(vec![LlmMessage {
                    role: "user".into(),
                    content: prompt,
                }])
                .await
            {
                Ok(s) => s,
                Err(_) => format!("{}…", owner_md.chars().take(800).collect::<String>()),
            }
        };
        {
            let mut guard = self.owner_summary_cache.write().await;
            *guard = Some((summary.clone(), std::time::Instant::now()));
        }
        summary
    }

    /// Per-pup history: rolling compressed summary + last VERBATIM_ROWS turns.
    pub async fn build_history(&self, pup: &str) -> Vec<LlmMessage> {
        let mut msgs: Vec<LlmMessage> = Vec::new();

        // Prepend rolling compressed summary
        if let Ok(Some((summary, _))) = self.memory.get_context_summary(pup).await {
            msgs.push(LlmMessage {
                role: "system".into(),
                content: format!("## Earlier conversation summary\n{summary}"),
            });
        }

        // Append most recent verbatim turns
        let recent = self
            .memory
            .recent_conversations(pup, VERBATIM_ROWS)
            .await
            .unwrap_or_default();
        msgs.extend(
            recent
                .into_iter()
                .rev()
                .map(|(role, content)| LlmMessage { role, content }),
        );
        msgs
    }

    /// Build memory context using hybrid retrieval (rule force-injection + Weibull decay).
    pub async fn build_memory_context(&self, query: &str) -> (Vec<MemorySearchResult>, String) {
        let memory_context = self
            .memory_injector
            .build_memory_context(query, &MemoryBudget::default())
            .await
            .unwrap_or_default();
        let formatted = MemoryInjector::format_for_injection(&memory_context);
        (memory_context, formatted)
    }

    /// Build the complete relevant_memories vector for injection into a prompt.
    pub fn format_memories_for_prompt(memories_str: &str) -> Vec<String> {
        if memories_str.is_empty() {
            vec![]
        } else {
            vec![memories_str.to_string()]
        }
    }
}
