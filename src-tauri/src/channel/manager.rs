use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use chrono::Utc;
use tauri::Emitter;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::channel::heartbeat::HeartbeatMonitor;
use crate::channel::types::{ChannelCompletedPayload, ChannelMessagePayload};
use crate::memory::system::MemorySystem;

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
    /// Lazily initialised after Tauri setup.
    app_handle: Arc<OnceLock<tauri::AppHandle>>,
    /// Broadcast senders keyed by channel_id. Removed when channel is completed.
    senders: Arc<RwLock<HashMap<String, broadcast::Sender<ChannelMessagePayload>>>>,
    pub monitor: Arc<HeartbeatMonitor>,
}

impl ChannelManager {
    pub fn new(memory: Arc<MemorySystem>) -> Self {
        Self {
            memory,
            app_handle: Arc::new(OnceLock::new()),
            senders: Arc::new(RwLock::new(HashMap::new())),
            monitor: Arc::new(HeartbeatMonitor::new()),
        }
    }

    /// Initialise the AppHandle. Call this once from Tauri's `setup` closure.
    pub fn init_handle(&self, handle: tauri::AppHandle) {
        let _ = self.app_handle.set(handle);
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
        let msg_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        self.memory
            .post_channel_message(
                &msg_id,
                channel_id,
                sender,
                content,
                "text",
                None,
                None,
                mentions,
            )
            .await?;

        let payload = ChannelMessagePayload {
            channel_id: channel_id.to_string(),
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: content.to_string(),
            msg_type: "text".to_string(),
            artifact_name: None,
            status_val: None,
            mentions: mentions.iter().map(|s| s.to_string()).collect(),
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
        if let Some(app) = self.app_handle.get() {
            let _ = app.emit("channel_message", payload);
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
        let msg_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        self.memory
            .post_channel_message(
                &msg_id,
                channel_id,
                sender,
                "", // no content for status messages
                "status",
                None,
                Some(status_val),
                &[],
            )
            .await?;

        let payload = ChannelMessagePayload {
            channel_id: channel_id.to_string(),
            id: msg_id.clone(),
            sender: sender.to_string(),
            content: String::new(),
            msg_type: "status".to_string(),
            artifact_name: None,
            status_val: Some(status_val.to_string()),
            mentions: vec![],
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
        if let Some(app) = self.app_handle.get() {
            let _ = app.emit("channel_message", payload);
        }

        Ok(msg_id)
    }

    /// Record a heartbeat for a pup — ONLY updates the monitor, does not persist or emit.
    pub async fn post_heartbeat(&self, channel_id: &str, sender: &str) {
        self.monitor.beat(channel_id, sender).await;
    }

    /// Complete a channel: persist completion, emit event, clean up state.
    pub async fn complete(&self, channel_id: &str) -> Result<()> {
        self.memory.complete_channel(channel_id).await?;

        if let Some(app) = self.app_handle.get() {
            let _ = app.emit(
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
