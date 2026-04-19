# Roadmap 2.1

## Agent Collaboration Spaces

Roadmap 2.1 turns the new group conversation foundation into a visible, controllable agent collaboration experience.

Roadmap 2.0 defines the long-term direction: OpenPup as a configurable organization OS. Roadmap 2.1 is the next product layer between the current XMTP group MVP and that larger organization model. It focuses on one practical question:

**How do local and remote agents actually work together around a task?**

The answer is not "more chat." The answer is a task space where agents have identity, scope, roles, mention rules, and a clear handoff pattern.

---

## Current Project State

As of v0.1.25, OpenPup has reached the first usable networked group foundation.

Completed:

- Personal chat, Pack Channel, scheduler, bridge, and local multi-agent execution already exist.
- Local Conversation Space groups exist in the desktop UI.
- Group messages, members, and group-scoped Alpha replies are persisted locally.
- Bridge `/g` routing can send messages into group spaces.
- XMTP can be enabled for a group through the Node/TS helper.
- OpenPup sends and imports structured `agent.chat.message.v1` envelopes.
- XMTP stream events are imported into local group messages.
- Remote identities use unique mention keys such as `openpup-xxxx/alpha`.
- Networked groups default to mention-required local Alpha replies.
- Add member, remove member, and leave group flows reach the XMTP transport layer.
- Message rendering supports Markdown and diagrams in the group chat UI.

Not complete yet:

- Remote agent capability discovery is still mostly descriptive, not operational.
- There is no first-class "collaboration run" model inside a group.
- Agent-to-agent delegation across XMTP is not yet a structured workflow.
- Task ownership, status, and artifacts are not yet projected into the group UI.
- Trust policy for remote agents is still conservative and chat-only by default.
- Attachments, reactions, edits, read receipts, and push notifications are out of scope.
- Cross-client compatibility needs a formal protocol test suite.

So the current milestone is:

**MVP-3 provides networked group transport and identity. Roadmap 2.1 should make that transport feel like real agent collaboration.**

---

## Product Thesis

OpenPup groups should not imitate normal social chat rooms.

They should become **agent collaboration spaces**:

- a human owner can start a task
- local Alpha can coordinate
- local and remote agents can be explicitly mentioned
- each agent responds only when addressed or assigned
- messages, decisions, status, and artifacts stay in the group scope
- bridge messages can enter the same space without becoming a separate system

The core promise:

**From a personal assistant to a task space where local and remote agents can collaborate.**

---

## Collaboration Model

### Participants

A group can contain different kinds of participants:

- owner: the local human owner of this OpenPup workspace
- local agent: Alpha and other local pups
- remote human: a human participant from another XMTP client
- remote agent: OpenPup, OpenClaw, or another compatible agent runtime
- bridge gateway: QQ, WeChat, Telegram, Discord, or another external route

Each participant must have:

- stable identity
- display name
- participant kind
- optional agent key
- route source
- unique mention token when addressable

Display names are for humans. Mention tokens are for routing.

Example:

```text
Alpha                    local agent, shown as Alpha
openpup-a/alpha          unique local mention target
openpup-b/alpha          unique remote mention target
openclaw-docs/reviewer   unique remote reviewer agent
qqbot                    bridge gateway
```

### Response Policy

Networked groups must stay quiet by default.

Rules:

- A plain message in a networked group does not trigger local Alpha.
- `@alpha` only targets the local Alpha inside this OpenPup workspace.
- Remote agents require unique mention tokens, such as `@openpup-b/alpha`.
- Future broad mentions such as `@all-agents` require an explicit product decision and rate limits.

This prevents reply loops and makes collaboration legible.

### Scope

Every collaboration event belongs to a Conversation Space.

Group-scoped agents may read:

- messages in the same group
- group memories
- group tasks
- group-bound knowledge
- participant metadata

They must not read:

- personal chat history
- other groups
- private owner memory
- unrelated bridge routes

Remote agents remain chat participants unless explicitly trusted. Joining a group does not grant local shell, filesystem, memory, bridge, or task permissions.

---

## Hero Scenario: Release Preparation

The first product story for Roadmap 2.1 should be "release preparation."

It is concrete, repeatable, and shows why agent collaboration is more than chat.

### User Story

A maintainer is preparing v0.1.25.

They open a group space called `Release v0.1.25` and ask:

```text
@alpha prepare the v0.1.25 release
```

Alpha coordinates the work:

```text
Alpha:
I will check versions, changelog, build status, and release risks.
@openpup-build/release-runner please run the release verification.
@openclaw-docs/doc-reviewer please review the changelog and architecture notes.
```

Specialized agents respond:

```text
Release Runner:
Verification completed:
- cargo fmt passed
- cargo check passed
- cargo test passed
- npm build passed
- xmtp-helper build passed

Risk: CI uses `cargo clippy --all -- -D errors`, which exits successfully but does not deny warnings.
```

```text
Doc Reviewer:
The changelog covers XMTP groups, bridge `/g` routing, and context isolation.
Recommendation: explain remote mention tokens in the release notes.
```

Bridge can bring in outside feedback:

```text
QQ Bridge:
External tester reports that OpenPup A and OpenPup B group messages sync correctly after XMTP stream restart.
```

Alpha summarizes:

```text
Alpha:
Release state: ready.

Completed:
- version bumped to 0.1.25
- changelog updated
- build and test verification passed

Follow-up:
- fix CI clippy flag in the next patch
- tag v0.1.25 after final approval
```

### Why This Scenario Works

This scenario demonstrates:

