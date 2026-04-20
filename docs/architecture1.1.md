# OpenPup Architecture 1.1

This document consolidates the group chat, context isolation, XMTP agent conversation, and XMTP Node helper plans into one architecture reference.

It supersedes these design notes:

- `docs/conversation-space-product-plan.md`
- `docs/context-scope-isolation.md`
- `docs/agent-conversation-over-xmtp.md`
- `docs/xmtp-node-helper-mvp3.md`

## Core Decision

OpenPup owns product semantics. Transports only deliver messages.

The product model is:

```text
Conversation Space
  - local semantic boundary
  - context scope
  - members
  - messages
  - Alpha participation
  - bridge routes
  - permissions
  - memories, tasks, and knowledge
        |
        v
Transport Binding
  - local
  - xmtp
  - relay
  - lan_p2p
  - bridge
        |
        v
Network or local delivery substrate
```

A group is a multi-member `ConversationSpace`. XMTP, relay, P2P, QQ, WeChat, and desktop UI routes are not the group itself. They are delivery paths into or out of a space.

## Goals

- Support personal chat, local groups, and networked groups without changing the core product model.
- Keep personal context, group context, and different group contexts isolated by default.
- Let bridge routes enter groups explicitly while keeping personal chat as the default.
- Use XMTP for encrypted remote group delivery and fanout without requiring OpenPup clients to expose public addresses.
- Allow non-OpenPup participants, such as OpenClaw or other agent runtimes, to join if they speak the agent conversation protocol.
- Avoid remote participants gaining local tool permissions by joining a group.
- Prevent multi-agent reply storms with explicit responder policy.
- Keep XMTP SDK and packaging risk behind a local Node/TS helper for MVP-3.

## Non-Goals

- OpenPup does not implement remote fanout when an XMTP binding is active. XMTP does.
- Remote agents do not get local shell, filesystem, memory, task, knowledge, or bridge permissions by default.
- The first XMTP implementation does not include attachments, reactions, edits, deletes, read receipts, push notifications, or complex trust delegation.
- The first XMTP implementation does not define a custom OpenPup relay.
- Plain XMTP text is not part of the MVP group membership protocol. MVP imports structured agent envelopes only.

## Product Model

The UI can expose three main areas:

- Personal: the owner's private chat and personal workspace.
- Groups: multi-member conversation spaces with their own messages, members, Alpha behavior, context, and transport bindings.
- Channels: structured pup collaboration runs.

Internally, groups are `ConversationSpace` records with `kind = group`.

### Core Tables

```text
conversation_spaces
- id
- kind                 // personal | group
- title
- description
- owner_identity_id
- created_at
- updated_at

conversation_members
- id
- conversation_id
- identity_id
- role                 // owner | admin | member | agent
- status               // active | invited | left | removed
- joined_at
- last_seen_at

conversation_messages
- id
- conversation_id
- sender_identity_id
- sender_route_id
- sender_kind          // human | agent | system
- content
- created_at

conversation_transport_bindings
- id
- conversation_id
- kind                 // local | xmtp | relay | lan_p2p | bridge
- label
- transport_ref
- status               // active | paused | failed
- created_at
- updated_at

identity_routes
- id
- identity_id
- route_kind           // app | bridge | xmtp | local_agent | remote_agent
- platform             // desktop | qq | weixin | xmtp | relay
- external_user_id
- external_chat_id
- enabled
- created_at
- updated_at
```

The local database remains the UI and context source of truth. Network state is projected into local conversation tables.

## Context Scope Isolation

All context-sensitive operations must carry a scope and an actor.

```rust
pub enum ContextScope {
    Personal {
        owner_id: String,
    },
    Group {
        group_id: String,
    },
}

pub struct ScopeActor {
    pub actor_id: String,
    pub actor_kind: ActorKind,
    pub role: Option<String>,
    pub source_route_id: Option<String>,
}

pub enum ActorKind {
    Owner,
    HumanMember,
    Agent,
    RemoteAgent,
    BridgeRoute,
}
```

The pair `(ContextScope, ScopeActor)` is the authorization and retrieval boundary for:

- Agent runs
- Conversation history
- Memory retrieval and extraction
- Task creation and execution
- Knowledge search
- Bridge routing
- Tool permission checks
- Scheduled notifications

### Scope Types

Global public context can be used in personal and group scopes. Examples include owner display name, language, public style preferences, and non-sensitive product settings.

Global private context is available only in personal scope. Examples include private notes, sensitive personal memory, personal chat summaries, and private preferences.

Security policy is global and applies everywhere, but it is not conversational memory. Examples include filesystem sandbox roots, shell risk rules, tool permission rules, and network permission requirements.

