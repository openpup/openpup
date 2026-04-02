use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use sqlx::{Pool, Row, Sqlite};

use crate::llm::client::LlmClient;
use crate::memory::system::cosine_similarity;

const RRF_K: f32 = 60.0;

/// Weibull decay parameters per memory type.
fn weibull_params(memory_type: &str) -> (f64, f64) {
    // Returns (k, λ)
    match memory_type {
        "rule" => (0.5, 180.0),
        "preference" => (0.8, 60.0),
        "experience" => (1.5, 30.0),
        _ => (1.0, 90.0), // fact / general
    }
}

/// Compute Weibull-based memory strength.
///
/// strength = importance × exp(-(days/λ)^k) × (1 + 0.15 × ln(1 + n))
pub fn memory_strength(
    importance: f64,
    last_accessed: i64,
    memory_type: &str,
    access_count_total: i64,
) -> f64 {
    let now = Utc::now().timestamp();
    let days = (now - last_accessed) as f64 / 86400.0;
    let days = days.max(0.0);

    let (k, lambda) = weibull_params(memory_type);

    // Weibull survival: exp(-(days/λ)^k)
    let survival = (-((days / lambda).powf(k))).exp();

    // Access reinforcement (Ebbinghaus-inspired): 1 + 0.15 × ln(1 + n)
    let reinforcement = 1.0 + 0.15 * ((1.0 + access_count_total as f64).ln());

    importance * survival * reinforcement
}

/// A single memory search result with all scoring metadata.
#[derive(Debug, Clone)]
pub struct MemorySearchResult {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub confidence: f32,
    pub score: f32,
    /// true = forced injection (rule), not from retrieval pipeline
    pub is_forced: bool,
}

pub struct MemoryRetriever {
    pool: Pool<Sqlite>,
    llm: Arc<LlmClient>,
}

impl MemoryRetriever {
    pub fn new(pool: Pool<Sqlite>, llm: Arc<LlmClient>) -> Self {
        Self { pool, llm }
    }

    /// Main retrieval: vector + FTS5 + RRF fusion + Weibull strength weighting.
    ///
    /// Graceful degradation: if embedding fails (model unavailable, API timeout,
    /// etc.), falls back to FTS5-only retrieval rather than returning empty results.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemorySearchResult>> {
        // Step 1: parallel vector + FTS5 retrieval
        let (vec_res, fts_res) = tokio::join!(
            self.vector_search(query, limit * 3),
            self.fts_search(query, limit * 3),
        );
        let vec_failed = vec_res.is_err();
        let vec_res = vec_res.unwrap_or_default();
        let fts_res = fts_res.unwrap_or_default();

        // If both are empty and vector search failed, try a broader FTS5 query
        // by splitting into individual keywords (OR semantics)
        let fts_res = if fts_res.is_empty() && vec_failed {
            let keywords: Vec<&str> = query.split_whitespace().collect();
            if keywords.len() > 1 {
                let or_query = keywords.join(" OR ");
                self.fts_search(&or_query, limit * 3)
                    .await
                    .unwrap_or_default()
            } else {
                fts_res
            }
        } else {
            fts_res
        };

        // Step 2: RRF fusion
        let mut rrf: HashMap<String, f32> = HashMap::new();
        for (rank, (id, _)) in vec_res.iter().enumerate() {
            *rrf.entry(id.clone()).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
        for (rank, (id, _)) in fts_res.iter().enumerate() {
            *rrf.entry(id.clone()).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
        }

        let mut fused: Vec<(String, f32)> = rrf.into_iter().collect();
        fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        fused.truncate(limit * 2);

        if fused.is_empty() {
            return Ok(vec![]);
        }

        // Step 3: batch fetch details + compute Weibull strength in Rust
        let ids: Vec<String> = fused.iter().map(|(id, _)| id.clone()).collect();
        let placeholders = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            r#"
            SELECT id, content, memory_type, confidence, importance,
                   last_accessed, access_count_total
            FROM long_term_memory
            WHERE id IN ({})
              AND memory_type != 'invalidated'
              AND superseded_by IS NULL
            "#,
            placeholders
        );

        let mut q = sqlx::query(&sql);
        for id in &ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(&self.pool).await?;

        // Step 4: merge RRF score with Weibull strength
        let rrf_map: HashMap<String, f32> = fused.into_iter().collect();
        let mut results: Vec<MemorySearchResult> = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let content: String = row.get("content");
                let mem_type: String = row.get("memory_type");
                let confidence: f64 = row.get("confidence");
                let importance: f64 = row.get("importance");
                let last_accessed: i64 = row.get("last_accessed");
                let access_total: i64 = row.get("access_count_total");

                let strength = memory_strength(importance, last_accessed, &mem_type, access_total);
                let rrf_score = rrf_map.get(&id).copied().unwrap_or(0.0);
                let final_score = rrf_score * strength as f32;

                MemorySearchResult {
                    id,
                    content,
                    memory_type: mem_type,
                    confidence: confidence as f32,
                    score: final_score,
                    is_forced: false,
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        // Step 5: update access timestamps (triggers access_count_total increment)
        let now = Utc::now().timestamp();
        for mem in &results {
            let _ = sqlx::query("UPDATE long_term_memory SET last_accessed=? WHERE id=?")
                .bind(now)
                .bind(&mem.id)
                .execute(&self.pool)
                .await;
        }

        Ok(results)
    }

    async fn vector_search(&self, query: &str, limit: usize) -> Result<Vec<(String, f32)>> {
        let q_vec = self.llm.embed(query).await?;

        // Load candidate embeddings and compute cosine similarity in-process
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

        let mut scored: Vec<(String, f32)> = rows
            .into_iter()
            .filter_map(|row| {
                let id: String = row.get("id");
                let emb_json: String = row.get("embedding");
                let emb: Vec<f32> = serde_json::from_str(&emb_json).ok()?;
                let sim = cosine_similarity(&q_vec, &emb);
                if sim > 0.4 {
                    Some((id, sim))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    async fn fts_search(&self, query: &str, limit: usize) -> Result<Vec<(String, f32)>> {
        // FTS5 MATCH can fail on malformed queries; catch gracefully
        let rows: Vec<(String, f64)> = match sqlx::query_as(
            r#"
            SELECT m.id, bm25(memory_fts) AS bm25_score
            FROM memory_fts
            JOIN long_term_memory m ON memory_fts.rowid = m.rowid
            WHERE memory_fts MATCH ?
              AND m.memory_type != 'invalidated'
              AND m.superseded_by IS NULL
            ORDER BY bm25_score
            LIMIT ?
            "#,
        )
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(_) => return Ok(vec![]),
        };

        if rows.is_empty() {
            return Ok(vec![]);
        }

        let max_abs = rows
            .iter()
            .map(|(_, s)| s.abs())
            .fold(f64::NEG_INFINITY, f64::max)
            .max(1.0);

        let results: Vec<(String, f32)> = rows
            .into_iter()
            .map(|(id, score)| (id, (1.0 - score.abs() / max_abs) as f32))
            .collect();

        Ok(results)
    }
}
