use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::conversation::types::{
    AgentChatContent, AgentChatConversationRef, AgentChatEnvelope, AgentChatSender,
    AgentChatSenderActor, AgentChatSenderClient, AgentChatSenderTransport, AgentChatSenderVia,
    ConversationMessageRecord, ConversationSpaceRecord,
};
use crate::memory::system::MemorySystem;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmtpHelperEvent {
    pub event: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct XmtpHelperConfig {
    pub node_bin: PathBuf,
    pub script_path: PathBuf,
}

impl XmtpHelperConfig {
    pub fn local_dev(repo_root: impl AsRef<Path>) -> Self {
        Self {
            node_bin: PathBuf::from("node"),
            script_path: repo_root.as_ref().join("xmtp-helper/dist/index.js"),
        }
    }
}

#[derive(Debug, Serialize)]
struct HelperRequest {
    id: String,
    method: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct HelperResponse {
    id: Option<String>,
    result: Option<Value>,
    error: Option<HelperError>,
    event: Option<String>,
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct HelperError {
    code: String,
    message: String,
}

pub struct XmtpNodeHelper {
    config: XmtpHelperConfig,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    response_rx: Mutex<Option<mpsc::Receiver<HelperResponse>>>,
    event_tx: broadcast::Sender<XmtpHelperEvent>,
    next_id: AtomicU64,
}

impl XmtpNodeHelper {
    pub fn new(config: XmtpHelperConfig) -> Self {
        Self {
            config,
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            response_rx: Mutex::new(None),
            event_tx: broadcast::channel(256).0,
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if self.child.lock().await.is_some() {
            return Ok(());
        }

        let mut child = Command::new(&self.config.node_bin)
            .arg(&self.config.script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "start XMTP helper {}",
                    self.config.script_path.to_string_lossy()
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("XMTP helper stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("XMTP helper stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!("[xmtp-helper] {line}");
                }
            });
        }

        let (response_tx, response_rx) = mpsc::channel(128);
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let parsed = match serde_json::from_str::<HelperResponse>(&line) {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        tracing::warn!("[xmtp-helper] invalid output: {e}: {line}");
                        continue;
                    }
                };
                if let Some(event) = parsed.event.clone() {
                    let _ = event_tx.send(XmtpHelperEvent {
                        event,
                        payload: parsed.payload.clone().unwrap_or(Value::Null),
                    });
                    continue;
                }
                if response_tx.send(parsed).await.is_err() {
                    break;
                }
            }
        });

        *self.stdin.lock().await = Some(stdin);
        *self.response_rx.lock().await = Some(response_rx);
        *self.child.lock().await = Some(child);
        Ok(())
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.start().await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let request = HelperRequest {
            id: id.clone(),
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&request)? + "\n";
        {
            let mut stdin = self.stdin.lock().await;
            let stdin = stdin
                .as_mut()
                .ok_or_else(|| anyhow!("XMTP helper stdin is closed"))?;
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
        }

        loop {
            let response = {
                let mut rx_guard = self.response_rx.lock().await;
                let rx = rx_guard
                    .as_mut()
                    .ok_or_else(|| anyhow!("XMTP helper receiver is closed"))?;
                tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv())
                    .await
                    .context("XMTP helper request timed out")?
                    .ok_or_else(|| anyhow!("XMTP helper exited"))?
            };

            if response.id.as_deref() != Some(id.as_str()) {
                continue;
            }
            if let Some(error) = response.error {
                return Err(anyhow!("{}: {}", error.code, error.message));
            }
            return Ok(response.result.unwrap_or(Value::Null));
        }
    }

    pub async fn next_event(&self) -> Result<XmtpHelperEvent> {
        self.start().await?;
        let mut rx = self.event_tx.subscribe();
        rx.recv()
            .await
            .map_err(|e| anyhow!("XMTP helper event stream closed: {e}"))
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<XmtpHelperEvent> {
        self.event_tx.subscribe()
    }

    pub async fn init_default(&self, workspace_root: &Path) -> Result<Value> {
        let xmtp = crate::config::ensure_xmtp_config()?;
        self.request(
            "init",
            json!({
                "env": "dev",
                "dataDir": workspace_root.join("xmtp"),
                "identityPrivateKey": xmtp.identity_private_key,
                "dbEncryptionKey": xmtp.db_encryption_key
            }),
        )
        .await
    }

    pub async fn publish_message(
        &self,
        memory: &MemorySystem,
        workspace_root: &Path,
        space: &ConversationSpaceRecord,
        message: &ConversationMessageRecord,
    ) -> Result<Vec<String>> {
        let transports: Vec<_> = space
            .transports
            .iter()
            .filter(|transport| transport.kind == "xmtp" && transport.status == "active")
            .filter_map(|transport| transport.transport_ref.as_deref())
            .collect();
        if transports.is_empty() {
            return Ok(Vec::new());
        }

        let status = self.init_default(workspace_root).await?;
        let inbox_id = status
            .get("inboxId")
            .and_then(|value| value.as_str())
            .unwrap_or("openpup-local");
        let short = short_inbox_id(inbox_id);
        let client_display_name = format!("OpenPup {short}");
        let client_instance_id = format!("openpup:{short}");

        let mut remote_ids = Vec::new();
        for transport_ref in transports {
            let is_agent = message.sender_kind == "agent";
            let agent_key = if message.sender_identity_id == "agent_alpha" {
                Some("alpha".to_string())
            } else {
                None
            };
            let actor_id = agent_key
                .clone()
                .unwrap_or_else(|| if is_agent { "agent".to_string() } else { "owner".to_string() });
            let actor_display_name = agent_key
                .as_deref()
                .map(agent_display_name)
                .unwrap_or_else(|| if is_agent { "Agent".to_string() } else { "Owner".to_string() });
            let via = outbound_via(message, &actor_display_name);
            let envelope = AgentChatEnvelope {
                kind: "agent.chat.message.v1".to_string(),
                protocol: "agent-conversation-v1".to_string(),
                conversation: AgentChatConversationRef {
                    transport: "xmtp".to_string(),
                    transport_ref: transport_ref.to_string(),
                    local_hint: Some(space.id.clone()),
                },
                message_id: message.id.clone(),
                sender: AgentChatSender {
                    transport: AgentChatSenderTransport {
                        network: "xmtp".to_string(),
                        inbox_id: inbox_id.to_string(),
                    },
                    client: AgentChatSenderClient {
                        kind: "openpup".to_string(),
                        instance_id: client_instance_id.clone(),
                        display_name: client_display_name.clone(),
                        version: Some(env!("CARGO_PKG_VERSION").to_string()),
                    },
                    actor: AgentChatSenderActor {
                        kind: if is_agent { "agent" } else { "human" }.to_string(),
                        actor_id,
                        display_name: actor_display_name,
                        agent_key,
                    },
                    via: Some(via),
                },
                content: AgentChatContent {
                    content_type: "text/plain".to_string(),
                    text: message.content.clone(),
                },
                created_at: message.created_at,
            };

            let result = self
                .request(
                    "sendMessage",
                    json!({
                        "transportRef": transport_ref,
                        "envelope": envelope,
                    }),
                )
                .await?;
            let remote_message_id = result
                .get("remoteMessageId")
                .and_then(|value| value.as_str())
                .unwrap_or(&message.id)
                .to_string();
            memory
                .insert_xmtp_message_map(
                    &message.id,
                    &remote_message_id,
                    transport_ref,
                    "outbound",
                )
                .await?;
            remote_ids.push(remote_message_id);
        }
        Ok(remote_ids)
    }
}