Personal scope may read global public context, global private context, personal messages, personal memories, personal tasks, and personal knowledge sources.

Group scope may read global public context, security policy, messages in the same group, group-scoped memories, group-scoped tasks, group-bound knowledge, and group member or agent configuration.

Group scope must not read personal messages, global private context, other groups, or bridge routes not bound to a member of the group.

### Hard Rules

- No agent call without a scope.
- No memory retrieval without a scope filter.
- No task creation without a scope.
- No knowledge search without a scope filter.
- No bridge group routing without an explicit target, reply context, or unexpired temporary route.
- No remote agent receives host private context.
- No UI convenience feature may bypass scope checks.

## Bridge Routing

Bridge routes are entry points, not groups.

The default bridge target is always personal scope:

```text
bridge message -> personal Conversation Space
```

## Discord Bridge Rules

Discord is an experimental bridge aimed at two different flows and they use different routing rules:

1. Allowed channel:
   - Only the configured `owner_user_id` is accepted.
   - Only channels listed in `allowed_channels` are accepted.
   - Plain messages must explicitly mention `@alpha` before they are routed to the local personal bridge flow.
   - `/g <group> ...` and `/use <group|personal>` are accepted as explicit routing commands and do not require an `@alpha` mention.

2. Pack Channel thread:
   - A Discord thread can be bound to one local Pack Channel through `channel_transport_bindings`.
   - Messages posted inside a bound thread never enter personal chat.
   - Instead they are interpreted as Pack Channel review input:
     - `/continue ...` or `继续` -> continue the current review gate
     - `/abort ...` or `终止` -> abort the Pack Channel
     - `/changes ...` or `打回 ...` -> request changes
     - any other text -> review comment

This keeps Discord collaboration deterministic:

- allowed channels are the access boundary
- `pack_hub_channel_id` is the outbound projection target
- `pack_thread_mode` decides whether Pack Channels create a dedicated Discord thread or post directly into the hub channel
- `pack_fallback_to_channel` decides whether projection falls back to the hub channel if thread creation fails

Bridge-originated Pack Channel comments and decisions carry an origin marker so the same Discord thread does not receive its own mirrored echo back.

Explicit group routing can use:

```text
/g <group> <message>
发到<群名>：<内容>
reply_to previous group notification
/use <group> until expiry
```

Target resolution should produce:

```rust
pub enum ResolvedBridgeTarget {
    Personal,
    Group { group_id: String },
}
```

Every bridge inbound message should record platform, external chat id, external user id, resolved scope, source route id, original message id, and reply mapping when available.

A bridge user does not need an XMTP identity. OpenPup acts as a gateway for that route:

```text
QQ user
  -> OpenPup bridge route
  -> OpenPup local identity
  -> OpenPup Conversation Space
  -> optional XMTP transport
```

## XMTP Layering

XMTP is the encrypted remote transport. OpenPup Conversation Space is the local semantic projection.

```text
OpenPup Conversation Space
  - local id
  - title
  - members
  - local messages
  - Alpha group scope
  - bridge routes
  - context isolation
  - permissions
        |
        v
Conversation Transport Binding
  - kind = xmtp
  - transport_ref = xmtp conversation/group id
        |
        v
XMTP Group
  - encrypted transport
  - remote delivery
  - membership substrate
  - offline delivery
  - fanout
```

The OpenPup conversation id and the XMTP group id are different:

```text
OpenPup conversation id:
conv_xxx

XMTP conversation/group id:
xmtp_xxx
```

## Network Participants

Do not model remote participants as OpenPup instances. Model them as network participants.

Participant kinds:

```text
human
agent
app
gateway
unknown
```

Examples:

```text
Ben using OpenPup desktop:
participant_kind = human
app_kind = openpup

OpenPup Alpha:
participant_kind = agent
app_kind = openpup
agent_key = alpha

OpenClaw:
participant_kind = agent
app_kind = openclaw
agent_key = claw

OpenPup QQ bridge:
participant_kind = gateway
app_kind = openpup
route_label = qqbot bridge
```

### Network Identity Tables

```text
network_identities
- id
- network_kind          // xmtp
- network_identity_id   // inbox id or sender address
- display_name
- participant_kind      // human | agent | app | gateway | unknown
- app_kind              // openpup | openclaw | unknown
- agent_key             // alpha | claw | analyst | null
- capabilities_json
- trust_level           // owner | trusted | untrusted | blocked
- first_seen_at
- last_seen_at

xmtp_identities
- id
- inbox_id
- address
- signer_kind
- key_ref
- db_path
- env
- created_at
- updated_at

xmtp_message_map
- local_message_id
- xmtp_message_id
- xmtp_conversation_id
- direction             // inbound | outbound
- created_at
```

