use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use tokio::sync::{broadcast, oneshot, Mutex, RwLock};
use uuid::Uuid;

use crate::channel::heartbeat::HeartbeatMonitor;
use crate::channel::types::{ChannelCompletedPayload, ChannelMessagePayload, ChannelWorkflowState};
use crate::memory::system::MemorySystem;
use crate::runtime::{emit_event, SharedEventSink};

#[derive(Debug)]
pub enum ReviewDecision {
    Continue,
    RequestChanges {
        sender: String,
        comment: String,
        reply_to: Option<String>,
    },
}

struct PendingReview {
    responder: oneshot::Sender<ReviewDecision>,
}

/// Manages pack channels: creation, messaging, heartbeat tracking, and completion.
///
/// The AppHandle is initialised lazily via `init_handle()` (called from Tauri's
/// `setup` closure), matching the same pattern used by `PermissionChecker`.
///
/// Each active channel has an associated broadcast::Sender so internal tasks
/// can subscribe to channel messages without going through the DB.
#[derive(Clone)]
pub struct ChannelManager {
    memory: Arc<MemorySystem>,
    /// Optional event sink used by desktop runtimes.
    event_sink: Arc<OnceLock<SharedEventSink>>,
    /// Broadcast senders keyed by channel_id. Removed when channel is completed.
    senders: Arc<RwLock<HashMap<String, broadcast::Sender<ChannelMessagePayload>>>>,
    review_sessions: Arc<Mutex<HashMap<String, PendingReview>>>,
    pub monitor: Arc<HeartbeatMonitor>,
}

