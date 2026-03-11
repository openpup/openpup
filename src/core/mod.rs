//! Core 逻辑分组入口：kernel / runtime / agent_runtime / 记忆 / 审计 / Persona / Registry / Workspace。

pub mod kernel;
pub mod runtime;
pub mod agent_runtime;
pub mod scheduler;
pub mod llm;
pub mod runtime_audit;
pub mod memory;
pub mod persona;
pub mod registry;
pub mod workspace;
pub mod gateway;
pub mod orchestrator;
