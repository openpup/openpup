#[path = "../../src-tauri/src/runtime.rs"]
pub mod runtime;

#[path = "../../src-tauri/src/agents/mod.rs"]
pub mod agents;

#[path = "../../src-tauri/src/bridge/mod.rs"]
pub mod bridge;

#[path = "../../src-tauri/src/channel/mod.rs"]
pub mod channel;

#[path = "../../src-tauri/src/config.rs"]
pub mod config;

#[path = "../../src-tauri/src/crypto.rs"]
pub mod crypto;

#[path = "../../src-tauri/src/llm/client.rs"]
pub mod llm_client;

pub mod llm {
    pub use crate::llm_client as client;

    /// Stub local_embed module for the core crate (fastembed not available).
    pub mod local_embed {
        pub struct LocalEmbedder;

        impl LocalEmbedder {
            pub fn new() -> Self {
                Self
            }

            pub fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
                Err(anyhow::anyhow!(
                    "local embedding not available in core crate (fastembed not linked)"
                ))
            }
        }
    }
}

#[path = "../../src-tauri/src/mcp/mod.rs"]
pub mod mcp;

#[path = "../../src-tauri/src/memory/mod.rs"]
pub mod memory;

#[path = "../../src-tauri/src/skills/mod.rs"]
pub mod skills;

#[path = "../../src-tauri/src/tools/mod.rs"]
pub mod tools;

#[path = "../../src-tauri/src/knowledge/mod.rs"]
pub mod knowledge;

pub mod headless;
pub mod ipc;