`xmtp_message_map` prevents duplicates when a locally sent message later appears in the XMTP stream.

## Agent Conversation Envelope

OpenPup should send structured JSON envelopes over XMTP, not raw text, for networked group semantics.

### agent.chat.message.v1

```json
{
  "type": "agent.chat.message.v1",
  "protocol": "agent-conversation-v1",
  "conversation": {
    "local_hint": "conv_xxx",
    "transport": "xmtp",
    "transport_ref": "xmtp_group_xxx"
  },
  "message_id": "local_msg_xxx",
  "sender": {
    "transport": {
      "network": "xmtp",
      "inbox_id": "inbox_xxx"
    },
    "client": {
      "kind": "openpup",
      "instance_id": "openpup:abcd1234",
      "display_name": "OpenPup abcd1234",
      "version": "0.1.23"
    },
    "actor": {
      "kind": "agent",
      "actor_id": "alpha",
      "display_name": "Alpha",
      "agent_key": "alpha"
    },
    "via": {
      "kind": "agent",
      "label": "Alpha"
    }
  },
  "content": {
    "content_type": "text/plain",
    "text": "hello"
  },
  "created_at": 1760000000
}
```

`conversation.local_hint` is only a hint. Remote clients must map the XMTP group id to their own local Conversation Space.

The `via` field must be explicit when constructing the envelope. It describes how the message entered the conversation, such as local owner, local agent, bridge route, or gateway.

### agent.hello.v1

Future-compatible participant announcement:

```json
{
  "type": "agent.hello.v1",
  "protocol": "agent-conversation-v1",
  "sender": {
    "network": "xmtp",
    "inbox_id": "inbox_xxx",
    "display_name": "OpenClaw",
    "participant_kind": "agent",
    "app_kind": "openclaw",
    "agent_key": "claw"
  },
  "capabilities": {
    "chat": true,
    "summarize": true,
    "code_review": true,
    "tool_call_request": false,
    "supports_mentions": true
  },
  "created_at": 1760000000
}
```

### MVP Envelope Rule

For MVP-3, OpenPup imports XMTP group messages only when they contain a valid agent conversation envelope.

Plain XMTP text:

- is ignored by OpenPup group semantics in MVP-3
- does not create a conversation member
- does not trigger local Alpha
- does not trigger tools

Plain text compatibility can be revisited later, but it must remain chat-only and untrusted.

## Sending Flow

```text
UI or bridge posts local group message
  -> insert conversation_messages
  -> emit local message event
  -> find active xmtp transport binding
  -> build agent.chat.message.v1 envelope
  -> helper sendMessage
  -> insert xmtp_message_map
```

If helper send fails, keep the local message and add a system message or visible status indicating transport failure.

## Receiving Flow

```text
helper emits XMTP message
  -> resolve transportRef to conversation_id
  -> check xmtp_message_map
  -> parse envelope
  -> ignore if the envelope is invalid
  -> upsert network_identity
  -> ensure conversation_member for the envelope participant
  -> insert conversation_messages
  -> insert xmtp_message_map
  -> emit conversation_message_created
  -> emit conversation_members_changed if needed
```

Remote messages never enter personal chat history.

Do not create actor-level local members during add-member UI flows. Membership created from network activity should correspond to envelope participants.

## Responder Policy

Networked groups default to mention-required:

```text
local-only group:
  Alpha may auto-reply to local bridge /g messages

xmtp-bound group:
  Alpha replies only when local @alpha is mentioned
```

Mention rules:

```text
plain message:
  record only

@alpha:
  only local OpenPup Alpha may respond

@openpup-xxxx/alpha:
  unique remote or local agent address, future-compatible

@all-agents:
  future feature
```

Local Alpha and remote same-name agents must be treated as different mention targets.

## Remote Agent Trust Boundary

Remote agents are conversation members, not local tool callers.

Default rules:

- A remote agent may send chat messages.
- A remote agent may advertise capabilities.
- A remote agent may not call OpenPup shell tools.
- A remote agent may not read OpenPup private personal memory.
- A remote agent may not read other Conversation Spaces.
- A remote agent may not write tasks, memories, or knowledge without explicit permission.
- A remote agent may not trigger bridge outbound messages by default.

Unknown remote agents default to:

```text
trust_level = untrusted
```

First version behavior:

```text
untrusted remote agent = chat only
```

## XMTP Node Helper

