use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::memory::system::MemorySystem;
use crate::policy::{
    baseline_policy_decision, summarize_json, EffectKind, PolicyActor, PolicyAuditRecord,
    PolicyDecision, PolicyDetails, PolicyGrant, PolicyRequest, PolicyRisk,
};
use crate::runtime::{emit_event, SharedEventSink};

pub use crate::policy::ExecutionMode;

// ── Payloads ─────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct PermissionDetails {
    pub affected_files: Option<Vec<String>>,
    pub network_destinations: Option<Vec<String>>,
    pub estimated_cost: Option<f64>,
}

impl From<&PolicyDetails> for PermissionDetails {
    fn from(details: &PolicyDetails) -> Self {
        Self {
            affected_files: (!details.affected_files.is_empty())
                .then(|| details.affected_files.clone()),
            network_destinations: (!details.network_destinations.is_empty())
                .then(|| details.network_destinations.clone()),
            estimated_cost: details.estimated_cost,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct PermissionRequestPayload {
    /// Unique ID matched when the frontend calls `approve_permission` / `deny_permission`.
    pub request_id: String,
    /// Human-readable actor label used by the approval UI.
    pub skill_name: String,
    pub action_description: String,
    pub risk_level: String,
    pub details: PermissionDetails,
    pub actor_kind: String,
    pub actor_name: String,
    pub tool_name: String,
    pub effect_kind: String,
    pub scope: Value,
}

#[async_trait]
pub trait PermissionUi: Send + Sync {
    async fn request_permission(&self, payload: PermissionRequestPayload) -> Result<bool>;
}

struct PendingPermission {
    tx: oneshot::Sender<bool>,
    request: PolicyRequest,
}

// ── PermissionChecker / Approval broker ──────────────────────────────────────

#[derive(Clone)]
pub struct PermissionChecker {
    mode: Arc<RwLock<ExecutionMode>>,
    /// Optional event sink for GUI-style permission requests.
    event_sink: Arc<OnceLock<SharedEventSink>>,
    /// Optional interactive permission UI for headless runtimes.
    interactive_ui: Arc<Mutex<Option<Arc<dyn PermissionUi>>>>,
    /// In-flight requests awaiting user response.
    pending: Arc<Mutex<HashMap<String, PendingPermission>>>,
    /// Persistence for grants and audit logs.
    memory: Arc<Mutex<Option<Arc<MemorySystem>>>>,
}

impl PermissionChecker {
    pub fn new() -> Self {
        Self {
            mode: Arc::new(RwLock::new(ExecutionMode::Leashed)),
            event_sink: Arc::new(OnceLock::new()),
            interactive_ui: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            memory: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_policy_memory(&self, memory: Arc<MemorySystem>) {
        if let Ok(mut guard) = self.memory.lock() {
            *guard = Some(memory);
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

    // ── Public API ───────────────────────────────────────────────────────────

    pub async fn get_mode(&self) -> ExecutionMode {
        *self.mode.read().await
    }

    pub async fn set_mode(&self, mode: ExecutionMode) {
        *self.mode.write().await = mode;
    }

    pub async fn authorize(&self, request: PolicyRequest) -> Result<bool> {
        let mode = self.get_mode().await;
        match baseline_policy_decision(mode, &request) {
            PolicyDecision::Allow => {
                self.audit(&request, PolicyDecision::Allow, "allowed_by_policy")
                    .await;
                Ok(true)
            }
            PolicyDecision::Deny => {
                self.audit(&request, PolicyDecision::Deny, "denied_by_policy")
                    .await;
                Ok(false)
            }
            PolicyDecision::Ask => {
                if self.has_matching_grant(&request, mode).await {
                    self.audit(&request, PolicyDecision::Allow, "allowed_by_grant")
                        .await;
                    return Ok(true);
                }

                let approved = self.request_policy_confirmation(request.clone()).await?;
                self.audit(
                    &request,
                    if approved {
                        PolicyDecision::Allow
                    } else {
                        PolicyDecision::Deny
                    },
                    if approved {
                        "approved_once"
                    } else {
                        "denied_or_timeout"
                    },
                )
                .await;
                Ok(approved)
            }
        }
    }

    pub async fn authorize_skill_activation(
        &self,
        actor: PolicyActor,
        skill_name: &str,
        dangerous: bool,
    ) -> Result<bool> {
        if !dangerous {
            return Ok(true);
        }
        self.authorize(PolicyRequest {
            actor,
            tool_name: format!("skill:{skill_name}"),
            effect: EffectKind::SkillActivation,
            risk: PolicyRisk::Medium,
            scope: Default::default(),
            description: format!("Activate dangerous skill `{skill_name}`"),
            details: PolicyDetails::default(),
        })
        .await
    }

    pub async fn authorize_mcp_call(
        &self,
        actor: PolicyActor,
        server: &str,
        tool: &str,
        args: &Value,
    ) -> Result<bool> {
        let request = self.policy_request_for_mcp_call(actor, server, tool, args);
        self.authorize(request).await
    }

    pub fn policy_request_for_mcp_call(
        &self,
        actor: PolicyActor,
        server: &str,
        tool: &str,
        args: &Value,
    ) -> PolicyRequest {
        let (effect, risk) = classify_mcp_tool(tool);
        PolicyRequest {
            actor,
            tool_name: format!("mcp::{server}::{tool}"),
            effect,
            risk,
            scope: crate::policy::PolicyScope {
                mcp_server: Some(server.to_string()),
                mcp_tool: Some(tool.to_string()),
                ..Default::default()
            },
            description: format!(
                "Call MCP tool `{server}/{tool}` with {}",
                summarize_json(args, 240)
            ),
            details: PolicyDetails::default(),
        }
    }

    pub async fn denial_diagnostics(&self, prefix: &str, request: &PolicyRequest) -> String {
        let mode = self.get_mode().await;
        let baseline = baseline_policy_decision(mode, request);
        let reason = match baseline {
            PolicyDecision::Allow => "authorization returned false despite baseline allow",
            PolicyDecision::Ask => {
                "no matching remembered grant, or confirmation was denied/timed out"
            }
            PolicyDecision::Deny => "blocked by baseline policy",
        };
        format!(
            "{prefix}: actor={} source={} mode={} tool={} effect={} risk={} baseline_decision={} reason={} description={}",
            request.actor.label(),
            request.actor.source.as_str(),
            mode.as_str(),
            request.tool_name,
            request.effect.as_str(),
            request.risk.as_str(),
            baseline.as_str(),
            reason,
            request.description,
        )
    }

    pub async fn authorize_boundary_access(
        &self,
        actor: PolicyActor,
        tool_name: &str,
        path: &str,
    ) -> Result<bool> {
        self.authorize(PolicyRequest {
            actor,
            tool_name: format!("boundary:{tool_name}"),
            effect: EffectKind::WriteOutsideWorkspace,
            risk: PolicyRisk::High,
            scope: crate::policy::PolicyScope {
                path: Some(path.to_string()),
                ..Default::default()
            },
            description: format!("Access local path outside allowed roots: {path}"),
            details: PolicyDetails {
                affected_files: vec![path.to_string()],
                ..Default::default()
            },
        })
        .await
    }

    pub async fn approve(&self, request_id: &str, remember: bool) {
        let pending = self.pending.lock().unwrap().remove(request_id);
        if let Some(pending) = pending {
            if remember && pending.request.risk != PolicyRisk::High {
                self.persist_grant_for_request(&pending.request).await;
            }
            let _ = pending.tx.send(true);
        }
    }

    pub fn deny(&self, request_id: &str) {
        if let Some(pending) = self.pending.lock().unwrap().remove(request_id) {
            let _ = pending.tx.send(false);
        }
    }

    async fn has_matching_grant(&self, request: &PolicyRequest, mode: ExecutionMode) -> bool {
        let memory = self.memory.lock().ok().and_then(|guard| guard.clone());
        let Some(memory) = memory else {
            return false;
        };
        memory
            .matching_policy_grants(request, mode)
            .await
            .map(|grants| !grants.is_empty())
            .unwrap_or(false)
    }

    async fn persist_grant_for_request(&self, request: &PolicyRequest) {
        let memory = self.memory.lock().ok().and_then(|guard| guard.clone());
        let Some(memory) = memory else {
            return;
        };
        let now = Utc::now().timestamp();
        let mode = self.get_mode().await;
        let grant = PolicyGrant {
            id: Uuid::new_v4().to_string(),
            actor_kind: request.actor.kind,
            actor_name: request.actor.name.clone(),
            effect_kind: request.effect,
            tool_name: Some(request.tool_name.clone()),
            scope: request.grant_scope(),
            risk_ceiling: request.risk,
            mode: mode.as_str().to_string(),
            expires_at: None,
            created_at: now,
            created_by: request.actor.source,
        };
        let _ = memory.insert_policy_grant(&grant).await;
    }

    async fn audit(&self, request: &PolicyRequest, decision: PolicyDecision, result_status: &str) {
        let memory = self.memory.lock().ok().and_then(|guard| guard.clone());
        let Some(memory) = memory else {
            return;
        };
        let mode = self.get_mode().await;
        let record = PolicyAuditRecord {
            id: Uuid::new_v4().to_string(),
            actor_kind: request.actor.kind,
            actor_name: request.actor.name.clone(),
            source: request.actor.source,
            tool_name: request.tool_name.clone(),
            effect_kind: request.effect,
            risk: request.risk,
            decision,
            mode,
            args_summary: request.description.clone(),
            result_status: result_status.to_string(),
            created_at: Utc::now().timestamp(),
        };
        let _ = memory.record_policy_audit(&record).await;
    }

    async fn request_policy_confirmation(&self, request: PolicyRequest) -> Result<bool> {
        let request_id = Uuid::new_v4().to_string();
        let payload = PermissionRequestPayload {
            request_id: request_id.clone(),
            skill_name: request.actor.label(),
            action_description: request.description.clone(),
            risk_level: request.risk.as_str().to_string(),
            details: PermissionDetails::from(&request.details),
            actor_kind: request.actor.kind.as_str().to_string(),
            actor_name: request.actor.name.clone(),
            tool_name: request.tool_name.clone(),
            effect_kind: request.effect.as_str().to_string(),
            scope: serde_json::to_value(&request.scope).unwrap_or(Value::Null),
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
        self.pending.lock().unwrap().insert(
            request_id.clone(),
            PendingPermission {
                tx,
                request: request.clone(),
            },
        );

        emit_event(sink.as_ref(), "permission_request", payload);

        match timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(approved)) => Ok(approved),
            _ => {
                self.pending.lock().unwrap().remove(&request_id);
                Ok(false)
            }
        }
    }
}

impl Default for PermissionChecker {
    fn default() -> Self {
        Self::new()
    }
}

fn classify_mcp_tool(tool: &str) -> (EffectKind, PolicyRisk) {
    let lower = tool.to_ascii_lowercase();
    let read_only = [
        "get", "list", "read", "search", "fetch", "query", "find", "status", "show",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    if read_only {
        return (EffectKind::McpCall, PolicyRisk::Low);
    }
    let _side_effect = [
        "create", "update", "delete", "send", "post", "write", "patch", "merge", "close", "open",
        "run", "execute",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix) || lower.contains(&format!("_{prefix}")));
    (EffectKind::McpCall, PolicyRisk::Medium)
}
