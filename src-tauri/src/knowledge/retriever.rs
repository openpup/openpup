use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tracing::debug;

use crate::memory::system::MemorySystem;

use super::types::KbSearchResult;

/// Hybrid knowledge base retriever: vector search + FTS5 + RRF fusion.
pub struct KbRetriever {
    memory: Arc<MemorySystem>,
}

/// RRF constant — controls how much rank matters. Standard value is 60.
const RRF_K: f32 = 60.0;

impl KbRetriever {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self { memory }
    }

    /// Hybrid search: runs both vector and FTS5 searches, then merges via
    /// Reciprocal Rank Fusion (RRF).
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<KbSearchResult>> {
        // Fetch more candidates from each source to improve fusion quality
        let fetch_limit = (limit * 3).max(15);

        // Run vector + FTS in parallel
        let (vec_results, fts_results) = tokio::join!(
            self.vector_search(query, fetch_limit, tags),
            self.fts_search(query, fetch_limit, tags),
        );

        let vec_results = vec_results.unwrap_or_default();
        let fts_results = fts_results.unwrap_or_default();

        debug!(
            "[kb/retriever] hybrid search: vec={} fts={} results",
            vec_results.len(),
            fts_results.len()
        );

        // If one source is empty, just return the other
        if fts_results.is_empty() {
            return Ok(vec_results.into_iter().take(limit).collect());
        }
        if vec_results.is_empty() {
            return Ok(fts_results.into_iter().take(limit).collect());
        }

        // Reciprocal Rank Fusion
        Ok(rrf_merge(vec_results, fts_results, limit))
    }

    /// Pure vector/semantic search.
    async fn vector_search(
        &self,
        query: &str,
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<KbSearchResult>> {
        let query_vec = self.memory.embed_text(query).await?;
        self.memory
            .search_knowledge_chunks(&query_vec, limit, tags)
            .await
    }

    /// Pure FTS5 keyword search.
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<KbSearchResult>> {
        self.memory
            .fts_search_knowledge_chunks(query, limit, tags)
            .await
    }
}

/// Merge two ranked result lists using Reciprocal Rank Fusion.
/// Score for each document = sum( 1/(k + rank_i) ) across all lists where it appears.
fn rrf_merge(
    vec_results: Vec<KbSearchResult>,
    fts_results: Vec<KbSearchResult>,
    limit: usize,
) -> Vec<KbSearchResult> {
    // chunk_id → (rrf_score, best KbSearchResult)
    let mut scores: HashMap<String, (f32, KbSearchResult)> = HashMap::new();

    for (rank, r) in vec_results.into_iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f32 + 1.0);
        scores
            .entry(r.chunk_id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, r));
    }

    for (rank, r) in fts_results.into_iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f32 + 1.0);
        scores
            .entry(r.chunk_id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, r));
    }

    let mut merged: Vec<(f32, KbSearchResult)> = scores.into_values().collect();
    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    merged
        .into_iter()
        .take(limit)
        .map(|(score, mut r)| {
            r.score = score;
            r
        })
        .collect()
}