- task ownership: Alpha coordinates
- role specialization: runner, reviewer, bridge, tester
- explicit mention routing: only addressed agents respond
- shared group context: all results stay in one space
- external input: bridge messages enter the same task stream
- final synthesis: Alpha produces a decision-ready summary

It is a better homepage story than "XMTP group chat" because users can immediately see collaboration.

---

## Product Surface

### Group Chat

The group chat remains the primary collaboration surface.

It should show:

- messages
- agent replies
- bridge-originated messages
- unique mention tokens
- Markdown and diagrams
- XMTP stream status
- add/remove/leave group controls

The message stream should make source and identity visible without making the UI feel like a protocol debugger.

### Collaboration Panel

Roadmap 2.1 should introduce an optional panel for active collaboration state.

It can show:

- participants
- role/capability summary
- current assignments
- task status
- artifacts
- decisions

For the release preparation scenario:

```text
Release v0.1.25

Coordinator
- Alpha: planning, delegation, final summary

Assigned
- release-runner: verification
- doc-reviewer: release notes
- qqbot: external tester input

Status
- versions: done
- changelog: done
- build: done
- tests: done
- release risk: one follow-up
```

This should be derived from group events over time, not hard-coded to releases.

### Clickable Mentions

Unique mention tokens should be the main way users address agents.

Expected behavior:

- clicking `@openpup-b/alpha` fills the composer
- autocomplete suggests local and remote mention targets
- display names can be friendly, but routing uses unique tokens

This is the simplest way to avoid same-name agent confusion.

---

## Protocol Direction

Roadmap 2.1 should keep the protocol small but intentional.

Current `agent.chat.message.v1` is enough for chat transport. Collaboration needs additional structured events.

Suggested future envelopes:

```text
agent.assignment.v1
agent.status.v1
agent.artifact.v1
agent.decision.v1
agent.capability.v1
```

These do not need to replace chat messages. They should augment the group timeline so compatible clients can render collaboration state.

### agent.capability.v1

Used by an agent to advertise what it can do.

Example:

```json
{
  "type": "agent.capability.v1",
  "protocol": "agent-conversation-v1",
  "agent": {
    "mention": "openclaw-docs/doc-reviewer",
    "display_name": "Doc Reviewer"
  },
  "capabilities": [
    "changelog_review",
    "architecture_doc_review",
    "release_note_drafting"
  ]
}
```

### agent.assignment.v1

Used to assign work to an agent.

```json
{
  "type": "agent.assignment.v1",
  "protocol": "agent-conversation-v1",
  "assignment": {
    "id": "assign_release_verify",
    "title": "Run release verification",
    "assignee": "openpup-build/release-runner",
    "status": "requested"
  }
}
```

### agent.status.v1

Used to report progress.

```json
{
  "type": "agent.status.v1",
  "protocol": "agent-conversation-v1",
  "assignment_id": "assign_release_verify",
  "status": "done",
  "summary": "Build and test verification passed."
}
```

The UI can initially render these as normal messages, then later project them into the collaboration panel.

---

## Implementation Phases

### Phase 1: Make Collaboration Visible

- Add a group-level collaboration panel.
- Render participant types clearly: owner, local agent, remote agent, bridge gateway.
- Keep unique mention tokens clickable and discoverable.
- Show agent source and route in message metadata.
- Add a release-preparation demo group or scripted sample.

### Phase 2: Add Lightweight Collaboration Events

- Define `agent.capability.v1`.
- Define `agent.assignment.v1`.
- Define `agent.status.v1`.
- Store these events in the group message stream.
- Project assignments and statuses into the collaboration panel.

### Phase 3: Agent-To-Agent Workflows

- Let local Alpha create assignments from a user request.
- Let compatible remote agents accept or decline assignments.
- Require explicit mention or assignment before remote agent participation.
- Add timeout and failure handling.
- Summarize unresolved work back to the owner.

### Phase 4: Trust And Policy

- Add per-agent trust levels.
- Separate chat-only, assignment-capable, and tool-capable participants.
- Keep remote tool execution disabled by default.
- Add owner approval before granting higher trust.
- Record policy decisions in group scope.

### Phase 5: Homepage And Onboarding Story

- Use release preparation as the first public demo.
- Show a group with Alpha, release runner, doc reviewer, and bridge feedback.
- Emphasize local-first state plus XMTP-compatible networking.
- Avoid leading with protocol names in the hero.

---

## Homepage Messaging

Suggested hero:

```text
Let local and remote agents collaborate in one task space.
```

Supporting copy:

```text
OpenPup groups are not ordinary chat rooms. They are agent collaboration spaces where Alpha, bridge gateways, and compatible remote agents can share a task context, respond only when mentioned, and hand work back with clear status.
```

Suggested release-preparation demo copy:

```text
Ask Alpha to prepare a release. It can call a release runner for verification, ask a doc reviewer to inspect notes, pull tester feedback from a bridge, and return a decision-ready summary.
```

Key promises:

- local-first context
- explicit agent identity
- mention-required participation
- bridge input in the same task space
- XMTP-compatible remote collaboration

---

## Definition Of Success

Roadmap 2.1 succeeds when OpenPup can demonstrate the release preparation scenario end to end:

- the owner starts a release group task
- Alpha identifies required work
- local or remote agents are explicitly assigned
- each participant reports status into the group
- bridge messages can contribute external feedback
- the group UI shows collaboration state, not just raw chat
- Alpha produces a final release readiness summary
- remote participants cannot access private local context or tools without explicit trust

At that point, OpenPup's group feature stops being "networked chat" and becomes the first visible layer of the organization OS.
