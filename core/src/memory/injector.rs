use std::sync::Arc;

use anyhow::Result;
use sqlx::{Pool, Row, Sqlite};

use crate::memory::retriever::{MemoryRetriever, MemorySearchResult};

pub struct MemoryBudget {
    pub rule_slots: usize,
    pub semantic_slots: usize,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self {
            rule_slots: 10,
            semantic_slots: 5,
        }
    }
}

pub struct MemoryInjector {
    pool: Pool<Sqlite>,
    retriever: Arc<MemoryRetriever>,
}

impl MemoryInjector {
    pub fn new(pool: Pool<Sqlite>, retriever: Arc<MemoryRetriever>) -> Self {
        Self { pool, retriever }
    }

    /// Build memory context for LLM injection.
    ///
    /// v2: supports role-scoped memory — injects global + current role's memories.
    ///
    /// Order:
    ///   1. All active rules (forced, from active_rules view, bypass retrieval)
    ///   2. Semantic + Weibull-weighted Top-K (excluding rules to avoid duplicates)
    pub async fn build_memory_context(
        &self,
        query: &str,
        budget: &MemoryBudget,
    ) -> Result<Vec<MemorySearchResult>> {
        self.build_memory_context_for_role(query, budget, None).await
    }

    /// Build memory context with optional role scope filtering.
    /// When `role` is Some, includes both global and role-specific memories.
    pub async fn build_memory_context_for_role(
        &self,
        query: &str,
        budget: &MemoryBudget,
        role: Option<&str>,
    ) -> Result<Vec<MemorySearchResult>> {
        // Force-inject active rules (rules are always global)
        let rules = sqlx::query(
            "SELECT id, content, importance, confidence
             FROM active_rules
             LIMIT ?",
        )
        .bind(budget.rule_slots as i64)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut context: Vec<MemorySearchResult> = rules
            .iter()
            .map(|row| {
                let id: String = row.get("id");
                let content: String = row.get("content");
                let importance: f64 = row.get("importance");
                let confidence: f64 = row.get("confidence");
                MemorySearchResult {
                    id,
                    content,
                    memory_type: "rule".to_string(),
                    confidence: confidence as f32,
                    score: 2.0 + importance as f32, // ensure rules sort first
                    is_forced: true,
                }
            })
            .collect();

        let rule_ids: std::collections::HashSet<String> =
            context.iter().map(|m| m.id.clone()).collect();

        // Semantic retrieval (includes Weibull weighting)
        // v2: filter by role scope when specified
        let semantic = self
            .retriever
            .search_with_role(query, budget.semantic_slots + rule_ids.len(), role)
            .await
            .unwrap_or_default();

        for mem in semantic {
            if rule_ids.contains(&mem.id) {
                continue;
            }
            if mem.memory_type == "rule" {
                continue;
            }
            context.push(mem);
            if context.len() >= budget.rule_slots + budget.semantic_slots {
                break;
            }
        }

        Ok(context)
    }

    /// Format memories for prompt injection.
    pub fn format_for_injection(memories: &[MemorySearchResult]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let rules: Vec<_> = memories.iter().filter(|m| m.is_forced).collect();
        let others: Vec<_> = memories.iter().filter(|m| !m.is_forced).collect();

        let mut parts = Vec::new();

        if !rules.is_empty() {
            let lines = rules
                .iter()
                .map(|m| format!("- {}", m.content))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("## 用户规则（必须遵守）\n{lines}"));
        }

        if !others.is_empty() {
            let lines = others
                .iter()
                .map(|m| {
                    let label = match m.memory_type.as_str() {
                        "preference" => "偏好",
                        "fact" => "事实",
                        "experience" => "经历",
                        _ => "记忆",
                    };
                    format!("- [{}] {}", label, m.content)
                })
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("## 关于用户的记忆\n{lines}"));
        }

        parts.join("\n\n")
    }
}
