<div align="center">

<img src="openpup-icon.svg" width="80" alt="OpenPup">

# OpenPup

**The local AI assistant that remembers who you are**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/tauri-2.0-blue.svg)](https://tauri.app)

[← Back](README.md) · [中文](README.zh.md)

</div>

---

## What is OpenPup?

Most AI tools ask: *"What kind of AI do you want?"*
OpenPup asks: **"What kind of person are you?"**

Your `OWNER.md` is the core — a portrait you write about yourself, read by every pup on every message. Conversations, preferences, and habits accumulate into a **local memory** that no cloud service can replicate or take away.

A team of specialized "pups" handles your requests:

| Pup | Role |
|-----|------|
| Alpha | Orchestrator — knows you, coordinates the others |
| Dev | Software engineering, code review, debugging |
| Writer | Articles, emails, reports, any written content |
| Ops | DevOps, servers, deployments, infrastructure |
| Research | Information gathering, summarization, analysis |
| Life Admin | Scheduling, planning, personal organization |

Everything lives in `~/.openpup/` — plain files you can read, edit, and back up.

---

## vs. The Alternatives

Three mainstream open-source agent frameworks, each with a distinct focus:

### Feature Comparison

| | **OpenPup** | **OpenClaw** | **OpenFang** |
|---|:---:|:---:|:---:|
| **Position** | Personal desktop assistant | Multi-channel gateway | Agent operating system |
| **Core innovation** | OWNER.md (owner-centric) | Multi-agent routing + 27+ channels | 7 autonomous Hands (pre-built) |
| Tech stack | Rust + Tauri | Node.js + WebSocket | Rust (14 crates) |
| Memory footprint | ~40 MB | ~394 MB | ~40 MB |
| Startup time | seconds | ~6 sec | <200 ms |
| Expert team | ✅ 5 specialists + coordinator | Routes to single agent | Runs Hands (not agents) |
| Interface | Desktop app + CLI | Web UI + messaging channels | Dashboard + CLI |
| Channel support | Internal integrations | 27+ platforms (WhatsApp/Telegram/etc) | 40+ platform gateway |
| Use case | Daily companion | Heavy multi-channel users | Autonomous business flows |
| License | MIT / Apache 2.0 | MIT (foundation-maintained) | MIT |

### Quick Comparison

**OpenPup**
Core: embed your identity (`OWNER.md`) into the system so the pups understand you deeply. Specialist division of labor (Dev/Writer/Research/Ops/LifeAdmin). Native desktop app. Best for people who want a daily companion that gradually understands them better.

**OpenClaw**
Core: multi-channel gateway + multi-agent routing. Single AI assistant, but present simultaneously on WhatsApp, Telegram, Slack, Discord, and 27+ other platforms. Perfect for heavy users who need a unified presence across channels.

**OpenFang**
Core: 7 pre-built autonomous capability packages ("Hands") — lead generation, video clips, intelligence gathering, etc. Executes autonomously without needing conversation. Designed for teams that need to automate business workflows (sales, research, operations).

---

## Features

- **Streaming chat** with intelligent multi-agent routing
- **Long-term memory** — automatically extracts facts about you every 5 messages; semantic dedup prevents noise
- **Skills** — installable prompt-chain automations (TOML, [ClaWHub](https://clawhub.ai) compatible)
- **MCP support** — connect external tools via Model Context Protocol (streamable HTTP, rmcp)
- **Leashed / FreeRun modes** — confirm every action, or let pups act autonomously
- **Permission system** — risk-tiered (high/medium/low) with optional trust for recurring actions
- **Task tracker** — create and track tasks with lifecycle (pending → in_progress → done/failed)
- **CLI** — full terminal REPL (`openpup chat`) using the same local database
- **Workspace backup** — export/import your entire `~/.openpup/` as a tarball
- **Plugin API** — write custom pups as native `.dylib`/`.so`/`.dll` plugins
- **Themes** — GitHub-style dark and light modes

---

## Installation

### Download a release (recommended)

From the [Releases page](../../releases):

| Platform | Desktop app | CLI binary |
|----------|-------------|------------|
| macOS (Apple Silicon) | `openpup-*.dmg` | `openpup-cli-*-darwin-aarch64.tar.gz` |
| macOS (Intel) | `openpup-*.dmg` | `openpup-cli-*-darwin-x86_64.tar.gz` |
| Linux x86_64 | `openpup-*.AppImage` | `openpup-cli-*-linux-x86_64.tar.gz` |
| Windows | `openpup-*-setup.exe` | `openpup-cli-*-windows-x86_64.tar.gz` |

### Build from source

**Prerequisites:** Node.js 18+, Rust stable, [Tauri system deps](https://tauri.app/start/prerequisites/)

```bash
git clone https://github.com/openpup/openpup
cd openpup

# Desktop app
npm install
npm run tauri build

# CLI only (faster, no Node.js needed)
cargo build --release -p openpup
```

---

## Configuration

On first launch, OpenPup creates `~/.openpup/config.toml`:

```toml
[llm]
provider   = "openai"          # "openai" (OpenAI-compatible) or "ollama" (local)
model      = "gpt-4o"
mini_model = "gpt-4o-mini"     # cheaper model for classification tasks
api_key    = "sk-..."          # or set OPENPUP_API_KEY env var
api_base   = ""                # leave empty for default
                               # e.g. https://api.siliconflow.cn/v1 for SiliconFlow

[app]
execution_mode = "leashed"     # "leashed" (confirm actions) or "freerun"
theme          = "dark"        # "dark" or "light"
language       = "zh"          # "zh" or "en"

[pups]
enabled = ["alpha", "dev", "writer", "ops", "research", "life_admin"]
```

Full reference template: [`workspace/config.toml`](workspace/config.toml)

**Environment variable overrides** (take precedence over the file):

| Variable | Purpose |
|----------|---------|
| `OPENPUP_API_KEY` | API key |
| `OPENAI_API_KEY` | Fallback API key |
| `OPENAI_BASE_URL` | Base URL override |
| `OPENPUP_LLM_PROVIDER` | `openai` or `ollama` |

---

## CLI Usage

```bash
# Chat
openpup chat                          # chat with Alpha Pup
openpup chat --pup dev                # route directly to Dev Pup

# Memory
openpup memory list                   # browse long-term memories
openpup memory search "python"        # search by content
openpup memory count                  # total memory count

# Skills
openpup skill list                    # installed skills
openpup skill run weekly_summary      # run a skill
openpup skill run my_skill --input "context text"

# Overview
openpup status                        # memory count, active pups, config summary
```

---

## Skills

Skills are TOML manifests defining multi-step prompt chains. Compatible with the [ClaWHub](https://clawhub.ai) skill format.

```toml
name        = "daily_summary"
description = "Generate a daily summary from your memories"
version     = "1.0.0"

[[steps]]
type   = "search_memories"
query  = "today's activities and tasks"
limit  = 20

[[steps]]
type   = "generate_with_llm"
prompt = "Write a concise daily summary:\n{{memories}}"
```

Install from any Git repo via **Skills → Install from Git** in the app, or via `install_skill_from_git` command. The [ClaWHub skill hub](https://clawhub.ai/PhenixStar/skill-hub) is auto-installed on first run.

---

## MCP Integration

OpenPup uses [rmcp](https://github.com/modelcontextprotocol/rust-sdk) with streamable HTTP transport — the official MCP Rust SDK. Add servers in **Settings → MCP** or in `~/.openpup/mcp_servers.json`.

---

## Plugin Development

Add custom specialist pups as native plugins:

```rust
#[no_mangle]
pub extern "C" fn create_pup() -> *mut dyn SpecialistPup {
    Box::into_raw(Box::new(MyPup::new()))
}
```

Build as `cdylib`, place in `~/.openpup/plugins/`. See [`plugins/example_pup/README.md`](plugins/example_pup/README.md).

---

## Workspace Layout

```
~/.openpup/
├── config.toml          ← all configuration
├── database.db          ← SQLite: conversations, memories, tasks, skill runs
├── OWNER.md             ← your profile — name, preferences, context
├── PUPS.md              ← pup definitions
├── RULES.md             ← evolving rules the pups follow
├── memories/            ← daily diary (YYYY-MM-DD.md)
├── skills_state/        ← installed skill manifests and registry state
├── mcp_servers.json     ← MCP server list
└── plugins/             ← native pup plugins (.dylib / .so / .dll)
```

---

## License

[MIT](LICENSE-MIT) OR [Apache 2.0](LICENSE-APACHE) — © 2026 OpenPup Contributors
