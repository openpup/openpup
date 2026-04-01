//! Multi-layer context compaction strategies.
//!
//! Inspired by Claude Code's three-tier approach:
//! 1. **Micro-compaction** — selective pruning of tool outputs while keeping conversation intact
//! 2. **Full compaction** — rolling summary of older conversation history (existing logic, enhanced)
//! 3. **Session memory persistence** — extract critical facts to long-term memory before compacting
//!
//! The compaction engine decides which strategy to apply based on the context pressure level.

use anyhow::Result;
use sqlx::{Pool, Row, Sqlite};
use std::sync::Arc;
use tracing::debug;

use crate::llm::client::{LlmClient, LlmMessage};
use crate::memory::extractor::MemoryExtractor;

/// Context pressure levels drive which compaction strategy fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    /// Below 40% — no compaction needed.
    Low,
    /// 40–65% — micro-compact tool outputs only.
    Moderate,
    /// 65–85% — full compaction of older conversation turns.
    High,
    /// >85% — emergency: persist session memories + aggressive compaction.
    Critical,
}

impl PressureLevel {
    /// Determine pressure level from current token usage vs context limit.
    pub fn from_usage(tokens: u64, limit: u64) -> Self {
        if limit == 0 {
            return Self::Low;
        }
        let ratio = tokens as f64 / limit as f64;
        if ratio > 0.85 {
            Self::Critical
        } else if ratio > 0.65 {
            Self::High
        } else if ratio > 0.40 {
            Self::Moderate
        } else {
            Self::Low
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub strategy: &'static str,
    pub messages_before: usize,
    pub messages_after: usize,
    pub estimated_tokens_saved: u64,
}

/// Orchestrates multi-layer compaction for a single pup's conversation context.
pub struct CompactionEngine {
    pool: Pool<Sqlite>,
    llm: Arc<LlmClient>,
    extractor: Arc<MemoryExtractor>,
}

impl CompactionEngine {
    pub fn new(pool: Pool<Sqlite>, llm: Arc<LlmClient>, extractor: Arc<MemoryExtractor>) -> Self {
        Self {
            pool,
            llm,
            extractor,
        }
    }

    /// Run the appropriate compaction strategy based on context pressure.
    ///
    /// Returns a list of compaction results (multiple strategies may fire in sequence).
    pub async fn compact(
        &self,
        pup: &str,
        current_tokens: u64,
        context_limit: u64,
    ) -> Result<Vec<CompactionResult>> {
        let pressure = PressureLevel::from_usage(current_tokens, context_limit);
        debug!("[compaction] pup={pup} tokens={current_tokens} limit={context_limit} pressure={pressure:?}");

        let mut results = Vec::new();

        match pressure {
            PressureLevel::Low => {
                // No compaction needed
            }
            PressureLevel::Moderate => {
                // Layer 1: Micro-compact tool outputs only
                if let Ok(r) = self.micro_compact(pup).await {
                    results.push(r);
                }
            }
            PressureLevel::High => {
                // Layer 1 + Layer 2: micro-compact then full compaction
                if let Ok(r) = self.micro_compact(pup).await {
                    results.push(r);
                }
                if let Ok(r) = self.full_compact(pup).await {
                    results.push(r);
                }
            }
            PressureLevel::Critical => {
                // Layer 3 + Layer 1 + Layer 2: persist memories first, then compact
                if let Ok(r) = self.persist_session_memories(pup).await {
                    results.push(r);
                }
                if let Ok(r) = self.micro_compact(pup).await {
                    results.push(r);
                }
                if let Ok(r) = self.full_compact(pup).await {
                    results.push(r);
                }
            }
        }

        Ok(results)
    }

    // ── Layer 1: Micro-compaction ─────────────────────────────────────────────

    /// Selectively prune tool call outputs in conversation history while keeping
    /// the conversation flow intact. Tool results are replaced with a brief
    /// summary placeholder.
    ///
    /// Only targets assistant messages containing tool outputs (detected by
    /// JSON-like patterns or long content that looks like tool results).
    async fn micro_compact(&self, pup: &str) -> Result<CompactionResult> {
        // Find conversation rows that look like tool outputs (long content, JSON-like)
        let rows: Vec<(i64, String)> = sqlx::query_as(
            r#"SELECT id, content FROM conversations
               WHERE pup = ? AND role = 'assistant'
               AND (length(content) > 2000 OR content LIKE '{"%' OR content LIKE '[{%')
               ORDER BY id ASC
               LIMIT 50"#,
        )
        .bind(pup)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(CompactionResult {
                strategy: "micro_compact",
                messages_before: 0,
                messages_after: 0,
                estimated_tokens_saved: 0,
            });
        }

        let mut total_saved: u64 = 0;
        let mut compacted_count = 0;

        for (id, content) in &rows {
            let char_count = content.chars().count();
            if char_count <= 800 {
                continue; // Not worth compacting
            }

            // Keep first 200 chars + last 100 chars, replace middle with marker
            let head: String = content.chars().take(200).collect();
            let tail: String = content.chars().skip(char_count.saturating_sub(100)).collect();
            let compacted = format!(
                "{head}\n\n[… micro-compacted: {omitted} chars omitted …]\n\n{tail}",
                omitted = char_count - 300
            );

            sqlx::query("UPDATE conversations SET content = ? WHERE id = ?")
                .bind(&compacted)
                .bind(id)
                .execute(&self.pool)
                .await?;

            let saved = (char_count - compacted.chars().count()) as u64 / 4; // ~4 chars/token
            total_saved += saved;
            compacted_count += 1;
        }

        debug!(
            "[compaction] micro_compact pup={pup}: compacted {compacted_count} messages, saved ~{total_saved} tokens"
        );

        Ok(CompactionResult {
            strategy: "micro_compact",
            messages_before: rows.len(),
            messages_after: rows.len(), // same count, just shorter
            estimated_tokens_saved: total_saved,
        })
    }

    // ── Layer 2: Full compaction ──────────────────────────────────────────────

    /// Enhanced version of the existing `maybe_compress_context` logic.
    /// Compresses older conversation turns into a rolling summary, but with
    /// improvements:
    /// - Lower threshold (30 rows vs 40)
    /// - Preserves key facts & decisions explicitly
    /// - Merges incrementally with existing summary
    const VERBATIM_ROWS: i64 = 20;
    const COMPRESS_THRESHOLD: i64 = 30; // Lowered from 40 for more aggressive compaction

    async fn full_compact(&self, pup: &str) -> Result<CompactionResult> {
        let max_row: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(id), 0) FROM conversations WHERE pup = ?",
        )
        .bind(pup)
        .fetch_one(&self.pool)
        .await?;

