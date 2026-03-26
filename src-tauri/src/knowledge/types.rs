use serde::{Deserialize, Serialize};

/// A registered knowledge source (file, URL, manual entry, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSource {
    pub id: String,
    pub title: String,
    pub source_type: String, // "file" | "url" | "artifact" | "manual"
    pub source_path: Option<String>,
    pub mime_type: Option<String>,
    pub byte_size: Option<i64>,
    pub tags: Option<String>, // JSON array
    pub chunk_count: i64,
    pub status: String, // "pending" | "indexing" | "indexed" | "failed"
    pub error_msg: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A single chunk of text from a knowledge source, with its embedding.
#[derive(Debug, Clone)]
pub struct KnowledgeChunk {
    pub id: String,
    pub source_id: String,
    pub content: String,
    pub chunk_index: i64,
    pub heading_path: Option<String>,
    pub char_start: Option<i64>,
    pub char_end: Option<i64>,
    pub created_at: i64,
}

/// A search result returned to the frontend or to pup tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbSearchResult {
    pub chunk_id: String,
    pub source_id: String,
    pub source_title: String,
    pub source_path: Option<String>,
    pub content: String,
    pub heading_path: Option<String>,
    pub score: f32,
    pub chunk_index: i64,
}

/// Request to ingest a file into the knowledge base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRequest {
    pub path: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
}

/// Request to ingest raw text content (conversation summaries, artifacts, etc.).
#[derive(Debug, Clone)]
pub struct IngestTextRequest {
    pub title: String,
    pub content: String,
    pub source_type: String, // "conversation" | "artifact"
    pub tags: Vec<String>,
}

/// Progress event emitted during ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestProgressEvent {
    pub source_id: String,
    pub source_title: String,
    pub status: String,
    pub chunks_done: usize,
    pub chunks_total: usize,
    pub error: Option<String>,
}
