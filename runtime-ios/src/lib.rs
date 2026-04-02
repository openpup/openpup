use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use openpup_capabilities::{
    AppBridge, AppEvent, BackgroundTaskRequest, BackgroundTasks, Capabilities, CapabilityError,
    CapabilityProfile, Clock, Environment, ExecRequest, ExecResult, FileSystem, HttpRequest,
    HttpResponse, NetworkClient, NotificationRequest, Notifier, PluginHost, ProcessExecutor,
    ScheduledJob, ScheduledJobInfo, Scheduler, SecureStore,
};
use openpup_core::app::{build_app, OpenPupApp};
use openpup_core::skills::permissions::PermissionUi;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

pub struct IosRuntimeFactory;

impl IosRuntimeFactory {
    pub fn workspace_root() -> Result<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(path) = std::env::var("OPENPUP_MOBILE_WORKSPACE_ROOT") {
            candidates.push(PathBuf::from(path));
        }
        if let Some(root) = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .or_else(|| Some(std::env::temp_dir()))
        {
            candidates.push(root.join("openpup-mobile"));
        }

        let mut errors = Vec::new();
        for candidate in candidates {
            match ensure_writable_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(err) => errors.push(format!("{}: {}", candidate.display(), err)),
            }
        }

        Err(anyhow!(
            "cannot determine writable iOS workspace root: {}",
            errors.join("; ")
        ))
    }

    pub fn build_capabilities(workspace_root: PathBuf) -> Arc<Capabilities> {
        let state_root = workspace_root.join("mobile_runtime");
        Arc::new(Capabilities {
            fs: Arc::new(MobileFileSystem),
            secure_store: Arc::new(FileBackedSecureStore::new(state_root.join("secure_store.json"))),
            process: Arc::new(UnsupportedProcessExecutor::new("process_executor")),
            net: Arc::new(MobileNetworkClient::new()),
            scheduler: Arc::new(FileBackedScheduler::new(state_root.join("scheduled_jobs.json"))),
            notifier: Arc::new(FileBackedNotifier::new(state_root.join("notifications.jsonl"))),
            plugins: Arc::new(UnsupportedPluginHost::new("plugin_host")),
            background: Arc::new(FileBackedBackgroundTasks::new(state_root.join("background_tasks.jsonl"))),
            bridge: Arc::new(NoopAppBridge),
            env: Arc::new(IosEnvironment),
            clock: Arc::new(SystemClock),
        })
    }

    pub async fn build_app(permission_ui: Option<Arc<dyn PermissionUi>>) -> Result<OpenPupApp> {
        let workspace_root = Self::workspace_root()?;
        let capabilities = Self::build_capabilities(workspace_root.clone());
        build_app(workspace_root, capabilities, permission_ui, Vec::new()).await
    }
}

fn ensure_writable_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let probe = path.join(".write_test");
    std::fs::write(&probe, b"ok")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

struct MobileFileSystem;

#[async_trait]
impl FileSystem for MobileFileSystem {
    async fn read_to_string(&self, path: &Path) -> Result<String> {
        Ok(tokio::fs::read_to_string(path).await?)
    }

    async fn write_string(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(path).await?;
        Ok(())
    }

    async fn metadata(&self, path: &Path) -> Result<std::fs::Metadata> {
        Ok(tokio::fs::metadata(path).await?)
    }
}

struct FileBackedSecureStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedSecureStore {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read_map(&self) -> Result<std::collections::HashMap<String, String>> {
        if !self.path.exists() {
            return Ok(Default::default());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(&self.path)?)?)
    }

    fn write_map(&self, map: &std::collections::HashMap<String, String>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(map)?)?;
        Ok(())
    }
}

impl SecureStore for FileBackedSecureStore {
    fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let _guard = self.lock.blocking_lock();
        Ok(self.read_map()?.get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        let _guard = self.lock.blocking_lock();
        let mut map = self.read_map()?;
        map.insert(key.to_string(), value.to_string());
        self.write_map(&map)
    }

    fn delete_secret(&self, key: &str) -> Result<()> {
        let _guard = self.lock.blocking_lock();
        let mut map = self.read_map()?;
        map.remove(key);
        self.write_map(&map)
    }
}

struct UnsupportedProcessExecutor {
    name: &'static str,
}

