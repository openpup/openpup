use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::runtime::{emit_event, SharedEventSink};

// ── Execution mode ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Leashed,
    FreeRun,
}

// ── Action types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Action {
    DeleteFile(String),
    SendPublicMessage(String),
    MakePayment(f64),
    ModifySystemConfig(String),
    Other(String),
}

impl Action {
    pub fn is_dangerous(&self) -> bool {
        matches!(
            self,
            Action::DeleteFile(_)
                | Action::SendPublicMessage(_)
                | Action::MakePayment(_)
                | Action::ModifySystemConfig(_)
        )
    }
}

// ── Payloads ───────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct PermissionDetails {
    pub affected_files: Option<Vec<String>>,
    pub network_destinations: Option<Vec<String>>,
    pub estimated_cost: Option<f64>,
}

#[derive(Clone, Serialize)]
pub struct PermissionRequestPayload {
    /// Unique ID matched when the frontend calls `approve_permission` / `deny_permission`.
    pub request_id: String,
    pub skill_name: String,
    pub action_description: String,
    pub risk_level: String, // "high" | "low"
    pub details: PermissionDetails,
}

#[async_trait]
pub trait PermissionUi: Send + Sync {
    async fn request_permission(&self, payload: PermissionRequestPayload) -> Result<bool>;
}

// ── PermissionChecker ─────────────────────────────────────────────────────────

/// All interior state is `Arc`-wrapped so `Clone` is cheap and correct.
/// `app_handle` is stored in an `OnceLock` and initialised in Tauri's `setup()`
/// (the only place where `AppHandle` is available before the event loop starts).
#[derive(Clone)]
pub struct PermissionChecker {
    mode: Arc<RwLock<ExecutionMode>>,
    trusted_skills: Arc<RwLock<HashSet<String>>>,
    /// Optional event sink for GUI-style permission requests.
    event_sink: Arc<OnceLock<SharedEventSink>>,
    /// Optional interactive permission UI for headless runtimes.
    interactive_ui: Arc<Mutex<Option<Arc<dyn PermissionUi>>>>,
    /// In-flight requests awaiting user response.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Path to persist trusted skills JSON.
    trusted_skills_path: Arc<Mutex<Option<PathBuf>>>,
}

impl PermissionChecker {
    pub fn new() -> Self {
        Self {
            mode: Arc::new(RwLock::new(ExecutionMode::Leashed)),
            trusted_skills: Arc::new(RwLock::new(HashSet::new())),
            event_sink: Arc::new(OnceLock::new()),
            interactive_ui: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            trusted_skills_path: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the path to persist trusted skills and load any existing data.
    pub fn set_persist_path(&self, path: PathBuf) {
        if path.exists() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(skills) = serde_json::from_str::<Vec<String>>(&text) {
                    if let Ok(mut guard) = self.trusted_skills.try_write() {
                        for s in skills {
                            guard.insert(s);
                        }
                    }
                }
            }
        }
        if let Ok(mut p) = self.trusted_skills_path.lock() {
            *p = Some(path);
        }
    }

    pub fn set_event_sink(&self, sink: SharedEventSink) {
        let _ = self.event_sink.set(sink);
    }

    pub fn set_permission_ui(&self, ui: Arc<dyn PermissionUi>) {
        if let Ok(mut guard) = self.interactive_ui.lock() {
            *guard = Some(ui);
        }
    }

    // ── Public API ──────────────────────────────────────────────────────────────

    pub async fn get_mode(&self) -> ExecutionMode {
        *self.mode.read().await
    }

    pub async fn set_mode(&self, mode: ExecutionMode) {
        *self.mode.write().await = mode;
    }

