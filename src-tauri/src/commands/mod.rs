//! Tauri command handlers, split by domain.
//!
//! Each sub-module contains a focused set of commands. `AppState` lives here
//! so every sub-module can import it via `super::AppState`.

use std::sync::Arc;

use crate::agents::alpha::AlphaPup;
use crate::memory::file_layer::FileLayer;

#[derive(Clone)]
pub struct AppState {
    pub alpha: Arc<AlphaPup>,
    pub file_layer: Arc<FileLayer>,
    pub bridge_manager: Arc<crate::bridge::BridgeManager>,
}

pub mod bridge;
pub mod channel;
pub mod chat;
pub mod config;
pub mod context;
pub mod feedback;
pub mod knowledge;
pub mod mcp;
pub mod memory;
pub mod onboarding;
pub mod permissions;
pub mod pups;
pub mod skills;
pub mod tasks;
pub mod timeline;
pub mod workspace;

// Re-export all command functions so main.rs can reference them directly.
pub use bridge::*;
pub use channel::*;
pub use chat::*;
pub use config::*;
pub use context::*;
pub use feedback::*;
pub use knowledge::*;
pub use mcp::*;
pub use memory::*;
pub use onboarding::*;
pub use permissions::*;
pub use pups::*;
pub use skills::*;
pub use tasks::*;
pub use timeline::*;
pub use workspace::*;
