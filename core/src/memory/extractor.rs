use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use sqlx::{Pool, Row, Sqlite};
use tracing::debug;
use uuid::Uuid;

use crate::llm::client::{LlmClient, LlmMessage};
use crate::memory::system::cosine_similarity;

/// LLM-extracted candidate memory.
#[derive(Debug, Deserialize)]
struct ExtractedMemory {
    content: String,
    memory_type: String,
    importance: f32,
    confidence: f32,
}

pub struct MemoryExtractor {
    pool: Pool<Sqlite>,
    llm: Arc<LlmClient>,
}

impl MemoryExtractor {
    pub fn new(pool: Pool<Sqlite>, llm: Arc<LlmClient>) -> Self {
        Self { pool, llm }
    }

    /// Main entry: extract candidates → conflict detection → execute resolution.
    pub async fn extract_and_resolve(
        &self,
        transcript: &str,
        conversation_id: Option<i64>,
    ) -> Result<()> {
        let candidates = self.extract_candidates(transcript).await?;
        if candidates.is_empty() {
            return Ok(());
        }

        let mut diary_entries: Vec<String> = Vec::new();

        for candidate in candidates {
            // Semantic search for similar memories (threshold 0.75, wider than dedup 0.88)
            let similar = self.find_similar(&candidate.content, 3, 0.75).await?;

            let action_json = if similar.is_empty() {
                r#"{"action":"insert"}"#.to_string()
            } else {
                self.resolve_conflict(&candidate, &similar).await?
            };

            let (action, target_id, new_content) = parse_action(&action_json);

            match action.as_str() {
                "skip" => continue,

                "insert" | "coexist" => {
                    self.insert(&candidate, conversation_id).await?;
                    diary_entries
                        .push(format!("[{}] {}", candidate.memory_type, candidate.content));
                }

                "update" => {
                    if let Some(tid) = target_id {
                        let content = new_content.unwrap_or_else(|| candidate.content.clone());
                        self.update_content(&tid, &content, candidate.confidence)
                            .await?;
                        diary_entries
                            .push(format!("[update:{}] {}", candidate.memory_type, content));
                    }
                }

                "supersede" => {
                    if let Some(tid) = target_id {
                        let new_id = self.insert(&candidate, conversation_id).await?;
                        self.invalidate(&tid, &new_id).await?;
                        diary_entries.push(format!(
                            "[supersede:{}] {}",
                            candidate.memory_type, candidate.content
                        ));
                    }
                }

                _ => {
                    self.insert(&candidate, conversation_id).await?;
                    diary_entries
                        .push(format!("[{}] {}", candidate.memory_type, candidate.content));
                }
            }
        }

        debug!(
            "[memory_extractor] resolved {} diary entries",
            diary_entries.len()
        );
        Ok(())
    }

    /// Returns diary entries for the file layer (caller can append to daily diary).
    pub async fn extract_and_resolve_with_diary(
        &self,
        transcript: &str,
        conversation_id: Option<i64>,
    ) -> Result<Vec<String>> {
        let candidates = self.extract_candidates(transcript).await?;
        if candidates.is_empty() {
            return Ok(vec![]);
        }

        let mut diary_entries: Vec<String> = Vec::new();

        for candidate in candidates {
            let similar = self.find_similar(&candidate.content, 3, 0.75).await?;

            let action_json = if similar.is_empty() {
                r#"{"action":"insert"}"#.to_string()
            } else {
                self.resolve_conflict(&candidate, &similar).await?
            };

            let (action, target_id, new_content) = parse_action(&action_json);

            match action.as_str() {
                "skip" => continue,

                "insert" | "coexist" => {
                    self.insert(&candidate, conversation_id).await?;
                    diary_entries
                        .push(format!("[{}] {}", candidate.memory_type, candidate.content));
                }

                "update" => {
                    if let Some(tid) = target_id {
                        let content = new_content.unwrap_or_else(|| candidate.content.clone());
                        self.update_content(&tid, &content, candidate.confidence)
                            .await?;
                        diary_entries
                            .push(format!("[update:{}] {}", candidate.memory_type, content));
                    }
                }

                "supersede" => {
                    if let Some(tid) = target_id {
                        let new_id = self.insert(&candidate, conversation_id).await?;
                        self.invalidate(&tid, &new_id).await?;
                        diary_entries.push(format!(
                            "[supersede:{}] {}",
                            candidate.memory_type, candidate.content
                        ));
                    }
                }

                _ => {
                    self.insert(&candidate, conversation_id).await?;
                    diary_entries
                        .push(format!("[{}] {}", candidate.memory_type, candidate.content));
                }
            }
        }

        Ok(diary_entries)
    }

