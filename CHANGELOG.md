# Changelog

All notable changes to OpenPup will be documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased](https://github.com/openpup/openpup/compare/v0.1.0...HEAD)

### Added

- Unified `~/.openpup/config.toml` replaces scattered JSON config files
- Cargo workspace at repo root — single `target/` and `Cargo.lock` for all crates
- CLI binary (`openpup`) ships from the same workspace as the desktop app
- Light theme: full Tailwind v4 CSS custom property overrides + `!important` guards
- LLM streaming via `litellm-rs` `completion_stream` (replaces manual reqwest SSE)
- MCP: rmcp streamable-HTTP transport only — removed JSON-RPC fallback
- ClaWHub skill-hub auto-installed on first run via `install_from_git`
- CI workflow now checks both Rust workspace and frontend TypeScript build
- Release workflow builds CLI for Linux x86_64, macOS aarch64/x86_64, Windows x86_64
- Release workflow builds Tauri desktop app via `tauri-apps/tauri-action`
- MIT LICENSE

---

## [0.1.0](https://github.com/openpup/openpup/releases/tag/v0.1.0) — 2026-03-18

### Added

#### Core Architecture

- Tauri 2.0 + Rust backend + React 19 frontend
- Multi-agent system: Alpha orchestrator + 5 specialist pups (Dev / Writer / Ops / Life Admin / Research)
- SQLite memory store with semantic vector search (BAAI/bge-m3 embeddings)
- Streaming chat via `litellm-rs` — OpenAI-compatible API, supports Ollama

#### Memory System

- Long-term memory extraction every 5 messages
- Semantic deduplication before insert (cosine similarity ≥ 0.88)
- Memory type icons: ❤️ preference, ❌ boundary, 📋 fact, 🔼 event
- Daily diary entries at `~/.openpup/memories/YYYY-MM-DD.md`
- Memory search (LIKE + semantic)
- Memory CRUD UI (MemoryManager component)

#### Skills

- TOML-defined `prompt_chain` skills — compatible with ClaWHub / Anthropic Agent Skills protocol
- Skill registry: install from Git, discover from remote JSON endpoints
- Built-in skills: `find_skills`, `browser_control`, `file_operations`, `weekly_summary`, `daily_summary`, `skill_vetting`
- Security vetting before install: LLM reviews TOML manifest for risk flags
- Cron-based skill scheduler (60s tick, auto-run in FreeRun mode)
- Alpha Heartbeat: daily skill recommendations and RULES self-evolution

#### MCP (Model Context Protocol)

- rmcp 1.2.0 with streamable HTTP transport
- Multi-server management: add/remove/toggle servers at runtime
- Tool discovery cache per server
- Persisted to `~/.openpup/mcp_servers.json`
- Local MCP server: `read_file`, `write_file`, `open_browser`

#### Pup System

- 5 built-in specialists + Alpha orchestrator
- Custom pup creation via UI (PupManager)
- Native plugin support: load `.dylib`/`.so`/`.dll` from `~/.openpup/plugins/`
- Forced pup routing: click a pup in sidebar to bypass Alpha classification
- Pup memory injection: top-5 relevant memories in every specialist's system prompt

#### UI

- GitHub-style dark theme + warm light theme with full toggle
- Leashed / FreeRun mode toggle
- Permission dialog: risk-level-driven design (red/amber/emerald)
- Streaming Markdown rendering with live token display
- Abort button to cancel in-progress response
- Conversation search (Timeline tab)
- Task manager with status lifecycle (pending → in_progress → done / failed)
- Skill store: installed, discover, registry management tabs
- Onboarding flow: 6-step conversational setup (name, preferences, boundaries...)
- Model switcher in chat header
- Workspace backup/restore (export tar.gz, import with auto-backup)

#### CLI (`openpup`)

- `openpup chat [--pup <key>]` — REPL with rustyline history
- `openpup memory list/search/count`
- `openpup skill list/run [--dry-run] [--input]`
- `openpup status`
- Reads same `~/.openpup/` database as the GUI
- Streaming SSE output in terminal