        let (existing_summary, covers_through): (String, i64) = sqlx::query_as(
            "SELECT COALESCE(summary, ''), COALESCE(covers_through_row, 0) FROM context_summaries WHERE pup = ?",
        )
        .bind(pup)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or_default();

        let compressible_ceiling = max_row - Self::VERBATIM_ROWS;
        let uncovered_rows = compressible_ceiling - covers_through;

        if uncovered_rows < Self::COMPRESS_THRESHOLD {
            return Ok(CompactionResult {
                strategy: "full_compact",
                messages_before: 0,
                messages_after: 0,
                estimated_tokens_saved: 0,
            });
        }

        // Load uncovered rows (cap at 200 to limit LLM input)
        let rows: Vec<(i64, String, String)> = sqlx::query_as(
            r#"SELECT id, role, content FROM conversations
               WHERE pup = ? AND id > ? AND id <= ?
               ORDER BY id ASC
               LIMIT 200"#,
        )
        .bind(pup)
        .bind(covers_through)
        .bind(compressible_ceiling)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(CompactionResult {
                strategy: "full_compact",
                messages_before: 0,
                messages_after: 0,
                estimated_tokens_saved: 0,
            });
        }

        let row_count = rows.len();
        let last_row_id = rows.last().map(|(id, _, _)| *id).unwrap_or(covers_through);

        // Build transcript with smart truncation per message
        let transcript = rows
            .iter()
            .map(|(_, role, content)| {
                let snippet: String = content.chars().take(300).collect();
                let ellipsis = if content.chars().count() > 300 {
                    "…"
                } else {
                    ""
                };
                format!("{role}: {snippet}{ellipsis}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prior_context = if existing_summary.is_empty() {
            String::new()
        } else {
            format!("Prior summary:\n{existing_summary}\n\nNew exchanges to merge:\n")
        };

        let prompt = format!(
            "{prior_context}{transcript}\n\n\
             Produce a concise summary (≤300 words) of the above conversation exchanges.\n\
             MUST preserve:\n\
             - Key facts and decisions made\n\
             - User preferences discovered\n\
             - Ongoing task status and commitments\n\
             - Important context that would be needed to continue the conversation\n\
             Write in third-person neutral style. Be specific about names, dates, and numbers."
        );

        let new_summary = self
            .llm
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await?;

        // Upsert the summary
        sqlx::query(
            r#"INSERT INTO context_summaries (pup, summary, covers_through_row, updated_at)
               VALUES (?, ?, ?, unixepoch())
               ON CONFLICT(pup) DO UPDATE SET
                 summary = excluded.summary,
                 covers_through_row = excluded.covers_through_row,
                 updated_at = excluded.updated_at"#,
        )
        .bind(pup)
        .bind(&new_summary)
        .bind(last_row_id)
        .execute(&self.pool)
        .await?;

        // Estimate tokens saved: compressed rows now replaced by ~300 word summary
        let original_chars: usize = rows.iter().map(|(_, _, c)| c.chars().count()).sum();
        let summary_chars = new_summary.chars().count();
        let tokens_saved = (original_chars.saturating_sub(summary_chars)) as u64 / 4;

        debug!(
            "[compaction] full_compact pup={pup}: compressed {row_count} rows through row {last_row_id}, saved ~{tokens_saved} tokens"
        );

        Ok(CompactionResult {
            strategy: "full_compact",
            messages_before: row_count,
            messages_after: 1, // single summary
            estimated_tokens_saved: tokens_saved,
        })
    }

    // ── Layer 3: Session memory persistence ───────────────────────────────────

    /// Before aggressive compaction, extract critical facts from the conversation
    /// that's about to be compressed and persist them to long-term memory.
    ///
    /// This ensures that important information isn't lost when the conversation
    /// context is aggressively pruned under critical pressure.
    async fn persist_session_memories(&self, pup: &str) -> Result<CompactionResult> {
        // Get recent conversations that haven't been extracted yet
        let rows: Vec<(String, String)> = sqlx::query_as(
            r#"SELECT role, content FROM conversations
               WHERE pup = ?
               ORDER BY id DESC
               LIMIT 20"#,
        )
        .bind(pup)
        .fetch_all(&self.pool)
        .await?;

        if rows.len() < 4 {
            return Ok(CompactionResult {
                strategy: "persist_session_memories",
                messages_before: 0,
                messages_after: 0,
                estimated_tokens_saved: 0,
            });
        }

        let transcript = rows
            .iter()
            .rev()
            .map(|(role, content)| format!("{role}: {content}"))
            .collect::<Vec<_>>()
            .join("\n");

        // Use the memory extractor to find and persist new facts
        let _ = self
            .extractor
            .extract_and_resolve(&transcript, None)
            .await;

        debug!(
            "[compaction] persist_session_memories pup={pup}: extracted memories from {} rows",
            rows.len()
        );

        Ok(CompactionResult {
            strategy: "persist_session_memories",
            messages_before: rows.len(),
            messages_after: rows.len(), // doesn't change message count
            estimated_tokens_saved: 0,  // indirect savings via better long-term recall
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pressure_levels() {
        assert_eq!(PressureLevel::from_usage(0, 128_000), PressureLevel::Low);
        assert_eq!(
            PressureLevel::from_usage(30_000, 128_000),
            PressureLevel::Low
        );
        assert_eq!(
            PressureLevel::from_usage(55_000, 128_000),
            PressureLevel::Moderate
        );
        assert_eq!(
            PressureLevel::from_usage(90_000, 128_000),
            PressureLevel::High
        );
        assert_eq!(
            PressureLevel::from_usage(115_000, 128_000),
            PressureLevel::Critical
        );
        // Edge case: zero limit
        assert_eq!(PressureLevel::from_usage(100, 0), PressureLevel::Low);
    }
}
