//! Local embedding engine using fastembed as fallback.
//! Uses MultilingualE5Base (768-dim) for compatibility with remote bge-m3 embeddings.

use std::sync::Mutex;

use anyhow::{anyhow, Result};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use tracing::{info, warn};

/// Lazy-initialized local embedding engine.
pub struct LocalEmbedder {
    inner: Mutex<Option<TextEmbedding>>,
}

impl LocalEmbedder {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Embed a single text. Initializes the model on first call (downloads ~400MB model).
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| anyhow!("local embedder lock poisoned"))?;

        if guard.is_none() {
            info!("[local_embed] Initializing fastembed MultilingualE5Base (768-dim)...");
            let opts = TextInitOptions::new(EmbeddingModel::MultilingualE5Base)
                .with_show_download_progress(true);
            let model = TextEmbedding::try_new(opts);

            match model {
                Ok(m) => {
                    info!("[local_embed] Model loaded successfully");
                    *guard = Some(m);
                }
                Err(e) => {
                    warn!("[local_embed] Failed to init model: {e}");
                    return Err(anyhow!("local embed init failed: {e}"));
                }
            }
        }

        let engine = guard.as_mut().unwrap();
        let results = engine
            .embed(vec![text], None)
            .map_err(|e| anyhow!("local embed failed: {e}"))?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("local embed: empty result"))
    }
}