    pub async fn check_permission(&self, skill_name: &str, action: &Action) -> Result<bool> {
        match self.get_mode().await {
            ExecutionMode::Leashed => {
                if action.is_dangerous() {
                    self.request_user_confirmation(skill_name, action).await
                } else {
                    Ok(true)
                }
            }
            ExecutionMode::FreeRun => {
                let trusted = self.trusted_skills.read().await;
                if trusted.contains(skill_name) && !action.is_dangerous() {
                    Ok(true)
                } else {
                    self.request_user_confirmation(skill_name, action).await
                }
            }
        }
    }

    /// Called by `approve_permission` / `deny_permission` commands to resolve
    /// a pending request.
    pub fn respond(&self, request_id: &str, approved: bool) {
        if let Some(tx) = self.pending.lock().unwrap().remove(request_id) {
            let _ = tx.send(approved);
        }
    }

    pub async fn trust_skill(&self, skill_name: &str) {
        self.trusted_skills
            .write()
            .await
            .insert(skill_name.to_string());
        // Persist to JSON
        let skills: Vec<String> = self.trusted_skills.read().await.iter().cloned().collect();
        if let Ok(path_guard) = self.trusted_skills_path.lock() {
            if let Some(ref path) = *path_guard {
                if let Ok(text) = serde_json::to_string_pretty(&skills) {
                    let _ =
                        std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
                    let _ = std::fs::write(path, text);
                }
            }
        }
    }

    pub async fn request_via_bridge(
        &self,
        action_desc: &str,
        skill: &str,
        bridge_ctx: &crate::bridge::types::BridgeContext,
    ) -> Result<bool> {
        bridge_ctx
            .out_tx
            .send(crate::bridge::types::OutboundMessage {
                platform: bridge_ctx.platform.clone(),
                chat_id: bridge_ctx.chat_id.clone(),
                text: crate::bridge::formatter::format_perm_request(action_desc, skill),
                reply_to_id: None,
                msg_type: crate::bridge::types::OutboundType::PermRequest,
            })
            .await?;

        let reply = timeout(Duration::from_secs(300), bridge_ctx.wait_for_reply()).await?;
        Ok(matches!(
            reply.as_deref(),
            Some("yes") | Some("YES") | Some("Yes")
        ))
    }

    // ── Internal ────────────────────────────────────────────────────────────────

    async fn request_user_confirmation(&self, skill_name: &str, action: &Action) -> Result<bool> {
        let request_id = Uuid::new_v4().to_string();
        let payload = PermissionRequestPayload {
            request_id: request_id.clone(),
            skill_name: skill_name.to_string(),
            action_description: action_description(action),
            risk_level: if action.is_dangerous() { "high" } else { "low" }.to_string(),
            details: PermissionDetails {
                affected_files: match action {
                    Action::DeleteFile(p) => Some(vec![p.clone()]),
                    _ => None,
                },
                network_destinations: None,
                estimated_cost: match action {
                    Action::MakePayment(amt) => Some(*amt),
                    _ => None,
                },
            },
        };

        let interactive = self
            .interactive_ui
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(ui) = interactive {
            return ui.request_permission(payload).await;
        }

        let Some(sink) = self.event_sink.get() else {
            return Ok(false);
        };

        let (tx, rx) = oneshot::channel::<bool>();
        self.pending.lock().unwrap().insert(request_id.clone(), tx);

        emit_event(sink.as_ref(), "permission_request", payload);

        // Wait up to 5 minutes for the user to click Allow or Deny
        match timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(approved)) => Ok(approved),
            _ => {
                self.pending.lock().unwrap().remove(&request_id);
                Ok(false) // timeout or channel drop → deny
            }
        }
    }
}

fn action_description(action: &Action) -> String {
    match action {
        Action::DeleteFile(path) => format!("删除文件：{path}"),
        Action::SendPublicMessage(msg) => format!("发送公开消息：{msg}"),
        Action::MakePayment(amount) => format!("发起支付：${amount:.2}"),
        Action::ModifySystemConfig(key) => format!("修改系统配置：{key}"),
        Action::Other(desc) => desc.clone(),
    }
}