OpenPup should not link the Rust XMTP FFI directly for MVP-3.

The implementation decision is:

```text
OpenPup Tauri/Rust
  - ConversationSpace
  - SQLite
  - Alpha group scope
  - bridge routes
  - local events
        |
        | stdio JSONL
        v
openpup-xmtp-helper Node/TS
  - XMTP identity
  - XMTP group create/join
  - add/remove members
  - send agent envelope
  - stream XMTP messages
        |
        v
XMTP Group
```

Use a child process managed by Tauri/Rust. Use stdio JSONL for MVP-3 because it avoids port conflicts, avoids exposing a local server, and gives Rust a clear lifecycle boundary.

### Helper Layout

```text
xmtp-helper/
  package.json
  tsconfig.json
  src/
    index.ts
    protocol.ts
    xmtpClient.ts
    envelope.ts
```

The helper is a small transport service, not a second application.

### Identity Storage

XMTP identity is application-managed. The UI must not ask users to paste or edit private keys.

Rust owns identity persistence, following the same pattern as LLM API keys:

- plaintext is available only in memory
- `config.toml` stores `enc2:` values under `[xmtp]`
- `.keystore` stores the local AES-GCM key with 0600 permissions
- the Node helper receives `identityPrivateKey` and `dbEncryptionKey` only in the `init` request

The helper opens its persistent XMTP DB under `dataDir`. If the real XMTP client cannot start, `init` fails. Product UI should not expose a mock transport.

### Helper Protocol

Rust stdin to helper:

```json
{"id":"1","method":"sendMessage","params":{}}
```

Helper stdout response:

```json
{"id":"1","result":{}}
```

Helper stdout event:

```json
{"event":"message","payload":{}}
```

### Helper Commands

MVP-3 commands:

```text
init
status
identity
createGroup
addMembers
removeMembers
requestRemoval
syncGroups
sendMessage
startStream
stopStream
```

Future commands:

```text
joinGroup
exportInvite
resolveInvite
```

### init

```json
{
  "id": "1",
  "method": "init",
  "params": {
    "env": "dev",
    "dataDir": "/path/to/workspace/xmtp",
    "identityRef": "default",
    "identityPrivateKey": "0x...",
    "dbEncryptionKey": "..."
  }
}
```

Rust generates and saves the identity if `[xmtp]` is missing, then passes decrypted values to the helper.

### createGroup

```json
{
  "id": "2",
  "method": "createGroup",
  "params": {
    "conversationId": "conv_xxx",
    "title": "Alpha Lab"
  }
}
```

Response:

```json
{
  "id": "2",
  "result": {
    "transportRef": "xmtp_group_xxx",
    "invite": "agentchat://join?transport=xmtp&protocol=agent-conversation-v1&transport_ref=xmtp_group_xxx"
  }
}
```

### addMembers

XMTP groups are admin-added in MVP-3. The UI add-member flow takes an XMTP inbox id or compatible member id.

```json
{
  "id": "3",
  "method": "addMembers",
  "params": {
    "transportRef": "xmtp_group_xxx",
    "inboxIds": ["xmtp_inbox_xxx"]
  }
}
```

### removeMembers

Kicking a remote member must go through the transport when an XMTP binding is active.

```json
{
  "id": "4",
  "method": "removeMembers",
  "params": {
    "transportRef": "xmtp_group_xxx",
    "inboxIds": ["xmtp_inbox_xxx"]
  }
}
```

After transport removal succeeds, local membership rows for the same transport inbox can be marked removed or cleaned up.

### requestRemoval

Leaving an XMTP group should request transport-level removal first, then remove or archive the local projection.

```json
{
  "id": "5",
  "method": "requestRemoval",
  "params": {
    "transportRef": "xmtp_group_xxx"
  }
}
```

### sendMessage

```json
{
  "id": "6",
  "method": "sendMessage",
  "params": {
    "transportRef": "xmtp_group_xxx",
    "envelope": {
      "type": "agent.chat.message.v1",
      "protocol": "agent-conversation-v1"
    }
  }
}
```

Response:

```json
{
  "id": "6",
  "result": {
    "remoteMessageId": "xmtp_msg_xxx"
  }
}
```

### group Event

```json
{
  "event": "group",
  "payload": {
    "transportRef": "xmtp_group_xxx",
    "title": "Alpha Lab",
    "description": "",
    "createdAt": 1760000000,
    "addedByInboxId": "xmtp_inbox_xxx"
  }
}
```

### message Event

