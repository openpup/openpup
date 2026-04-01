use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Full channel record as returned by DB queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub id: String,
    pub task_id: String,
    pub title: String,
    /// "active" | "awaiting_review" | "completed" | "archived"
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
    pub current_layer: Option<i64>,
    pub review_round: i64,
    pub awaiting_user: bool,
    pub blocked_reason: Option<String>,
    /// Participating pup keys (from channel_members join).
    pub members: Vec<String>,
}

/// Single channel message as returned by DB queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessageRecord {
    pub id: String,
    pub channel_id: String,
    pub sender: String,
    pub content: String,
    /// "text" | "status" | "review_request" | "review_comment" | "review_decision" | "review_resume"
    pub msg_type: String,
    pub artifact_name: Option<String>,
    /// "started" | "done" | "blocked"
    pub status_val: Option<String>,
    pub mentions: Vec<String>,
    pub reply_to: Option<String>,
    pub event_payload: Option<Value>,
    pub timestamp: i64,
}

// ─── Tauri event payloads ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ChannelCreatedPayload {
    pub channel_id: String,
    pub title: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelMessagePayload {
    pub channel_id: String,
    pub id: String,
    pub sender: String,
    pub content: String,
    /// "text" | "status" | "review_request" | "review_comment" | "review_decision" | "review_resume"
    pub msg_type: String,
    pub artifact_name: Option<String>,
    /// "started" | "done" | "blocked" (only when msg_type == "status")
    pub status_val: Option<String>,
    pub mentions: Vec<String>,
    pub reply_to: Option<String>,
    pub event_payload: Option<Value>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelCompletedPayload {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelWorkflowState {
    pub channel_id: String,
    pub status: String,
    pub current_layer: Option<i64>,
    pub review_round: i64,
    pub awaiting_user: bool,
    pub blocked_reason: Option<String>,
}

// ─── DAG / Delegation plan types ──────────────────────────────────────────────

/// A single unit of work assigned to one pup, with optional dependencies on
/// other pups in the same plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub pup: String,
    pub description: String,
    pub depends_on: Vec<String>,
}

/// The full delegation plan emitted by Alpha before channel execution begins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationPlan {
    pub channel_id: String,
    pub channel_title: String,
    pub subtasks: Vec<Subtask>,
}
