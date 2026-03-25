# Changelog

All notable changes to OpenPup will be documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased](https://github.com/openpup/openpup/compare/v0.1.8...HEAD)

### Planned — v0.2.0: Configurable Organization OS

Roadmap 2.0 is tracked in `docs/roadmap2.0.md`.

#### Product Direction

- **Configurable organization model** — support user-defined organizations, units, roles, mandates, and operating protocols rather than a fixed "AI company" template
- **Persistent role ownership** — upgrade pups from per-task specialists into long-lived organizational roles with ongoing responsibilities and memory
- **Escalation-based execution** — reserve `Pack Channel` / DAG coordination for complex work, while simpler work can stay within a single role or handoff chain
- **External operating surfaces** — evolve Telegram into a lightweight control surface and Discord into a richer collaborative surface for visible multi-role interaction

#### System Capabilities

- **Role inboxes and work queues** — give each role a persistent backlog for routines, reactive triggers, escalations, and directed work
- **Routines and triggers** — add cadence-driven reviews and event-driven task creation so the system can operate proactively within defined boundaries
- **Organization memory layers** — separate personal, organizational, unit, role, and task memory so longer-running structures remain coherent
- **Governance and approvals** — introduce configurable decision, escalation, and approval policies so different organization types can operate safely

---

## [0.1.8](https://github.com/openpup/openpup/compare/v0.1.7...v0.1.8) — 2026-03-25

### Added

- **Streaming tool-call path** — agent tool loops can now consume tool-capable LLM responses through a streaming API, forwarding text tokens live while reconstructing tool calls after the stream completes.
- **Pre-connect abort handling** — streaming tool-call requests now honor abort signals even before the HTTP stream opens, so cancellation works during slow time-to-first-byte cases.
- **Skill prompt activation model** — skills can now be activated by injecting their prompt and permissions into the ongoing conversation, allowing Alpha to reuse skill instructions without always spawning a separate standalone loop.

### Changed

- **Skill permissions granularity** — file system access is now split into `file_read` and `file_write` across skills, pups, primitive tools, CLI output, and permission editing surfaces.
- **Skill identity and discovery** — SkillHub directory names are treated as canonical skill identifiers, and activated skills can expand the available tool set by unioning their permissions into the current run.
- **Release baseline** — desktop app metadata, npm package version, workspace crates, and lockfiles are aligned to `0.1.8`.

---

## [0.1.7](https://github.com/openpup/openpup/compare/v0.1.6...v0.1.7) — 2026-03-25

### Added

- **Real context visibility** — the desktop header now shows real per-pup prompt token usage against the active model context window instead of a rough estimate.
- **Adaptive skill discovery** — when many skills are enabled, Alpha now exposes a compact `discover_skills` catalog flow so installed skills remain usable without exhausting the model context window.
- **Current-time context injection** — the agent system prompt now includes the local date, weekday, and timezone so time-sensitive reasoning stays grounded.
- **Pack Channel abort control** — owners can terminate a review-stage Pack Channel directly from the operator UI, with the decision propagated through the command layer and workflow state.
- **Linux titlebar controls** — custom Linux window minimize, maximize, restore, and close controls were added for overlay titlebar layouts.

### Changed

- **Context guardrails** — agent runs now trim oversized context before calls, cache skill tool schemas by registry generation, and trigger compression based on real token usage when available.
- **Tool output budgeting** — primitive tool responses and in-loop tool results now scale truncation to the active model context window, preserving both the head and tail of long outputs to retain important errors.
- **Release baseline** — desktop app metadata, npm package version, workspace crates, and lockfiles are aligned to `0.1.7`.

---

## [0.1.6](https://github.com/openpup/openpup/compare/v0.1.5...v0.1.6) — 2026-03-24

### Added

- **Daemon-first runtime mode** — added a shared headless runtime with a dedicated `openpupd` daemon flow so desktop-independent operation can run continuously.
- **Token usage monitoring** — introduced token accounting and guardrails to improve observability and control of model context growth.

### Changed

- **Agent context loading strategy** — skill prompt payloads are now loaded lazily at execution time to reduce steady-state context pressure.
- **Runtime bootstrap consistency** — startup and log packaging paths are unified across desktop and headless entry points.
- **Release baseline** — workspace crate versions, Tauri bundle metadata, and npm package version are aligned to `0.1.6`.

### Fixed

- **Skill registry doctest parsing** — `SKILL.md` format example in `parse_skill_hub_dir` docs is now marked as text to avoid Rust doctest compilation failures.

---

## [0.1.5](https://github.com/openpup/openpup/compare/v0.1.3...v0.1.5) — 2026-03-22

### Added

- **WeChat bridge integration** — openpup now includes a first-party WeChat bridge with QR login, persisted account state, reconnect handling, and desktop configuration flow
- **Structured Pack Channel review events** — objections, review comments, change requests, resumes, and workflow state are now first-class persisted channel records rather than ad-hoc prompt text
- **Review workflow state sync** — active channels now persist `awaiting_review`, layer index, review round, user-blocked state, and related workflow metadata across backend and frontend
- **Pup visual metadata utility** — pup display labels and accent colors are now derived from a shared frontend utility so chat, channel, and timeline surfaces stay visually consistent

### Fixed

- **Blocked downstream work flowing forward** — downstream pups now receive a stronger review contract and can raise explicit structured review objections instead of packaging unusable upstream context as a normal result
- **Review continue semantics** — continuing from a blocked review now resumes the objecting pup so it can complete its current task, instead of rerunning the whole layer or skipping ahead in the DAG
- **Pack Channel activity visibility** — channel timelines now include richer intermediate activity entries so users can see what pups are doing between `started` and `done`
- **Review card information density** — review entries and controls were compressed so review-heavy channels remain readable without oversized cards or input surfaces
- **Pup identity mismatches across surfaces** — pack channel, chat, and timeline color/name handling now stay in sync for `alpha`, `you`, built-in pups, and custom pups

### Changed

- **Pack Channel review UX** — review requests now surface concise metadata, friendlier action labels, and an explicit operator panel for comment / request changes / continue
- **Pack Channel execution gating** — waiting-for-review channels no longer trip execution timeout monitoring while a human decision is pending
- **Version baseline** — desktop app, Tauri bundle, npm workspace, and CLI package versions are aligned for the `0.1.5` release line

---

## [0.1.3](https://github.com/openpup/openpup/compare/v0.1.2...v0.1.3) — 2026-03-22

### Added

- **Pack Channel runtime** — multi-pup DAG execution now creates persisted channels, plans, statuses, and live message history instead of staying as a static mock
- **Context Inspector component** — extracted the right-side inspector into a dedicated component while preserving the `Pack Channel` experience
- **External Bridge workspace** — added a dedicated `Bridge` navigation page and `BridgeSettings` UI for Telegram / Discord / Slack bridge configuration
- **Per-bridge proxy settings** — each bridge can now store its own proxy URL in config, with Telegram applying it to outbound and polling HTTP clients
- **Telegram bridge progress relay** — Telegram now receives richer progress updates for routed collaboration, layer execution, per-pup completion, aggregation, and stop acknowledgement
- **Bridge-to-channel mapping** — bridge-triggered collaboration now creates real `Pack Channel` records so work is visible both in chat surfaces and in the desktop UI

### Fixed

- **Unicode truncation panic** — replaced byte slicing with character-safe truncation in bridge result formatting and other high-risk output preview paths
- **Bridge memory persistence** — bridge conversations now enter the regular post-processing path so conversation history, diary updates, memory extraction, and task creation all stay in sync
- **Telegram final send validation** — Telegram bridge now checks HTTP status and Bot API payload success instead of assuming `sendMessage` worked
- **Bridge status visibility** — Telegram connection status now updates on polling success, inbound activity, and non-timeout send/poll errors so manual refresh reflects real backend state
- **Bridge stop control** — stop phrases such as `stop`, `cancel`, `停止`, and `停止吧` now flip the shared abort flag and send an explicit stop acknowledgement
- **Bridge collaboration observability** — messages classified as `channel:*` no longer disappear into an internal-only path; they now produce actual channel records and visible progress

### Changed

- **Bridge polling cadence** — the bridge status indicator now refreshes every 5 seconds instead of every 4 seconds
- **Settings information architecture** — bridge configuration was moved out of the general Settings page into its own navigation entry to keep operational controls grouped together
- **Release planning** — the next major planning track is now documented as `Roadmap 2.0`, focused on evolving openpup into a configurable organization operating system

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