```json
{
  "event": "message",
  "payload": {
    "transportRef": "xmtp_group_xxx",
    "remoteMessageId": "xmtp_msg_xxx",
    "envelope": {
      "type": "agent.chat.message.v1",
      "protocol": "agent-conversation-v1"
    },
    "rawText": null
  }
}
```

## Runtime Responsibilities

Rust remains the source of truth for local product state.

Rust owns:

- `conversation_spaces`
- `conversation_members`
- `conversation_messages`
- `conversation_transport_bindings`
- `network_identities`
- `xmtp_message_map`
- Alpha responder policy
- bridge `/g` and `/use` routing
- context isolation and tool permissions

Rust calls the helper only at transport boundaries:

```text
enable xmtp for group
add or remove xmtp members
send envelope to xmtp
receive xmtp stream event
request local user's removal from xmtp group
```

## Memory, Tasks, Knowledge, and Summaries

Target memory fields:

```text
scope_kind: global_public | global_private | personal | group
scope_id: nullable string
```

Rules:

- Personal memory uses `scope_kind = personal`.
- Group memory uses `scope_kind = group` and `scope_id = group_id`.
- Public owner preferences use `scope_kind = global_public`.
- Private owner preferences use `scope_kind = global_private`.
- Memory retrieval must filter by current scope.
- Memory extraction writes to the active scope unless the user explicitly asks to save globally.

Conversation summaries must be scoped:

```text
scope_kind: personal | group
scope_id: nullable string
pup_or_agent_id: string
```

Tasks must be scoped:

```text
scope_kind: personal | group
scope_id: nullable string
created_by_actor_id
created_by_role
```

Knowledge sources must be scoped or explicitly shared:

```text
scope_kind: global_public | personal | group
scope_id: nullable string
```

Aggregated UI may show multiple scopes, but execution and retrieval must stay scoped.

## Tool Permissions

Tool permission checks must include scope and actor:

```rust
pub struct ToolPermissionContext {
    pub scope: ContextScope,
    pub actor: ScopeActor,
    pub tool_name: String,
}
```

Security policy applies globally. Group permissions can add role checks, but shell and filesystem risk checks remain mandatory regardless of group membership.

## UI Implications

- Group chat displays only group messages.
- Personal chat displays only personal messages.
- Group membership UI should distinguish local owner, local agents, remote humans, remote agents, and gateways.
- Add member in an XMTP-bound group means adding an XMTP member, not creating a fake local actor.
- Delete group should require confirmation.
- Leave group should request transport-level removal when possible, then clean local projection.
- Remove member should call transport-level remove when an XMTP binding exists.
- Message rendering may support Markdown and diagrams, but rendered content must not change scope or permissions.
- Unique mention tokens should be clickable and fill the composer.

## MVP-3 Completion Criteria

MVP-3 is complete when:

- OpenPup creates or binds a Conversation Space to an XMTP group.
- OpenPup sends `agent.chat.message.v1` to the XMTP group.
- OpenPup streams and imports `agent.chat.message.v1` from the XMTP group.
- Only valid envelope participants become local conversation members.
- Duplicate messages are prevented through `xmtp_message_map`.
- Networked groups default to mention-required agent replies.
- Bridge `/g` messages can publish to XMTP after local insert.
- Add member uses XMTP transport membership.
- Remove member and leave group reach the XMTP transport layer.

Out of scope for MVP-3:

- Attachments
- Reactions
- Message edits/deletes
- Read receipts
- Push notifications
- Complex trust delegation
- Remote tool execution
- Key rotation UI
- Custom OpenPup relay

## Implementation Order

1. Add `xmtp-helper/` TS skeleton with JSONL protocol.
2. Add Rust child-process manager for the helper.
3. Add envelope encode/decode in Rust and TS.
4. Add database tables and transport binding commands.
5. Implement `enable_xmtp_for_conversation`.
6. Publish local group messages to helper/XMTP.
7. Import helper stream events into local Conversation Space.
8. Add UI controls for enabling XMTP, showing status, adding members, removing members, and leaving groups.
9. Add mention-required responder policy for xmtp-bound groups.
10. Migrate memory, tasks, knowledge, summaries, bridge records, agent runs, and tool permission requests to explicit scope fields.

## Local Two-Workspace Test

Desktop runtime supports overriding the workspace root for local multi-node tests:

```bash
OPENPUP_WORKSPACE=/tmp/openpup-a npm run tauri:dev
OPENPUP_WORKSPACE=/tmp/openpup-b npm run tauri:dev
```

Each workspace gets its own `config.toml`, SQLite DB, XMTP DB, and XMTP identity.

`OPENPUP_WORKSPACE` is the preferred test variable. `OPENPUP_APP_ROOT` may remain as a compatibility alias.
