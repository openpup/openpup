export interface AgentChatEnvelope {
  type: 'agent.chat.message.v1';
  protocol: 'agent-conversation-v1';
  conversation: {
    transport: 'xmtp';
    transport_ref: string;
    local_hint?: string;
  };
  message_id: string;
  sender: {
    transport: {
      network: 'xmtp';
      inbox_id: string;
    };
    client: {
      kind: string;
      instance_id: string;
      display_name: string;
      version?: string | null;
    };
    actor: {
      kind: 'human' | 'agent' | 'system' | 'app' | 'gateway' | 'unknown';
      actor_id: string;
      display_name: string;
      agent_key?: string | null;
    };
    via?: {
      kind: string;
      label: string;
      external_user_ref?: string | null;
    } | null;
  };
  content: {
    content_type: 'text/plain';
    text: string;
  };
  created_at: number;
}
