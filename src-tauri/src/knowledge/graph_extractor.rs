//! Async entity/relation extraction from knowledge chunks using the mini LLM.
//! Runs as a background task after ingestion — failures are silent and don't
//! affect the main KB index.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::knowledge::types::KnowledgeChunk;
use crate::llm::client::{LlmClient, LlmMessage};
use crate::memory::system::MemorySystem;

#[derive(Debug, Deserialize)]
struct ExtractionResult {
    #[serde(default)]
    entities: Vec<EntityDef>,
    #[serde(default)]
    relations: Vec<RelationDef>,
}

#[derive(Debug, Deserialize)]
struct EntityDef {
    name: String,
    entity_type: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RelationDef {
    from: String,
    to: String,
    relation: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_confidence() -> f32 {
    0.8
}

const VALID_ENTITY_TYPES: &[&str] = &[
    "person", "project", "concept", "tool", "org", "file", "other",
];
const VALID_RELATIONS: &[&str] = &[
    "depends_on", "created_by", "part_of", "uses", "related_to",
];

pub struct GraphExtractor {
    memory: Arc<MemorySystem>,
    llm: Arc<LlmClient>,
}

impl GraphExtractor {
    pub fn new(memory: Arc<MemorySystem>, llm: Arc<LlmClient>) -> Self {
        Self { memory, llm }
    }

    /// Extract entities and relations from all chunks of a source.
    /// Processes in batches of 10 chunks. Single batch failure does not abort.
    pub async fn extract_from_source(
        &self,
        source_id: &str,
        chunks: &[KnowledgeChunk],
    ) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        debug!(
            "[kg] extracting from source {} ({} chunks)",
            source_id,
            chunks.len()
        );

        for batch in chunks.chunks(10) {
            let combined: String = batch
                .iter()
                .map(|c| c.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");

            match self.extract_batch(&combined).await {
                Ok(result) => {
                    if let Err(e) =
                        self.upsert_entities_and_relations(source_id, batch, &result).await
                    {
                        warn!("[kg] upsert failed for source {source_id}: {e}");
                    }
                }
                Err(e) => {
                    warn!("[kg] extraction batch failed for source {source_id}: {e}");
                    // Continue with next batch
                }
            }
        }

        debug!("[kg] extraction complete for source {source_id}");
        Ok(())
    }

    async fn extract_batch(&self, text: &str) -> Result<ExtractionResult> {
        let system = r#"你是知识图谱抽取引擎。从文本中识别实体和关系。
输出严格的 JSON，不含任何解释：
{
  "entities": [
    {"name": "实体名", "entity_type": "concept|tool|project|person|org|file|other",
     "description": "一句话描述（可选）"}
  ],
  "relations": [
    {"from": "实体A", "to": "实体B",
     "relation": "depends_on|created_by|part_of|uses|related_to",
     "confidence": 0.9}
  ]
}
只抽取文本中明确提到的关系，不推断，不杜撰。
实体名用规范化形式（如 "Rust" 不是 "rust"，"SQLite" 不是 "sqlite"）。"#;

        let raw = self
            .llm
            .chat_mini(vec![
                LlmMessage {
                    role: "system".into(),
                    content: system.into(),
                },
                LlmMessage {
                    role: "user".into(),
                    content: text.chars().take(6000).collect(),
                },
            ])
            .await?;

        let clean = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let mut result: ExtractionResult = serde_json::from_str(clean)?;

        // Validate entity types and relations
        result.entities.retain(|e| {
            VALID_ENTITY_TYPES.contains(&e.entity_type.as_str())
        });
        result.relations.retain(|r| {
            VALID_RELATIONS.contains(&r.relation.as_str())
        });

        Ok(result)
    }

    async fn upsert_entities_and_relations(
        &self,
        source_id: &str,
        chunks: &[KnowledgeChunk],
        result: &ExtractionResult,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let mut name_to_id: HashMap<String, String> = HashMap::new();

        // 1. Upsert entities
        for e in &result.entities {
            let id = Uuid::new_v4().to_string();
            let actual_id = self
                .memory
                .upsert_kg_entity(
                    &id,
                    &e.name,
                    &e.entity_type,
                    e.description.as_deref(),
                    source_id,
                    now,
                )
                .await?;
            name_to_id.insert(e.name.clone(), actual_id);
        }

        // 2. Link chunks to entities (if entity name appears in chunk content)
        for chunk in chunks {
            for e in &result.entities {
                if chunk.content.contains(&e.name) {
                    if let Some(eid) = name_to_id.get(&e.name) {
                        let _ = self.memory.link_chunk_entity(&chunk.id, eid).await;
                    }
                }
            }
        }

        // 3. Insert relations
        for r in &result.relations {
            if let (Some(fid), Some(tid)) = (name_to_id.get(&r.from), name_to_id.get(&r.to)) {
                let _ = self
                    .memory
                    .insert_kg_relation(
                        &Uuid::new_v4().to_string(),
                        fid,
                        tid,
                        &r.relation,
                        source_id,
                        r.confidence,
                        now,
                    )
                    .await;
            }
        }

        debug!(
            "[kg] upserted {} entities, {} relations for source {}",
            result.entities.len(),
            result.relations.len(),
            source_id
        );

        Ok(())
    }
}
