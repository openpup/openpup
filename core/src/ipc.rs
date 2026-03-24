use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::bridge::types::{BridgeConfig, BridgeConnectionStatus};
use crate::bridge::weixin::{StoredWeixinAccount, WeixinQrStartResult, WeixinQrWaitResult};
use crate::channel::types::{
    ChannelMessageRecord, ChannelRecord, ChannelWorkflowState, DelegationPlan,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    Ping,
    Status,
    Ask {
        input: String,
        pup: Option<String>,
    },
    BridgeGetConfig,
    BridgeSaveConfig {
        config: BridgeConfig,
    },
    BridgeReload,
    BridgeStop,
    BridgeStatus,
    WeixinQrStart {
        base_url: String,
        proxy_url: Option<String>,
        route_tag: Option<String>,
        account_id: Option<String>,
        bot_type: Option<String>,
        force: bool,
    },
    WeixinQrWait {
        base_url: String,
        proxy_url: Option<String>,
        route_tag: Option<String>,
        session_key: String,
        bot_type: Option<String>,
        timeout_ms: Option<i64>,
    },
    WeixinQrCancel {
        session_key: String,
    },
    WeixinAccounts,
    WeixinActivate {
        account_id: String,
    },
    ChannelList {
        limit: i64,
    },
    ChannelShow {
        channel_id: String,
    },
    ChannelContinue {
        channel_id: String,
        sender: String,
        comment: String,
    },
    ChannelRequestChanges {
        channel_id: String,
        sender: String,
        comment: String,
        reply_to: Option<String>,
    },
    ChannelComment {
        channel_id: String,
        sender: String,
        comment: String,
        reply_to: Option<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub pid: u32,
    pub started_at: i64,
    pub workspace_root: String,
    pub socket_path: String,
    pub log_path: String,
    pub bridge_enabled: bool,
    pub scheduler_enabled: bool,
    pub memory_count: usize,
    pub active_task_count: usize,
    pub enabled_skill_count: usize,
    pub enabled_pup_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelDetails {
    pub channel: Option<ChannelRecord>,
    pub workflow: Option<ChannelWorkflowState>,
    pub plan: Option<DelegationPlan>,
    pub messages: Vec<ChannelMessageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum DaemonResponse {
    Pong,
    Status(DaemonStatus),
    AskCompleted { content: String },
    BridgeConfig(BridgeConfig),
    BridgeSaved(BridgeConfig),
    BridgeStatus(Vec<BridgeConnectionStatus>),
    WeixinQrStart(WeixinQrStartResult),
    WeixinQrWait(WeixinQrWaitResult),
    WeixinAccounts(Vec<StoredWeixinAccount>),
    WeixinActivated(BridgeConfig),
    ChannelList(Vec<ChannelRecord>),
    ChannelDetails(ChannelDetails),
    ChannelActionOk,
    Ack { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonEvent {
    Response { response: DaemonResponse },
    Token { token: String },
    Activity { label: String },
    Done { content: String },
    Error { message: String },
}

pub fn daemon_runtime_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openpup")
        .join("run")
}

pub fn daemon_socket_path() -> PathBuf {
    daemon_runtime_dir().join("openpupd.sock")
}

pub fn daemon_pid_path() -> PathBuf {
    daemon_runtime_dir().join("openpupd.pid")
}

pub fn daemon_log_path() -> PathBuf {
    daemon_runtime_dir().join("logs").join(format!(
        "openpupd.log.{}",
        chrono::Local::now().format("%Y-%m-%d")
    ))
}
