//! Local embedding stub — fastembed has been removed.
//! Remote embedding API is the only supported path; configure OPENPUP_API_KEY.

use anyhow::{anyhow, Result};

/// Placeholder that always returns an error directing users to the remote API.
pub struct LocalEmbedder;

impl LocalEmbedder {
    pub fn new() -> Self {
        Self
    }

    pub fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Err(anyhow!(
            "local embedding unavailable (fastembed removed). \
             Please configure a remote embedding API via OPENPUP_API_KEY."
        ))
    }
}
