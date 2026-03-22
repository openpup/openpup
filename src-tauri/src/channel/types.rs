use serde::{Deserialize, Serialize};

/// Full channel record as returned by DB queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub id: String,
    pub task_id: String,
    pub title: String,
    /// "active" | "completed" | "archived"
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
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
    /// "text" | "status"
    pub msg_type: String,
    pub artifact_name: Option<String>,
    /// "started" | "done" | "blocked"
    pub status_val: Option<String>,
    pub mentions: Vec<String>,
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
    /// "text" | "status"
    pub msg_type: String,
    pub artifact_name: Option<String>,
    /// "started" | "done" | "blocked" (only when msg_type == "status")
    pub status_val: Option<String>,
    pub mentions: Vec<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelCompletedPayload {
    pub channel_id: String,
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
