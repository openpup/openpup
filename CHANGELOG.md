# Changelog

All notable changes to OpenPup will be documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased](https://github.com/openpup/openpup/compare/v0.1.2...HEAD)

### Planned — v0.1.3: Multi-Pup Async Collaboration

#### Backend

- **Parallel execution engine** — refactor `do_stream` to spawn multiple pups concurrently via `tokio::join!`, each streaming independently
- **Channel message persistence** — add SQLite tables `channels` / `channel_messages` (channel_id, pup, kind, content, timestamp)
- **Pup-to-pup messaging protocol** — define message kinds (`text` / `artifact` / `@mention`) so Research can pass artifacts to Writer
- **Alpha task decomposition** — Alpha produces a structured delegation plan before dispatching to each pup
- **Result aggregation** — Alpha merges all pup outputs and emits `stream_done` once all complete (`aggregate_pup_results` already drafted)

#### Frontend

- **Pack Channel live subscription** — consume Tauri `channel_message` events in real time, replacing the current static mock
- **Sidebar badge** — show an orange numeric badge on the Pack Channel nav item when a channel is active
- **Jump-back on completion** — display a "View final reply" button at the channel footer when all pups finish, linking back to main Chat
- **Task breakdown card in Alpha bubble** — render a subtask card (`① Dev Pup → … / ② Writer Pup → …`) when a multi-pup collaboration is initiated

---

## [0.1.2](https://github.com/openpup/openpup/compare/v0.1.1...v0.1.2) — 2026-03-20

### Added

- **UI prototype & design spec** — `docs/openpup-prototype.html` interactive prototype and `docs/openpup-ui-design-prompt.md` design reference added to repo
- **README screenshots** — app interface screenshot added to `README.md`, `README.zh.md`, `README.en.md`

### Fixed

- **Duplicate error bubbles** — `stream_error` event was emitted twice on API failure (once in `do_stream` map_err, once in outer handler); removed the redundant emit so only one error message appears
- **MCP / Pup delete button unresponsive** — `window.confirm()` is silently blocked in Tauri WebView; replaced with inline two-step confirmation (click Delete → confirm / cancel) in `McpSettings` and `PupManager`
- **Chinese IME Enter sends message mid-composition** — added `!e.nativeEvent.isComposing` guard to `onKeyDown` so candidate-selection Enter is ignored

### Changed

- **Font family** — body font aligned with prototype: `-apple-system, 'Helvetica Neue', sans-serif`; mono font updated to `'SF Mono', 'Fira Code', 'Courier New', monospace`
- **Font weight** — removed all `font-weight: 600` violations (`PackChannel` artifact name, channel title); only 400 and 500 used throughout, per design spec
- **Chat content centering** — messages and input bar constrained to `max-width: 720px` centered in the main area for better visual focus
- **Timeline font sizes** — meta label 13 px → 11 px; description 15 px → 12.5 px, matching prototype
- **PermissionDialog typography** — title 14 px → 13 px; detail text 13 px → 11.5 px with line-height 1.7
- **Streaming routing feedback** — `stream_activity "routing"` event emitted immediately before `classify_intent` LLM call so users see a progress indicator rather than a blank thinking state
- **Skills directory** — `skills/personal/` subdirectory flattened; `daily_summary.toml` and `weekly_summary.toml` moved to `skills/` root
- **Removed built-in skills** — `browser_control` and `skill_vetting` removed from `skills/core/`
- **Onboarding header** — removed undefined `font-display` class, invalid `fontVariationSettings`, and faux-italic on Chinese text; simplified to logo + tertiary tagline

---

## [0.1.1](https://github.com/openpup/openpup/compare/v0.1.0...v0.1.1) — 2026-03-19

### Added

- **Skills as first-class LLM tools** — every enabled skill is injected into Alpha's tool loop as `skill__<name>`; LLM can invoke skills directly in the same turn without user re-routing
- **Skill auto-discovery after LLM writes files** — `SkillRegistry.refresh()` re-scans `scan_roots` directories after any `file_write` or `shell_exec` tool call; newly written skills are live in the same conversation without restart
- **`task_update` built-in tool** — agents mark tasks `in_progress` / `done` / `failed` via tool call; task IDs and instructions injected into system prompt
- **Detailed activity stream** — each tool call emits a specific kind (`shell`, `file_read`, `file_write`, `http`, `memory`, `task`, `mcp`, `skill`) with an abbreviated human-readable label (command / path / URL)
- **enc2 API key encryption** — `api_key` in `config.toml` encrypted at rest with AES-256-GCM; keystore at `~/.openpup/.keystore` (mode 0600); transparent on load/save
- **`get_safe_config` Tauri command** — exposes LLM model, provider and skill search paths without leaking the API key
- **SkillHub / ClawHub format support** — `register_from_dir` recognises directories with `SKILL.md` + optional `_meta.json` alongside native TOML
- **Onboarding auto-scroll** — left conversation panel scrolls to the latest message automatically
- **`shell_exec` working directory** — defaults to `~/.openpup/` so relative paths in LLM shell commands resolve correctly
- **Skills path initialisation** — default `~/.openpup/skills/` directory created and added to `search_paths` on first onboarding completion

### Fixed

- **Onboarding stuck on "complete"** — save order corrected: LLM provider configured before `save_onboarding_data` triggers embedding, eliminating the API timeout that froze the step
- **Skills not appearing in Timeline** — `record_skill_run` / `complete_skill_run` now called in `alpha.rs` when routing via `skill:<name>`, not only from the scheduler
- **Tasks always showing "pending"** — agent tool loop handles `task_update` calls; system prompt includes task IDs and explicit instructions to update status on start and completion
- **Frontend abort not stopping backend** — `chat_with_tools` wrapped in `tokio::select!` polling the abort flag every 100 ms; abort also checked between each tool in the tool chain
- **Skill loading after restart** — startup now calls `register_from_dir` (updates both `skills` and `installed` maps) instead of `load_from_dir`; skills are visible to routing immediately on launch
- **MCP/skill tool name validation** — tool names sanitized to `^[a-zA-Z0-9_-]+$` before sending to API; `fn_name_map` reverse-lookup in `MCPOrchestrator` restores original server/tool names for dispatch; fixes 400 errors from DeepSeek and other strict providers

### Changed

- App header, sidebar, chat bubbles and input bar visual style unified with Onboarding (gradients, `backdrop-blur`, rounded bubbles)
- Activity steps in streaming view: latest step at full opacity, previous steps faded to 40%; per-kind icons and colours replace the single `⚙` glyph

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

