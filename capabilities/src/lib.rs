use std::ffi::OsString;
use std::path::PathBuf;
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityProfile {
    DesktopFull,
    AndroidMobile,
    IosRestricted,
}

#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("capability `{0}` is not supported on this runtime")]
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub command: String,
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub sandboxed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub sandbox_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub url: String,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub final_url: String,
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: Option<String>,
    pub name: String,
    pub schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRequest {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskRequest {
    pub name: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEvent {
    pub event: String,
    pub payload: Value,
}

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn read_to_string(&self, path: &Path) -> anyhow::Result<String>;
    async fn write_string(&self, path: &Path, content: &str) -> anyhow::Result<()>;
    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()>;
    async fn metadata(&self, path: &Path) -> anyhow::Result<std::fs::Metadata>;
}

pub struct RestrictedFileSystem {
    inner: Arc<dyn FileSystem>,
    allowed_roots: Vec<PathBuf>,
}

impl RestrictedFileSystem {
    pub fn new(inner: Arc<dyn FileSystem>, allowed_roots: Vec<PathBuf>) -> anyhow::Result<Self> {
        let mut normalized_roots = Vec::new();
        for root in allowed_roots {
            let normalized = normalize_path(&root)?;
            normalized_roots.push(normalized);
            if let Some(canonical) = canonicalize_existing_prefix(&root)? {
                normalized_roots.push(canonical);
            }
        }
        normalized_roots.sort();
        normalized_roots.dedup();
        if normalized_roots.is_empty() {
            return Err(anyhow!(
                "restricted filesystem requires at least one allowed root"
            ));
        }
        Ok(Self {
            inner,
            allowed_roots: normalized_roots,
        })
    }

    fn ensure_allowed(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let normalized = normalize_path(path)?;
        if !self.is_under_allowed_root(&normalized) {
            return Err(anyhow!(
                "filesystem access denied outside allowed roots: {}",
                normalized.display()
            ));
        }

        if let Some(canonical) = canonicalize_existing_prefix(&normalized)? {
            if !self.is_under_allowed_root(&canonical) {
                return Err(anyhow!(
                    "filesystem access denied through path outside allowed roots: {}",
                    canonical.display()
                ));
            }
        }

        Ok(normalized)
    }

    fn is_under_allowed_root(&self, path: &Path) -> bool {
        self.allowed_roots.iter().any(|root| path.starts_with(root))
    }
}

#[async_trait]
impl FileSystem for RestrictedFileSystem {
    async fn read_to_string(&self, path: &Path) -> anyhow::Result<String> {
        let path = self.ensure_allowed(path)?;
        self.inner.read_to_string(&path).await
    }

    async fn write_string(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        let path = self.ensure_allowed(path)?;
        self.inner.write_string(&path, content).await
    }

    async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
        let path = self.ensure_allowed(path)?;
        self.inner.create_dir_all(&path).await
    }

    async fn metadata(&self, path: &Path) -> anyhow::Result<std::fs::Metadata> {
        let path = self.ensure_allowed(path)?;
        self.inner.metadata(&path).await
    }
}

fn normalize_path(path: &Path) -> anyhow::Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(anyhow!("path escapes root: {}", path.display()));
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    Ok(out)
}

fn canonicalize_existing_prefix(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(Some(canonical));
    }

    let mut current = path;
    let mut suffix: Vec<OsString> = Vec::new();
    while let Some(parent) = current.parent() {
        if parent == current {
            break;
        }
        if let Some(name) = current.file_name() {
            suffix.push(name.to_os_string());
        }
        if parent.exists() {
            let mut canonical = std::fs::canonicalize(parent)
                .with_context(|| format!("canonicalize '{}'", parent.display()));
            if let Ok(canonical) = canonical.as_mut() {
                for part in suffix.iter().rev() {
                    canonical.push(part);
                }
            }
            return canonical.map(Some);
        }
        current = parent;
    }
    Ok(None)
}

