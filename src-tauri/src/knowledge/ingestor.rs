use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tracing::{debug, error};
use uuid::Uuid;

use crate::memory::system::MemorySystem;

use super::chunker::{chunk_sections, ChunkConfig};
use super::parser::{parse_file, Section};
use super::types::{IngestProgressEvent, IngestRequest, IngestTextRequest};

/// Async document ingestion pipeline.
pub struct Ingestor {
    memory: Arc<MemorySystem>,
}

impl Ingestor {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self { memory }
    }

    /// Ingest a file: parse → chunk → embed → store.
    /// Returns the source_id on success.
    pub async fn ingest(
        &self,
        req: &IngestRequest,
        on_progress: impl Fn(IngestProgressEvent) + Send,
    ) -> Result<String> {
        let source_id = Uuid::new_v4().to_string();
        let path = std::path::Path::new(&req.path);

        // Determine title
        let title = req.title.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

        // Get file size
        let byte_size = std::fs::metadata(path).map(|m| m.len() as i64).ok();

        // Insert source record
        let now = Utc::now().timestamp();
        let tags_json = serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".to_string());
        self.memory
            .insert_knowledge_source(
                &source_id,
                &title,
                "file",
                Some(&req.path),
                None,
                byte_size,
                Some(&tags_json),
                now,
            )
            .await?;

        // Mark as indexing
        self.memory
            .update_knowledge_source_status(&source_id, "indexing", None)
            .await?;

        on_progress(IngestProgressEvent {
            source_id: source_id.clone(),
            source_title: title.clone(),
            status: "indexing".to_string(),
            chunks_done: 0,
            chunks_total: 0,
            error: None,
        });

        // Parse
        let doc = match parse_file(path) {
            Ok(d) => d,
            Err(e) => {
                let msg = format!("Parse error: {e}");
                self.memory
                    .update_knowledge_source_status(&source_id, "failed", Some(&msg))
                    .await?;
                on_progress(IngestProgressEvent {
                    source_id: source_id.clone(),
                    source_title: title,
                    status: "failed".to_string(),
                    chunks_done: 0,
                    chunks_total: 0,
                    error: Some(msg.clone()),
                });
                return Err(anyhow::anyhow!("{msg}"));
            }
        };

        // Update mime_type
        let _ = self
            .memory
            .update_knowledge_source_mime(&source_id, &doc.mime_type)
            .await;

        // Chunk
        let config = ChunkConfig::default();
        let drafts = chunk_sections(&doc.sections, &config);
        let total = drafts.len();

        on_progress(IngestProgressEvent {
            source_id: source_id.clone(),
            source_title: title.clone(),
            status: "indexing".to_string(),
            chunks_done: 0,
            chunks_total: total,
            error: None,
        });

        // Embed and store each chunk
        for (i, draft) in drafts.iter().enumerate() {
            let chunk_id = Uuid::new_v4().to_string();

            // Generate embedding via the LLM API
            let embedding = match self.memory.embed_text(&draft.content).await {
                Ok(emb) => emb,
                Err(e) => {
                    error!("Embedding failed for chunk {i} of {source_id}: {e}");
                    // Skip this chunk but continue
                    continue;
                }
            };

            let emb_json = serde_json::to_string(&embedding).unwrap_or_else(|_| "[]".to_string());

            self.memory
                .insert_knowledge_chunk(
                    &chunk_id,
                    &source_id,
                    &draft.content,
                    i as i64,
                    draft.heading_path.as_deref(),
                    draft.char_start.map(|v| v as i64),
                    draft.char_end.map(|v| v as i64),
                    &emb_json,
                    Utc::now().timestamp(),
                )
                .await?;

            if (i + 1) % 5 == 0 || i + 1 == total {
                on_progress(IngestProgressEvent {
                    source_id: source_id.clone(),
                    source_title: title.clone(),
                    status: "indexing".to_string(),
                    chunks_done: i + 1,
                    chunks_total: total,
                    error: None,
                });
            }

            debug!(
                "[kb/ingest] chunk {}/{} embedded for source {}",
                i + 1,
                total,
                source_id
            );
        }

        // Mark as indexed
        self.memory
            .update_knowledge_source_indexed(&source_id, total as i64)
            .await?;

        on_progress(IngestProgressEvent {
            source_id: source_id.clone(),
            source_title: title,
            status: "indexed".to_string(),
            chunks_done: total,
            chunks_total: total,
            error: None,
        });

        Ok(source_id)
    }

    /// Ingest raw text content (conversation summary, artifact output, etc.).
    /// Returns the source_id on success.
    pub async fn ingest_text(&self, req: &IngestTextRequest) -> Result<String> {
        let source_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let tags_json = serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".to_string());

        self.memory
            .insert_knowledge_source(
                &source_id,
                &req.title,
                &req.source_type,
                None,
                Some("text/plain"),
                Some(req.content.len() as i64),
                Some(&tags_json),
                now,
            )
            .await?;

        self.memory
            .update_knowledge_source_status(&source_id, "indexing", None)
            .await?;

        // Treat content as a single section, then chunk it
        let sections = vec![Section {
            heading_path: None,
            content: req.content.clone(),
            char_start: 0,
            char_end: req.content.len(),
        }];

        let config = ChunkConfig::default();
        let drafts = chunk_sections(&sections, &config);
        let total = drafts.len();

        for (i, draft) in drafts.iter().enumerate() {
            let chunk_id = Uuid::new_v4().to_string();

            let embedding = match self.memory.embed_text(&draft.content).await {
                Ok(emb) => emb,
                Err(e) => {
                    error!("Embedding failed for text chunk {i} of {source_id}: {e}");
                    continue;
                }
            };

            let emb_json = serde_json::to_string(&embedding).unwrap_or_else(|_| "[]".to_string());

            self.memory
                .insert_knowledge_chunk(
                    &chunk_id,
                    &source_id,
                    &draft.content,
                    i as i64,
                    draft.heading_path.as_deref(),
                    draft.char_start.map(|v| v as i64),
                    draft.char_end.map(|v| v as i64),
                    &emb_json,
                    Utc::now().timestamp(),
                )
                .await?;

            debug!(
                "[kb/ingest_text] chunk {}/{} embedded for source {}",
                i + 1,
                total,
                source_id
            );
        }

        self.memory
            .update_knowledge_source_indexed(&source_id, total as i64)
            .await?;

        debug!(
            "[kb/ingest_text] completed: source={} title='{}' chunks={}",
            source_id, req.title, total
        );

        Ok(source_id)
    }
}
