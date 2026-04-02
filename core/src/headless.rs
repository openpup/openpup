use std::path::PathBuf;
use std::sync::Arc;

use openpup_capabilities::Capabilities;

use crate::agents::alpha::AlphaPup;
use crate::app::OpenPupApp;
use crate::channel::manager::ChannelManager;
use crate::mcp::orchestrator::MCPOrchestrator;
use crate::memory::file_layer::FileLayer;
use crate::memory::system::MemorySystem;
use crate::skills::executor::SkillExecutor;
use crate::skills::permissions::PermissionChecker;

#[derive(Clone)]
pub struct HeadlessRuntime {
    pub app: Arc<OpenPupApp>,
    pub workspace_root: PathBuf,
    pub capabilities: Arc<Capabilities>,
    pub alpha: Arc<AlphaPup>,
    pub memory: Arc<MemorySystem>,
    pub file_layer: Arc<FileLayer>,
    pub permissions: PermissionChecker,
    pub skill_executor: Arc<SkillExecutor>,
    pub mcp_orchestrator: Arc<MCPOrchestrator>,
    pub channel_manager: Arc<ChannelManager>,
}

impl HeadlessRuntime {
    pub fn from_app(app: OpenPupApp) -> Self {
        let app = Arc::new(app);
        Self {
            workspace_root: app.workspace_root.clone(),
            capabilities: app.capabilities.clone(),
            alpha: app.alpha.clone(),
            memory: app.memory.clone(),
            file_layer: app.file_layer.clone(),
            permissions: app.permissions.clone(),
            skill_executor: app.skill_executor.clone(),
            mcp_orchestrator: app.mcp_orchestrator.clone(),
            channel_manager: app.channel_manager.clone(),
            app,
        }
    }
}
