use std::sync::Arc;

use anyhow::Result;

use crate::memory::system::MemorySystem;

use super::types::KbSearchResult;

/// Semantic vector search over the knowledge base.
pub struct KbRetriever {
    memory: Arc<MemorySystem>,
}

impl KbRetriever {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self { memory }
    }

    /// Search the knowledge base using semantic similarity.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<KbSearchResult>> {
        // Generate query embedding
        let query_vec = self.memory.embed_text(query).await?;

        // Search chunks
        self.memory
            .search_knowledge_chunks(&query_vec, limit, tags)
            .await
    }
}
