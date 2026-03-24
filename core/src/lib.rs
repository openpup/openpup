#[path = "../../src-tauri/src/runtime.rs"]
pub mod runtime;

#[path = "../../src-tauri/src/config.rs"]
pub mod config;
#[path = "../../src-tauri/src/crypto.rs"]
pub mod crypto;
#[path = "../../src-tauri/src/llm/client.rs"]
pub mod llm_client;
#[path = "../../src-tauri/src/mcp/orchestrator.rs"]
pub mod mcp_orchestrator;
#[path = "../../src-tauri/src/mcp/server.rs"]
pub mod mcp_server;
#[path = "../../src-tauri/src/bridge/mod.rs"]
pub mod bridge;
#[path = "../../src-tauri/src/memory/file_layer.rs"]
pub mod memory_file_layer;
#[path = "../../src-tauri/src/memory/system.rs"]
pub mod memory_system;
#[path = "../../src-tauri/src/tools/primitive.rs"]
pub mod tools_primitive;
#[path = "../../src-tauri/src/skills/registry.rs"]
pub mod skills_registry;
#[path = "../../src-tauri/src/skills/executor.rs"]
pub mod skills_executor;
#[path = "../../src-tauri/src/skills/job_registry.rs"]
pub mod skills_job_registry;
#[path = "../../src-tauri/src/skills/permissions.rs"]
pub mod skills_permissions;
#[path = "../../src-tauri/src/skills/scheduler.rs"]
pub mod skills_scheduler;
#[path = "../../src-tauri/src/channel/dag.rs"]
pub mod channel_dag;
#[path = "../../src-tauri/src/channel/heartbeat.rs"]
pub mod channel_heartbeat;
#[path = "../../src-tauri/src/channel/manager.rs"]
pub mod channel_manager;
#[path = "../../src-tauri/src/channel/types.rs"]
pub mod channel_types;
#[path = "../../src-tauri/src/agents/specialist.rs"]
pub mod agents_specialist;
#[path = "../../src-tauri/src/agents/custom_pup.rs"]
pub mod agents_custom_pup;
#[path = "../../src-tauri/src/agents/dev_pup.rs"]
pub mod agents_dev_pup;
#[path = "../../src-tauri/src/agents/life_admin_pup.rs"]
pub mod agents_life_admin_pup;
#[path = "../../src-tauri/src/agents/ops_pup.rs"]
pub mod agents_ops_pup;
#[path = "../../src-tauri/src/agents/research_pup.rs"]
pub mod agents_research_pup;
#[path = "../../src-tauri/src/agents/writer_pup.rs"]
pub mod agents_writer_pup;
#[path = "../../src-tauri/src/agents/plugins.rs"]
pub mod agents_plugins;
#[path = "../../src-tauri/src/agents/alpha.rs"]
pub mod agents_alpha;

pub mod headless;
pub mod ipc;

pub mod channel {
    pub use crate::channel_dag as dag;
    pub use crate::channel_heartbeat as heartbeat;
    pub use crate::channel_manager as manager;
    pub use crate::channel_types as types;
}

pub mod llm {
    pub use crate::llm_client as client;
}

pub mod mcp {
    pub use crate::mcp_orchestrator as orchestrator;
    pub use crate::mcp_server as server;
}

pub mod memory {
    pub use crate::memory_file_layer as file_layer;
    pub use crate::memory_system as system;
}

pub mod tools {
    pub use crate::tools_primitive as primitive;
}

pub mod skills {
    pub use crate::skills_executor as executor;
    pub use crate::skills_job_registry as job_registry;
    pub use crate::skills_permissions as permissions;
    pub use crate::skills_registry as registry;
    pub use crate::skills_scheduler as scheduler;
}

pub mod agents {
    pub use crate::agents_alpha as alpha;
    pub use crate::agents_custom_pup as custom_pup;
    pub use crate::agents_dev_pup as dev_pup;
    pub use crate::agents_life_admin_pup as life_admin_pup;
    pub use crate::agents_ops_pup as ops_pup;
    pub use crate::agents_plugins as plugins;
    pub use crate::agents_research_pup as research_pup;
    pub use crate::agents_specialist as specialist;
    pub use crate::agents_writer_pup as writer_pup;

    pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }

        for i in (0..=max_bytes).rev() {
            if s.is_char_boundary(i) {
                return &s[..i];
            }
        }

        ""
    }
}
