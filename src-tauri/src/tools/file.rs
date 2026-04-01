/// File tool implementations for ToolRegistry.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use tracing::debug;

use super::primitive::ToolRegistry;

impl ToolRegistry {
    pub(crate) async fn file_read(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        debug!("[tool/file_read] {}", resolved.display());
        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow!("file_read '{}': {e}", resolved.display()))?;
        Ok(self.dynamic_truncate(&content))
    }

    pub(crate) async fn file_write(&self, path: &str, content: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        debug!(
            "[tool/file_write] {} ({} bytes)",
            resolved.display(),
            content.len()
        );
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow!("file_write mkdir '{}': {e}", parent.display()))?;
        }
        tokio::fs::write(&resolved, content)
            .await
            .map_err(|e| anyhow!("file_write '{}': {e}", resolved.display()))?;
        Ok(format!(
            "Written {} bytes to '{}'",
            content.len(),
            resolved.display()
        ))
    }

    pub(crate) async fn skill_list_resources(&self, skill_name: &str, limit: usize) -> Result<String> {
        debug!("[tool/skill_list_resources] {} limit={}", skill_name, limit);
        let listing = self
            .skill_registry
            .list_skill_resources(skill_name, limit)
            .await?;
        Ok(self.dynamic_truncate(&listing))
    }

    pub(crate) async fn skill_read_resource(&self, skill_name: &str, relpath: &str) -> Result<String> {
        let resolved = self
            .skill_registry
            .resolve_skill_resource_path(skill_name, relpath)
            .await?;
        debug!(
            "[tool/skill_read_resource] {}:{} -> {}",
            skill_name,
            relpath,
            resolved.display()
        );
        let metadata = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| anyhow!("skill_read_resource '{}:{}': {e}", skill_name, relpath))?;
        if metadata.is_dir() {
            return Err(anyhow!(
                "skill_read_resource '{}:{}' resolved to a directory, not a file",
                skill_name,
                relpath
            ));
        }
        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| anyhow!("skill_read_resource '{}:{}': {e}", skill_name, relpath))?;
        Ok(self.dynamic_truncate(&content))
    }

    pub(crate) fn resolve_path(&self, path: &str) -> PathBuf {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workspace_root.join(path)
        }
    }
}
