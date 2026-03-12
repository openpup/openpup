//! openpup library crate.
//!
//! 该 crate 复用核心模块给多个二进制（openpup CLI / gateway / node_worker 等）。

pub mod audit;
pub mod channels;
pub mod cli;
pub mod config;
pub mod core;
pub mod loops;
pub mod tools;

// 兼容：原顶层模块现位于 core，供其他模块使用
pub use crate::core::agent_runtime;
pub use crate::core::llm;
pub use crate::core::memory;
pub use crate::core::persona;
pub use crate::core::registry;
pub use crate::core::runtime;
pub use crate::core::runtime_audit;
pub use crate::core::scheduler;
pub use crate::core::workspace;
