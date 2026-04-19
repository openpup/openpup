use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTransportRecord {
    pub kind: String,
    pub label: String,
    pub status: String,
    pub transport_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatConversationRef {
    pub transport: String,
    pub transport_ref: String,
    pub local_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatSender {
    pub transport: AgentChatSenderTransport,
    pub client: AgentChatSenderClient,
    pub actor: AgentChatSenderActor,
    pub via: Option<AgentChatSenderVia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatSenderTransport {
    pub network: String,
    pub inbox_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatSenderClient {
    pub kind: String,
    pub instance_id: String,
    pub display_name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatSenderActor {
    pub kind: String,
    pub actor_id: String,
    pub display_name: String,
    pub agent_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatSenderVia {
    pub kind: String,
    pub label: String,
    pub external_user_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatContent {
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentChatEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    pub protocol: String,
    pub conversation: AgentChatConversationRef,
    pub message_id: String,
    pub sender: AgentChatSender,
    pub content: AgentChatContent,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSpaceRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub owner_identity_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub accent: String,
    pub invite_code: String,
    pub routing_mode: String,
    pub member_count: i64,
    pub unread: i64,
    pub transports: Vec<ConversationTransportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMemberRecord {
    pub id: String,
    pub conversation_id: String,
    pub identity_id: String,
    pub display_name: String,
    pub mention_key: String,
    pub role: String,
    pub status: String,
    pub route_label: String,
    pub online: bool,
    pub accent: String,
    pub joined_at: i64,
    pub last_seen_at: Option<i64>,
    pub network_kind: Option<String>,
    pub transport_inbox_id: Option<String>,
    pub client_kind: Option<String>,
    pub client_instance_id: Option<String>,
    pub client_display_name: Option<String>,
    pub actor_kind: Option<String>,
    pub actor_id: Option<String>,
    pub actor_display_name: Option<String>,
    pub via_kind: Option<String>,
    pub via_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub sender_identity_id: String,
    pub sender_route_id: Option<String>,
    pub sender_name: String,
    pub sender_kind: String,
    pub route_label: Option<String>,
    pub content: String,
    pub created_at: i64,
    pub network_kind: Option<String>,
    pub transport_inbox_id: Option<String>,
    pub client_kind: Option<String>,
    pub client_instance_id: Option<String>,
    pub client_display_name: Option<String>,
    pub actor_kind: Option<String>,
    pub actor_id: Option<String>,
    pub actor_display_name: Option<String>,
    pub via_kind: Option<String>,
    pub via_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessageCreatedPayload {
    pub conversation_id: String,
    pub message: ConversationMessageRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMembersChangedPayload {
    pub conversation_id: String,
    pub members: Vec<ConversationMemberRecord>,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSpacesChangedPayload {
    pub spaces: Vec<ConversationSpaceRecord>,
}