impl UnsupportedProcessExecutor {
    fn new(name: &'static str) -> Self {
        Self { name }
    }
}

#[async_trait]
impl ProcessExecutor for UnsupportedProcessExecutor {
    fn is_supported(&self) -> bool {
        false
    }

    async fn exec(&self, _req: ExecRequest) -> Result<ExecResult> {
        Err(CapabilityError::Unsupported(self.name).into())
    }
}

struct MobileNetworkClient {
    client: reqwest::Client,
}

impl MobileNetworkClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl NetworkClient for MobileNetworkClient {
    async fn get(&self, req: HttpRequest) -> Result<HttpResponse> {
        let mut request = self.client.get(&req.url);
        if let Some(user_agent) = req.user_agent {
            request = request.header("User-Agent", user_agent);
        }
        let resp = request.send().await?;
        Ok(HttpResponse {
            final_url: resp.url().to_string(),
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotificationEnvelope {
    title: String,
    body: String,
    created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackgroundEnvelope {
    name: String,
    payload: Value,
    created_at_ms: i64,
}

struct FileBackedScheduler {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedScheduler {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl Scheduler for FileBackedScheduler {
    fn is_supported(&self) -> bool {
        true
    }

    async fn schedule(&self, job: ScheduledJob) -> Result<String> {
        let _guard = self.lock.lock().await;
        let mut jobs: Vec<ScheduledJobInfo> = read_json_file(&self.path).await?.unwrap_or_default();
        let id = job.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        jobs.retain(|existing| existing.id != id);
        jobs.push(ScheduledJobInfo {
            id: id.clone(),
            name: job.name,
            schedule: job.schedule,
        });
        write_json_file(&self.path, &jobs).await?;
        Ok(id)
    }

    async fn cancel(&self, job_id: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut jobs: Vec<ScheduledJobInfo> = read_json_file(&self.path).await?.unwrap_or_default();
        jobs.retain(|job| job.id != job_id);
        write_json_file(&self.path, &jobs).await
    }

    async fn list(&self) -> Result<Vec<ScheduledJobInfo>> {
        Ok(read_json_file(&self.path).await?.unwrap_or_default())
    }
}

struct FileBackedNotifier {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedNotifier {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl Notifier for FileBackedNotifier {
    fn is_supported(&self) -> bool {
        true
    }

    async fn notify(&self, msg: NotificationRequest) -> Result<()> {
        let _guard = self.lock.lock().await;
        append_jsonl(
            &self.path,
            &NotificationEnvelope {
                title: msg.title,
                body: msg.body,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
            },
        )
        .await
    }
}

struct UnsupportedPluginHost {
    name: &'static str,
}

impl UnsupportedPluginHost {
    fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl PluginHost for UnsupportedPluginHost {
    fn is_supported(&self) -> bool {
        false
    }

    fn plugin_paths(&self) -> Result<Vec<PathBuf>> {
        Err(CapabilityError::Unsupported(self.name).into())
    }
}

struct FileBackedBackgroundTasks {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedBackgroundTasks {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl BackgroundTasks for FileBackedBackgroundTasks {
    fn is_supported(&self) -> bool {
        true
    }

    async fn enqueue(&self, task: BackgroundTaskRequest) -> Result<String> {
        let _guard = self.lock.lock().await;
        let id = uuid::Uuid::new_v4().to_string();
        append_jsonl(
            &self.path,
            &BackgroundEnvelope {
                name: format!("{id}:{}", task.name),
                payload: task.payload,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
            },
        )
        .await?;
        Ok(id)
    }
}

struct NoopAppBridge;

#[async_trait]
impl AppBridge for NoopAppBridge {
    async fn emit_event(&self, _event: AppEvent) -> Result<()> {
        Ok(())
    }
}

struct IosEnvironment;

impl Environment for IosEnvironment {
    fn platform(&self) -> String {
        "ios".to_string()
    }

    fn capability_profile(&self) -> CapabilityProfile {
        CapabilityProfile::IosRestricted
    }

    fn app_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&tokio::fs::read_to_string(path).await?)?))
}

async fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serde_json::to_vec_pretty(value)?).await?;
    Ok(())
}

async fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(&line).await?;
    Ok(())
}