fn short_inbox_id(inbox_id: &str) -> String {
    inbox_id.chars().take(8).collect::<String>().to_ascii_lowercase()
}

fn agent_display_name(agent_key: &str) -> String {
    match agent_key {
        "alpha" => "Alpha".to_string(),
        other => other.to_string(),
    }
}

fn outbound_via(
    message: &ConversationMessageRecord,
    actor_display_name: &str,
) -> AgentChatSenderVia {
    if message.sender_kind == "agent" {
        AgentChatSenderVia {
            kind: "agent".to_string(),
            label: actor_display_name.to_string(),
            external_user_ref: None,
        }
    } else if message.sender_identity_id.starts_with("bridge:qqbot:") {
        AgentChatSenderVia {
            kind: "bridge.qq".to_string(),
            label: "QQ".to_string(),
            external_user_ref: None,
        }
    } else if message.sender_identity_id.starts_with("bridge:weixin:") {
        AgentChatSenderVia {
            kind: "bridge.weixin".to_string(),
            label: "Weixin".to_string(),
            external_user_ref: None,
        }
    } else if message.sender_identity_id.starts_with("bridge:telegram:") {
        AgentChatSenderVia {
            kind: "bridge.telegram".to_string(),
            label: "Telegram".to_string(),
            external_user_ref: None,
        }
    } else {
        AgentChatSenderVia {
            kind: "desktop".to_string(),
            label: "Desktop".to_string(),
            external_user_ref: None,
        }
    }
}
