use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
pub const MAX_EXECUTION: Duration = Duration::from_secs(600);

#[derive(Debug)]
pub enum TimeoutKind {
    Idle,
    MaxExecution,
}

#[derive(Debug)]
struct PupHeartbeat {
    /// Time of the last heartbeat (or registration).
    last_beat: Instant,
    /// Time the pup was registered (execution start).
    started_at: Instant,
}

/// Tracks heartbeats for pups participating in pack channels.
///
/// Structure: channel_id → pup_id → PupHeartbeat
#[derive(Clone)]
pub struct HeartbeatMonitor {
    state: Arc<RwLock<HashMap<String, HashMap<String, PupHeartbeat>>>>,
}

impl HeartbeatMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pup as active in a channel.
    pub async fn register(&self, channel_id: &str, pup_id: &str) {
        let mut guard = self.state.write().await;
        let channel_map = guard
            .entry(channel_id.to_string())
            .or_insert_with(HashMap::new);
        let now = Instant::now();
        channel_map.insert(
            pup_id.to_string(),
            PupHeartbeat {
                last_beat: now,
                started_at: now,
            },
        );
    }

    /// Record a heartbeat for a pup in a channel.
    pub async fn beat(&self, channel_id: &str, pup_id: &str) {
        let mut guard = self.state.write().await;
        if let Some(channel_map) = guard.get_mut(channel_id) {
            if let Some(hb) = channel_map.get_mut(pup_id) {
                hb.last_beat = Instant::now();
            }
        }
    }

    /// Unregister a pup from a channel (called when the pup finishes).
    pub async fn unregister(&self, channel_id: &str, pup_id: &str) {
        let mut guard = self.state.write().await;
        if let Some(channel_map) = guard.get_mut(channel_id) {
            channel_map.remove(pup_id);
        }
    }

    /// Check for timed-out pups in a channel.
    ///
    /// Returns a list of (pup_id, TimeoutKind) for pups that have exceeded
    /// IDLE_TIMEOUT since their last beat or MAX_EXECUTION since registration.
    pub async fn check_timeouts(&self, channel_id: &str) -> Vec<(String, TimeoutKind)> {
        let guard = self.state.read().await;
        let Some(channel_map) = guard.get(channel_id) else {
            return vec![];
        };
        let now = Instant::now();
        let mut timed_out = Vec::new();
        for (pup_id, hb) in channel_map {
            if now.duration_since(hb.started_at) >= MAX_EXECUTION {
                timed_out.push((pup_id.clone(), TimeoutKind::MaxExecution));
            } else if now.duration_since(hb.last_beat) >= IDLE_TIMEOUT {
                timed_out.push((pup_id.clone(), TimeoutKind::Idle));
            }
        }
        timed_out
    }

    /// Remove all heartbeat state for a channel (called when channel completes).
    pub async fn cleanup_channel(&self, channel_id: &str) {
        let mut guard = self.state.write().await;
        guard.remove(channel_id);
    }
}

impl Default for HeartbeatMonitor {
    fn default() -> Self {
        Self::new()
    }
}
