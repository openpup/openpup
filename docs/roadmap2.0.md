# Roadmap 2.0

## Configurable Organization OS

Roadmap 2.0 reframes openpup from a multi-agent desktop companion into a configurable organization operating system.

The goal is not simply to spawn more pups per task. The goal is to let one person define an organization, assign long-lived responsibilities to AI roles, and operate that organization through routines, triggers, collaboration, and memory.

---

## Why

Current DAG-based collaboration is strong at executing a single complex task:

- Alpha decomposes a request
- specialist pups execute in layers
- results are aggregated into one final reply

That is useful, but it is still fundamentally task-scoped. It does not by itself create:

- long-lived ownership
- recurring organizational behavior
- proactive detection of work
- durable role memory
- governance, escalation, and approval structure

Roadmap 2.0 aims to close that gap.

---

## Product Thesis

openpup should become a system where:

- users define an organization, not just a set of isolated agents
- pups can be bound to roles with persistent mandates
- work can originate from user commands, routines, or external triggers
- complex work escalates into `Pack Channel`, while simpler work stays within a single role or handoff
- external surfaces like Telegram and Discord act as operating interfaces for the same shared organization state

In short:

**v1 was about multi-agent execution.**
**v2 is about organizational capability.**

---

## Core Design Principles

### 1. Organization First

The primary abstraction is an organization definition, not a single agent prompt.

An organization should be able to express:

- identity
- structure
- units
- roles
- authority
- escalation rules
- communication protocols

### 2. Roles Before Tasks

Each role should carry a long-lived mandate.

Tasks are one expression of that mandate, not the reason the role exists.

This allows:

- proactive work
- ownership continuity
- role-specific memory
- reliable delegation

### 3. Escalation Instead of Over-Orchestration

Not every piece of work should become a DAG.

Expected operating modes:

- single-role execution
- handoff between roles
- DAG / `Pack Channel` escalation for complex work

This keeps collaboration intentional and legible.

### 4. Shared State, Multiple Surfaces

Telegram, Discord, desktop chat, and `Pack Channel` should not become separate systems.

They should all project the same underlying:

- organization memory
- role state
- work queues
- channel history
- task status

---

## Capabilities To Build

### Organization Spec

Support a configurable organization schema with concepts such as:

- organization
- unit
- role
- agent binding
- mandate
- policy
- routine
- trigger

This should support many shapes:

- one-person company
- one-person dynasty
- one-person department
- studio
- lab
- family office

### Role Inbox And Work Queue

Each role needs a persistent inbox/backlog so work can accumulate outside of a single request.

Work should be typed:

- directed
- reactive
- recurring
- strategic

### Routines And Triggers

The system should be able to create work from:

- schedules
- stale tasks
- time-based review cadences
- external messages
- file changes
- service events

This is the foundation of bounded AI proactivity.

### Memory Layers

Memory should be separated across multiple scopes:

- personal
- organizational
- unit
- role
- task

This prevents role confusion and makes long-term operation coherent.

### Governance

Different organizations need different operating rules.

The system should support:

- approval chains
- role authority boundaries
- escalation paths
- user override and stop controls

### External Operating Surfaces

Roadmap 2.0 treats external bridges as operating interfaces:

- Telegram: lightweight owner control surface
- Discord: richer collaborative surface with visible role interaction
- Desktop: deep inspection and orchestration console

---

## Suggested Implementation Phases

### Phase 1: Organization Foundations

- define organization schema
- bind pups to roles
- store mandates and policies
- keep current DAG engine as the execution core

### Phase 2: Persistent Work

- role inboxes
- work queues
- recurring jobs
- trigger-driven task creation

### Phase 3: Escalation Engine

- decide when work stays local to a role
- decide when to hand off
- decide when to escalate into `Pack Channel`

### Phase 4: External Surfaces

- Telegram as operational control surface
- Discord as visible collaboration surface
- unified state across UI and bridges

---

## Non-Goals For Roadmap 2.0

Roadmap 2.0 is not about:

- making every surface equally complex
- turning all interaction into autonomous group chat
- replacing the user as final authority
- maximizing the number of active pups on screen

It is about creating a configurable structure where AI roles can operate responsibly, persistently, and visibly.

---

## Definition Of Success

Roadmap 2.0 succeeds when openpup can do all of the following:

- represent user-defined organization structures cleanly
- keep long-lived role responsibilities coherent over time
- generate and maintain work without requiring every step to be user-triggered
- escalate complex work into visible collaboration channels
- expose the same organizational state across desktop and bridge surfaces

At that point, openpup stops being just a multi-agent app and becomes an organization operating system.
