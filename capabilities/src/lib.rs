use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

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