pub trait SecureStore: Send + Sync {
    fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>>;
    fn set_secret(&self, key: &str, value: &str) -> anyhow::Result<()>;
    fn delete_secret(&self, key: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub trait ProcessExecutor: Send + Sync {
    fn is_supported(&self) -> bool;
    async fn exec(&self, req: ExecRequest) -> anyhow::Result<ExecResult>;
}

#[async_trait]
pub trait NetworkClient: Send + Sync {
    async fn get(&self, req: HttpRequest) -> anyhow::Result<HttpResponse>;
}

#[async_trait]
pub trait Scheduler: Send + Sync {
    fn is_supported(&self) -> bool;
    async fn schedule(&self, job: ScheduledJob) -> anyhow::Result<String>;
    async fn cancel(&self, job_id: &str) -> anyhow::Result<()>;
    async fn list(&self) -> anyhow::Result<Vec<ScheduledJobInfo>>;
}

#[async_trait]
pub trait Notifier: Send + Sync {
    fn is_supported(&self) -> bool;
    async fn notify(&self, msg: NotificationRequest) -> anyhow::Result<()>;
}

pub trait PluginHost: Send + Sync {
    fn is_supported(&self) -> bool;
    fn plugin_paths(&self) -> anyhow::Result<Vec<PathBuf>>;
}

#[async_trait]
pub trait BackgroundTasks: Send + Sync {
    fn is_supported(&self) -> bool;
    async fn enqueue(&self, task: BackgroundTaskRequest) -> anyhow::Result<String>;
}

#[async_trait]
pub trait AppBridge: Send + Sync {
    async fn emit_event(&self, event: AppEvent) -> anyhow::Result<()>;
}

pub trait Environment: Send + Sync {
    fn platform(&self) -> String;
    fn capability_profile(&self) -> CapabilityProfile;
    fn app_version(&self) -> String;
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct Capabilities {
    pub fs: Arc<dyn FileSystem>,
    pub secure_store: Arc<dyn SecureStore>,
    pub process: Arc<dyn ProcessExecutor>,
    pub net: Arc<dyn NetworkClient>,
    pub scheduler: Arc<dyn Scheduler>,
    pub notifier: Arc<dyn Notifier>,
    pub plugins: Arc<dyn PluginHost>,
    pub background: Arc<dyn BackgroundTasks>,
    pub bridge: Arc<dyn AppBridge>,
    pub env: Arc<dyn Environment>,
    pub clock: Arc<dyn Clock>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopFileSystem;

    #[async_trait]
    impl FileSystem for NoopFileSystem {
        async fn read_to_string(&self, _path: &Path) -> anyhow::Result<String> {
            unimplemented!()
        }

        async fn write_string(&self, _path: &Path, _content: &str) -> anyhow::Result<()> {
            unimplemented!()
        }

        async fn create_dir_all(&self, _path: &Path) -> anyhow::Result<()> {
            unimplemented!()
        }

        async fn metadata(&self, _path: &Path) -> anyhow::Result<std::fs::Metadata> {
            unimplemented!()
        }
    }

    #[test]
    fn restricted_fs_allows_paths_under_roots() {
        let root = std::env::temp_dir().join("openpup-workspace");
        let fs = RestrictedFileSystem::new(Arc::new(NoopFileSystem), vec![root.clone()]).unwrap();

        assert!(fs.ensure_allowed(&root.join("notes/today.md")).is_ok());
    }

    #[test]
    fn restricted_fs_rejects_parent_escape() {
        let root = std::env::temp_dir().join("openpup-workspace");
        let fs = RestrictedFileSystem::new(Arc::new(NoopFileSystem), vec![root.clone()]).unwrap();

        assert!(fs.ensure_allowed(&root.join("../secret.txt")).is_err());
    }

    #[test]
    fn restricted_fs_rejects_unlisted_roots() {
        let root = std::env::temp_dir().join("openpup-workspace");
        let outside = std::env::temp_dir().join("openpup-outside/secret.txt");
        let fs = RestrictedFileSystem::new(Arc::new(NoopFileSystem), vec![root]).unwrap();

        assert!(fs.ensure_allowed(&outside).is_err());
    }
}
