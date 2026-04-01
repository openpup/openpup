//! Graph-based retrieval: BFS traversal from a named entity, returning
//! related chunks via the kg_chunk_entities mapping.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Result;
use tracing::debug;

use crate::knowledge::types::KbSearchResult;
use crate::memory::system::MemorySystem;

pub struct GraphRetriever {
    memory: Arc<MemorySystem>,
}

impl GraphRetriever {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self { memory }
    }

    /// BFS graph search starting from entities matching `entity_name`.
    /// Returns chunks linked to discovered entities within `max_hops`.
    pub async fn search(&self, entity_name: &str, max_hops: usize) -> Result<Vec<KbSearchResult>> {
        // 1. Find starting entities by fuzzy name match
        let start_entities = self.memory.find_kg_entities(entity_name).await?;
        if start_entities.is_empty() {
            return Ok(vec![]);
        }

        debug!(
            "[kg/retriever] found {} start entities for '{}'",
            start_entities.len(),
            entity_name
        );

        // 2. BFS traversal
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut related_ids: Vec<String> = Vec::new();

        for (id, _, _, _) in &start_entities {
            queue.push_back((id.clone(), 0));
            visited.insert(id.clone());
        }

        while let Some((entity_id, hop)) = queue.pop_front() {
            related_ids.push(entity_id.clone());
            if hop >= max_hops {
                continue;
            }

            let neighbors = self.memory.kg_neighbors(&entity_id).await?;
            for n in neighbors {
                if !visited.contains(&n) {
                    visited.insert(n.clone());
                    queue.push_back((n, hop + 1));
                }
            }
        }

        // 3. Collect chunks from related entities (deduplicated)
        let mut seen_chunks: HashSet<String> = HashSet::new();
        let mut results: Vec<KbSearchResult> = Vec::new();

        for entity_id in &related_ids {
            let chunks = self.memory.kg_entity_chunks(entity_id, 3).await?;
            for r in chunks {
                if seen_chunks.insert(r.chunk_id.clone()) {
                    results.push(r);
                }
            }
        }

        debug!(
            "[kg/retriever] graph search: {} entities, {} chunks",
            related_ids.len(),
            results.len()
        );

        Ok(results)
    }
}