    /// LLM extraction of candidate memories (mini model).
    async fn extract_candidates(&self, transcript: &str) -> Result<Vec<ExtractedMemory>> {
        let system = r#"从对话中提取值得长期记忆的信息。
只提取关于"用户这个人"的信息：偏好、规则、事实、经历。
不提取任务内容、代码、文档（那是知识库的职责）。

输出严格的 JSON 数组，不含任何解释：
[
  {
    "content":     "一句话，第三人称（用户偏好…）",
    "memory_type": "fact|preference|rule|experience",
    "importance":  0.0-1.0,
    "confidence":  0.0-1.0
  }
]

规则：
- rule：用户明确说"不要""禁止""必须"等约束
- importance < 0.7 的信息不值得记录，输出空数组
- 对话没有值得记忆的信息时，输出 []"#;

        let prompt = format!("{system}\n\n对话内容：\n{transcript}");

        let raw = self
            .llm
            .chat_mini(vec![LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await?;

        let clean = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        Ok(serde_json::from_str::<Vec<ExtractedMemory>>(clean).unwrap_or_default())
    }

    /// LLM conflict resolution, returns JSON action string.
    async fn resolve_conflict(
        &self,
        new_mem: &ExtractedMemory,
        existing: &[(String, String, f32)], // (id, content, similarity)
    ) -> Result<String> {
        let existing_text = existing
            .iter()
            .enumerate()
            .map(|(i, (id, content, score))| {
                format!(
                    "[{}] id={} 相似度={:.2}\n内容：{}",
                    i + 1,
                    id,
                    score,
                    content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt = format!(
            r#"新提取的记忆：
{}

已有的相似记忆：
{}

判断关系，输出严格 JSON（不含其他文字）：
{{
  "action":      "insert|skip|update|supersede|coexist",
  "target_id":   "操作对象 id（skip/update/supersede 时填写，其他为 null）",
  "new_content": "update 时的新内容（其他为 null）"
}}

- insert：新信息，无冲突
- skip：完全相同或语义等价
- update：措辞需更新，但语义相近
- supersede：新事实取代旧事实（用户状态/偏好已变化）
- coexist：相似但不冲突，可以共存"#,
            new_mem.content, existing_text
        );

        let raw = self
            .llm
            .chat_mini(vec![
                LlmMessage {
                    role: "system".into(),
                    content: "你是记忆冲突分析引擎，只输出 JSON。".into(),
                },
                LlmMessage {
                    role: "user".into(),
                    content: prompt,
                },
            ])
            .await?;

        Ok(raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string())
    }

    async fn find_similar(
        &self,
        content: &str,
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(String, String, f32)>> {
        let q_vec = match self.llm.embed(content).await {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };

        let rows = sqlx::query(
            r#"
            SELECT m.id, m.content, e.embedding
            FROM long_term_memory m
            JOIN memory_embeddings e ON m.id = e.memory_id
            WHERE m.memory_type != 'invalidated'
              AND m.superseded_by IS NULL
            ORDER BY m.importance DESC
            LIMIT 500
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut scored: Vec<(String, String, f32)> = rows
            .into_iter()
            .filter_map(|row| {
                let id: String = row.get("id");
                let content: String = row.get("content");
                let emb_json: String = row.get("embedding");
                let emb: Vec<f32> = serde_json::from_str(&emb_json).ok()?;
                let sim = cosine_similarity(&q_vec, &emb);
                if sim >= threshold {
                    Some((id, content, sim))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    async fn insert(&self, mem: &ExtractedMemory, conversation_id: Option<i64>) -> Result<String> {
        self.insert_with_scope(mem, conversation_id, "global").await
    }

    async fn insert_with_scope(&self, mem: &ExtractedMemory, conversation_id: Option<i64>, role_scope: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO long_term_memory
             (id, content, memory_type, importance, confidence,
              extracted_from, created_at, last_accessed,
              access_count, access_count_total, role_scope)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?)",
        )
        .bind(&id)
        .bind(&mem.content)
        .bind(&mem.memory_type)
        .bind(mem.importance as f64)
        .bind(mem.confidence as f64)
        .bind(conversation_id)
        .bind(now)
        .bind(now)
        .bind(role_scope)
        .execute(&self.pool)
        .await?;

        // Generate and store embedding (best-effort)
        if let Ok(emb) = self.llm.embed(&mem.content).await {
            if let Ok(emb_json) = serde_json::to_string(&emb) {
                let _ = sqlx::query(
                    "INSERT OR REPLACE INTO memory_embeddings (memory_id, embedding) VALUES (?, ?)",
                )
                .bind(&id)
                .bind(emb_json)
                .execute(&self.pool)
                .await;
            }
        }

        Ok(id)
    }

    async fn update_content(&self, id: &str, content: &str, confidence: f32) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE long_term_memory SET content=?, confidence=?, last_accessed=? WHERE id=?",
        )
        .bind(content)
        .bind(confidence as f64)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        // Re-embed
        if let Ok(emb) = self.llm.embed(content).await {
            if let Ok(emb_json) = serde_json::to_string(&emb) {
                let _ = sqlx::query("UPDATE memory_embeddings SET embedding=? WHERE memory_id=?")
                    .bind(emb_json)
                    .bind(id)
                    .execute(&self.pool)
                    .await;
            }
        }

        Ok(())
    }

    async fn invalidate(&self, old_id: &str, new_id: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE long_term_memory
             SET superseded_by=?, valid_until=?, memory_type='invalidated'
             WHERE id=?",
        )
        .bind(new_id)
        .bind(now)
        .bind(old_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn parse_action(json_str: &str) -> (String, Option<String>, Option<String>) {
    let v: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();
    let action = v["action"].as_str().unwrap_or("insert").to_string();
    let target_id = v["target_id"].as_str().map(str::to_string);
    let new_content = v["new_content"].as_str().map(str::to_string);
    (action, target_id, new_content)
}
