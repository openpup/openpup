use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use openpup_capabilities::{
    AppBridge, AppEvent, BackgroundTaskRequest, BackgroundTasks, Capabilities, CapabilityError,
    CapabilityProfile, Clock, Environment, ExecRequest, ExecResult, FileSystem, HttpRequest,
    HttpResponse, NetworkClient, NotificationRequest, Notifier, PluginHost, ProcessExecutor,
    RestrictedFileSystem, ScheduledJob, ScheduledJobInfo, Scheduler, SecureStore,
};
use openpup_core::agents::specialist::SpecialistPup;
use openpup_core::app::{build_app, OpenPupApp};
use openpup_core::headless::HeadlessRuntime;
use openpup_core::skills::permissions::PermissionUi;

#[allow(improper_ctypes_definitions)]
type CreatePupFn = unsafe extern "C" fn() -> *mut dyn SpecialistPup;

pub struct DesktopRuntimeFactory;

impl DesktopRuntimeFactory {
    pub fn workspace_root() -> Result<PathBuf> {
        if let Ok(root) = std::env::var("OPENPUP_WORKSPACE") {
            if !root.trim().is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
        if let Ok(root) = std::env::var("OPENPUP_APP_ROOT") {
            if !root.trim().is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
        Ok(home_dir.join(".openpup"))
    }

    pub fn build_capabilities(workspace_root: PathBuf) -> Arc<Capabilities> {
        let fs = restricted_fs(Arc::new(DesktopFileSystem), &workspace_root);
        Arc::new(Capabilities {
            fs,
            secure_store: Arc::new(NoopSecureStore),
            process: Arc::new(DesktopProcessExecutor),
            net: Arc::new(DesktopNetworkClient::new()),
            scheduler: Arc::new(UnsupportedScheduler),
            notifier: Arc::new(UnsupportedNotifier),
            plugins: Arc::new(DesktopPluginHost::new(workspace_root.join("plugins"))),
            background: Arc::new(UnsupportedBackgroundTasks),
            bridge: Arc::new(NoopAppBridge),
            env: Arc::new(DesktopEnvironment),
            clock: Arc::new(SystemClock),
        })
    }

    pub async fn build_app(permission_ui: Option<Arc<dyn PermissionUi>>) -> Result<OpenPupApp> {
        let workspace_root = Self::workspace_root()?;
        let capabilities = Self::build_capabilities(workspace_root.clone());
        let dynamic_pups = load_dynamic_pups(capabilities.plugins.as_ref())?;
        build_app(workspace_root, capabilities, permission_ui, dynamic_pups).await
    }

    pub async fn build_headless(
        permission_ui: Option<Arc<dyn PermissionUi>>,
    ) -> Result<HeadlessRuntime> {
        let app = Self::build_app(permission_ui).await?;
        Ok(HeadlessRuntime::from_app(app))
    }
}

fn restricted_fs(inner: Arc<dyn FileSystem>, workspace_root: &Path) -> Arc<dyn FileSystem> {
    let mut roots = vec![workspace_root.to_path_buf(), std::env::temp_dir()];
    if let Some(data_dir) = dirs::data_local_dir() {
        roots.push(data_dir.join("openpup"));
    }
    Arc::new(
        RestrictedFileSystem::new(inner, roots)
            .expect("restricted filesystem should have allowed roots"),
    )
}

struct DesktopFileSystem;

#[async_trait]
impl FileSystem for DesktopFileSystem {
    async fn read_to_string(&self, path: &Path) -> Result<String> {
        Ok(tokio::fs::read_to_string(path).await?)
    }

    async fn write_string(&self, path: &Path, content: &str) -> Result<()> {
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

struct NoopSecureStore;

impl SecureStore for NoopSecureStore {
    fn get_secret(&self, _key: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn set_secret(&self, _key: &str, _value: &str) -> Result<()> {
        Err(CapabilityError::Unsupported("secure_store").into())
    }

    fn delete_secret(&self, _key: &str) -> Result<()> {
        Ok(())
    }
}

struct DesktopProcessExecutor;

impl DesktopProcessExecutor {
    fn build_shell_command(command: &str) -> tokio::process::Command {
        #[cfg(windows)]
        {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/d", "/s", "/c", command]);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(["-c", command]);
            cmd
        }
    }

    #[cfg(not(windows))]
    async fn kill_process_tree(pid: u32) {
        use tokio::process::Command;
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .output()
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .output()
            .await;
    }

    #[cfg(windows)]
    async fn kill_process_tree(pid: u32) {
        use tokio::process::Command;
        let _ = Command::new("taskkill")
            .args(["/T", "/PID", &pid.to_string()])
            .output()
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await;
    }
}

#[async_trait]
impl ProcessExecutor for DesktopProcessExecutor {
    fn is_supported(&self) -> bool {
        true
    }

    async fn exec(&self, req: ExecRequest) -> Result<ExecResult> {
        let sandbox_dir = if req.sandboxed {
            Some(tempfile::tempdir()?)
        } else {
            None
        };
        let cwd = sandbox_dir
            .as_ref()
            .map(|dir| dir.path().to_path_buf())
            .unwrap_or(req.cwd);

        let mut cmd = Self::build_shell_command(&req.command);
        cmd.current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if req.sandboxed {
            #[cfg(windows)]
            {
                cmd.env_clear()
                    .env("PATH", std::env::var("PATH").unwrap_or_default())
                    .env(
                        "SYSTEMROOT",
                        std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into()),
                    )
                    .env("TEMP", &cwd)
                    .env("TMP", &cwd)
                    .env("USERPROFILE", &cwd);
            }
            #[cfg(not(windows))]
            {
                cmd.env_clear()
                    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                    .env("HOME", &cwd)
                    .env("TMPDIR", &cwd);
            }
        }

        let child = cmd
            .spawn()
            .map_err(|e| anyhow!("process spawn failed: {e}"))?;
        let pid = child.id();
        let output = tokio::time::timeout(
            Duration::from_millis(req.timeout_ms),
            child.wait_with_output(),
        )
        .await;

        match output {
            Ok(Ok(output)) => Ok(ExecResult {
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.status.code(),
                timed_out: false,
                sandbox_dir: sandbox_dir.as_ref().map(|dir| dir.path().to_path_buf()),
            }),
            Ok(Err(e)) => Err(anyhow!("process execution failed: {e}")),
            Err(_) => {
                if let Some(pid) = pid {
                    Self::kill_process_tree(pid).await;
                }
                Ok(ExecResult {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    exit_code: None,
                    timed_out: true,
                    sandbox_dir: sandbox_dir.as_ref().map(|dir| dir.path().to_path_buf()),
                })
            }
        }
    }
}

struct DesktopNetworkClient {
    client: reqwest::Client,
}

impl DesktopNetworkClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl NetworkClient for DesktopNetworkClient {
    async fn get(&self, req: HttpRequest) -> Result<HttpResponse> {
        let mut request = self.client.get(&req.url);
        if let Some(user_agent) = req.user_agent {
            request = request.header("User-Agent", user_agent);
        }
        let resp = request.send().await?;
        let status = resp.status().as_u16();
        let final_url = resp.url().to_string();
        let body = resp.text().await.unwrap_or_default();
        Ok(HttpResponse {
            final_url,
            status,
            body,
        })
    }
}

struct UnsupportedScheduler;

#[async_trait]
impl Scheduler for UnsupportedScheduler {
    fn is_supported(&self) -> bool {
        false
    }

    async fn schedule(&self, _job: ScheduledJob) -> Result<String> {
        Err(CapabilityError::Unsupported("scheduler").into())
    }

    async fn cancel(&self, _job_id: &str) -> Result<()> {
        Err(CapabilityError::Unsupported("scheduler").into())
    }

    async fn list(&self) -> Result<Vec<ScheduledJobInfo>> {
        Ok(Vec::new())
    }
}

struct UnsupportedNotifier;

#[async_trait]
impl Notifier for UnsupportedNotifier {
    fn is_supported(&self) -> bool {
        false
    }

    async fn notify(&self, _msg: NotificationRequest) -> Result<()> {
        Err(CapabilityError::Unsupported("notifier").into())
    }
}

struct DesktopPluginHost {
    root: PathBuf,
}

impl DesktopPluginHost {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl PluginHost for DesktopPluginHost {
    fn is_supported(&self) -> bool {
        true
    }

    fn plugin_paths(&self) -> Result<Vec<PathBuf>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut libs = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                if matches!(ext, "dylib" | "so" | "dll") {
                    libs.push(path);
                }
            }
        }
        Ok(libs)
    }
}

struct UnsupportedBackgroundTasks;

#[async_trait]
impl BackgroundTasks for UnsupportedBackgroundTasks {
    fn is_supported(&self) -> bool {
        false
    }

    async fn enqueue(&self, _task: BackgroundTaskRequest) -> Result<String> {
        Err(CapabilityError::Unsupported("background_tasks").into())
    }
}

struct NoopAppBridge;

#[async_trait]
impl AppBridge for NoopAppBridge {
    async fn emit_event(&self, _event: AppEvent) -> Result<()> {
        Ok(())
    }
}

struct DesktopEnvironment;

impl Environment for DesktopEnvironment {
    fn platform(&self) -> String {
        std::env::consts::OS.to_string()
    }

    fn capability_profile(&self) -> CapabilityProfile {
        CapabilityProfile::DesktopFull
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

fn load_dynamic_pups(host: &dyn PluginHost) -> Result<Vec<Arc<dyn SpecialistPup>>> {
    if !host.is_supported() {
        return Ok(Vec::new());
    }

    let mut pups: Vec<Arc<dyn SpecialistPup>> = Vec::new();
    for path in host.plugin_paths()? {
        if let Ok(pup) = load_one_plugin(&path) {
            pups.push(pup);
        }
    }
    Ok(pups)
}

fn load_one_plugin(path: &PathBuf) -> Result<Arc<dyn SpecialistPup>> {
    let lib = unsafe { libloading::Library::new(path)? };
    let lib = Box::leak(Box::new(lib));

    unsafe {
        let constructor: libloading::Symbol<CreatePupFn> = lib.get(b"create_pup")?;
        let raw = constructor();
        if raw.is_null() {
            anyhow::bail!("create_pup returned null");
        }
        let boxed: Box<dyn SpecialistPup> = Box::from_raw(raw);
        Ok(Arc::from(boxed))
    }
}