impl ChannelManager {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self {
            memory,
            event_sink: Arc::new(OnceLock::new()),
            senders: Arc::new(RwLock::new(HashMap::new())),
            review_sessions: Arc::new(Mutex::new(HashMap::new())),
            monitor: Arc::new(HeartbeatMonitor::new()),
        }
    }

    pub fn set_event_sink(&self, sink: SharedEventSink) {
        let _ = self.event_sink.set(sink);
    }

    /// Create a new channel, register a broadcast sender, and return the channel id.
    pub async fn create_channel(
        &self,
        task_id: &str,
        title: &str,
        members: &[&str],
    ) -> Result<String> {
        let channel_id = Uuid::new_v4().to_string();
        self.memory
            .create_channel(&channel_id, task_id, title, members)
            .await?;

        // Set up broadcast channel for this channel_id
        let (tx, _) = broadcast::channel(128);
        {
            let mut guard = self.senders.write().await;
            guard.insert(channel_id.clone(), tx);
        }

        Ok(channel_id)
    }

    /// Post a text message to a channel, persist it, and emit a Tauri event.
    /// Returns the message id.
    pub async fn post_text(
        &self,
        channel_id: &str,
        sender: &str,
        content: &str,
        mentions: &[&str],
    ) -> Result<String> {
        self.post_message(
            channel_id, sender, content, "text", None, None, mentions, None, None,
        )
        .await
    }

    pub async fn post_message(
        &self,
        channel_id: &str,
        sender: &str,
        content: &str,
        msg_type: &str,
        artifact_name: Option<&str>,
        status_val: Option<&str>,
        mentions: &[&str],
        reply_to: Option<&str>,
        event_payload: Option<Value>,
    ) -> Result<String> {
        let msg_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        self.memory
            .post_channel_message(
                &msg_id,
                channel_id,
                sender,
                content,
                msg_type,
                artifact_name,
                status_val,
                mentions,
                reply_to,
                event_payload.as_ref(),
            )
            .await?;

        let payload = ChannelMessagePayload {
            channel_id: channel_id.to_string(),
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: content.to_string(),
            msg_type: msg_type.to_string(),
            artifact_name: artifact_name.map(|value| value.to_string()),
            status_val: status_val.map(|value| value.to_string()),
            mentions: mentions.iter().map(|s| s.to_string()).collect(),
            reply_to: reply_to.map(|value| value.to_string()),
            event_payload,
            timestamp: now,
        };

        // Broadcast internally
        {
            let guard = self.senders.read().await;
            if let Some(tx) = guard.get(channel_id) {
                let _ = tx.send(payload.clone());
            }
        }

        // Emit Tauri event if handle is available
        if let Some(sink) = self.event_sink.get() {
            emit_event(sink.as_ref(), "channel_message", payload);
        }

        Ok(msg_id)
    }

    /// Post a status message to a channel (e.g., "started", "done", "failed").
    /// Returns the message id.
    pub async fn post_status(
        &self,
        channel_id: &str,
        sender: &str,
        status_val: &str,
    ) -> Result<String> {
        self.post_message(
            channel_id,
            sender,
            "",
            "status",
            None,
            Some(status_val),
            &[],
            None,
            None,
        )
        .await
    }

    pub async fn post_activity(
        &self,
        channel_id: &str,
        sender: &str,
        content: &str,
    ) -> Result<String> {
        self.post_message(
            channel_id,
            sender,
            content,
            "activity",
            None,
            None,
            &[],
            None,
            None,
        )
        .await
    }

    pub async fn update_workflow_state(
        &self,
        channel_id: &str,
        status: &str,
        current_layer: Option<i64>,
        review_round: i64,
        awaiting_user: bool,
        blocked_reason: Option<&str>,
    ) -> Result<()> {
        self.memory
            .set_channel_workflow_state(
                channel_id,
                status,
                current_layer,
                review_round,
                awaiting_user,
                blocked_reason,
            )
            .await?;

        if let Some(sink) = self.event_sink.get() {
            if let Some(state) = self.memory.get_channel_workflow_state(channel_id).await? {
                emit_event(sink.as_ref(), "channel_workflow_updated", state);
            }
        }

        Ok(())
    }

    pub async fn begin_review(
        &self,
        channel_id: &str,
        layer_index: usize,
        review_round: i64,
        message: &str,
    ) -> Result<oneshot::Receiver<ReviewDecision>> {
        self.update_workflow_state(
            channel_id,
            "awaiting_review",
            Some(layer_index as i64),
            review_round,
            true,
            Some("awaiting_review"),
        )
        .await?;
        self.post_message(
            channel_id,
            "alpha",
            message,
            "review_request",
            None,
            None,
            &[],
            None,
            None,
        )
        .await?;

        let (tx, rx) = oneshot::channel();
        let mut sessions = self.review_sessions.lock().await;
        sessions.insert(channel_id.to_string(), PendingReview { responder: tx });
        Ok(rx)
    }

    pub async fn submit_review_comment(
        &self,
        channel_id: &str,
        sender: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<String> {
        self.post_message(
            channel_id,
            sender,
            content,
            "review_comment",
            None,
            None,
            &[],
            reply_to,
            None,
        )
        .await
    }

    pub async fn request_changes(
        &self,
        channel_id: &str,
        sender: &str,
        comment: &str,
        reply_to: Option<&str>,
        event_payload: Option<Value>,
    ) -> Result<()> {
        self.post_message(
            channel_id,
            sender,
            comment,
            "review_decision",
            None,
            None,
            &[],
            reply_to,
            event_payload,
        )
        .await?;

        let mut sessions = self.review_sessions.lock().await;
        let pending = sessions
            .remove(channel_id)
            .ok_or_else(|| anyhow!("channel is not waiting for review"))?;
        let _ = pending.responder.send(ReviewDecision::RequestChanges {
            sender: sender.to_string(),
            comment: comment.to_string(),
            reply_to: reply_to.map(|value| value.to_string()),
        });
        Ok(())
    }

    pub async fn continue_channel(
        &self,
        channel_id: &str,
        sender: &str,
        comment: &str,
    ) -> Result<()> {
        self.post_message(
            channel_id,
            sender,
            comment,
            "review_resume",
            None,
            None,
            &[],
            None,
            None,
        )
        .await?;

        let mut sessions = self.review_sessions.lock().await;
        let pending = sessions
            .remove(channel_id)
            .ok_or_else(|| anyhow!("channel is not waiting for review"))?;
        let _ = pending.responder.send(ReviewDecision::Continue);
        Ok(())
    }

    pub async fn workflow_state(&self, channel_id: &str) -> Result<Option<ChannelWorkflowState>> {
        self.memory.get_channel_workflow_state(channel_id).await
    }

    pub async fn clear_review_session(&self, channel_id: &str) {
        let mut sessions = self.review_sessions.lock().await;
        sessions.remove(channel_id);
    }

    /// Record a heartbeat for a pup — ONLY updates the monitor, does not persist or emit.
    pub async fn post_heartbeat(&self, channel_id: &str, sender: &str) {
        self.monitor.beat(channel_id, sender).await;
    }

    /// Complete a channel: persist completion, emit event, clean up state.
    pub async fn complete(&self, channel_id: &str) -> Result<()> {
        self.clear_review_session(channel_id).await;
        self.memory.complete_channel(channel_id).await?;

        if let Some(sink) = self.event_sink.get() {
            emit_event(
                sink.as_ref(),
                "channel_completed",
                ChannelCompletedPayload {
                    channel_id: channel_id.to_string(),
                },
            );
        }

        // Remove broadcast sender
        {
            let mut guard = self.senders.write().await;
            guard.remove(channel_id);
        }

        // Clean up heartbeat state
        self.monitor.cleanup_channel(channel_id).await;

        Ok(())
    }

    /// Return the number of currently active channels.
    pub async fn active_count(&self) -> Result<i64> {
        self.memory.active_channel_count().await
    }
}
