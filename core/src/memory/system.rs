use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Pool, Row, Sqlite,
};

use crate::bridge::types::{
    BridgeConnectionState, BridgeConnectionStatus, InboundMessage, OutboundMessage,
};
use crate::channel::types::{
    ChannelMessageRecord, ChannelRecord, ChannelWorkflowState, DelegationPlan,
};
use crate::conversation::types::{
    ConversationMemberRecord, ConversationMessageRecord, ConversationSpaceRecord,
    ConversationTransportRecord,
};
use crate::llm::client::LlmClient;

#[derive(Clone)]
pub struct MemorySystem {
    pool: Pool<Sqlite>,
    llm: Arc<LlmClient>,
}

fn normalize_mention_key(value: &str, fallback: &str) -> String {
    let mut key = String::new();
    let mut last_separator = false;
    for ch in value
        .trim()
        .trim_start_matches('@')
        .trim_start_matches('＠')
        .chars()
    {
        if ch.is_alphanumeric() {
            key.extend(ch.to_lowercase());
            last_separator = false;
        } else if (ch == '-' || ch == '_' || ch == '.' || ch == '/' || ch == ':')
            && !last_separator
            && !key.is_empty()
        {
            key.push(if ch == ':' { '-' } else { ch });
            last_separator = true;
        } else if !last_separator && !key.is_empty() {
            key.push('-');
            last_separator = true;
        }
    }
    let key = key
        .trim_matches(|ch| matches!(ch, '-' | '_' | '.' | '/'))
        .to_string();
    if key.is_empty() {
        fallback.to_string()
    } else {
        key
    }
}

impl MemorySystem {
    pub async fn new(db_path: &str, llm: Arc<LlmClient>) -> Result<Self> {
        // Ensure parent directory exists before connecting
        let path = std::path::Path::new(db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(10));

        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect_with(opts)
            .await?;

        let this = Self { pool, llm };
        this.init_schema().await?;
        Ok(this)
    }

    async fn init_schema(&self) -> Result<()> {
        // conversations
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS conversations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
        content TEXT NOT NULL,
        timestamp INTEGER NOT NULL,
        metadata TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add pup column if not present (idempotent — error = already exists)
        let _ =
            sqlx::query("ALTER TABLE conversations ADD COLUMN pup TEXT NOT NULL DEFAULT 'alpha'")
                .execute(&self.pool)
                .await;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_conversations_timestamp
      ON conversations(timestamp);
      "#,
        )
        .execute(&self.pool)
        .await?;

        // long_term_memory
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS long_term_memory (
        id TEXT PRIMARY KEY,
        content TEXT NOT NULL,
        memory_type TEXT,
        importance REAL DEFAULT 0.5,
        created_at INTEGER NOT NULL,
        last_accessed INTEGER,
        access_count INTEGER DEFAULT 0,
        metadata TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_long_term_memory_created_at
      ON long_term_memory(created_at);
      "#,
        )
        .execute(&self.pool)
        .await?;

        // tasks
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        description TEXT NOT NULL,
        assigned_pup TEXT,
        status TEXT,
        created_at INTEGER NOT NULL,
        completed_at INTEGER,
        result TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // contacts
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS contacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        email TEXT,
        relationship TEXT,
        notes TEXT,
        last_interaction INTEGER
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // embedding vectors stored as JSON blobs (avoids sqlite-vec extension requirement)
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS memory_embeddings (
        memory_id TEXT PRIMARY KEY,
        embedding TEXT NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // context summary — one row per pup storing a rolling compressed summary
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS context_summaries (
        id INTEGER PRIMARY KEY,
        summary TEXT NOT NULL,
        covers_through_row INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add pup column to context_summaries (idempotent, legacy)
        let _ = sqlx::query(
            "ALTER TABLE context_summaries ADD COLUMN pup TEXT NOT NULL DEFAULT 'alpha'",
        )
        .execute(&self.pool)
        .await;

        // v2: add scope column — 'global' for shared summary, or pup key for per-agent
        let _ = sqlx::query(
            "ALTER TABLE context_summaries ADD COLUMN scope TEXT NOT NULL DEFAULT 'global'",
        )
        .execute(&self.pool)
        .await;

        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_context_summaries_scope ON context_summaries(scope)",
        )
        .execute(&self.pool)
        .await?;

        // Migration: add pup index on conversations
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversations_pup ON conversations(pup, id)")
            .execute(&self.pool)
            .await?;

        // skill execution history
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS skill_runs (
        id TEXT PRIMARY KEY,
        skill_name TEXT NOT NULL,
        triggered_by TEXT NOT NULL,
        started_at INTEGER NOT NULL,
        completed_at INTEGER,
        status TEXT NOT NULL DEFAULT 'running',
        output TEXT,
        job_id TEXT DEFAULT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Migration: add job_id column if not present (idempotent)
        let _ = sqlx::query("ALTER TABLE skill_runs ADD COLUMN job_id TEXT DEFAULT NULL")
            .execute(&self.pool)
            .await;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_skill_runs_started_at
      ON skill_runs(started_at);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_skill_runs_job_id
      ON skill_runs(job_id);
      "#,
        )
        .execute(&self.pool)
        .await?;

        // ── Pack Channel tables ─────────────────────────────────────────────────

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS pack_channels (
        id           TEXT PRIMARY KEY,
        task_id      TEXT NOT NULL,
        title        TEXT NOT NULL,
        status       TEXT NOT NULL DEFAULT 'active',
        current_layer INTEGER,
        review_round INTEGER NOT NULL DEFAULT 0,
        awaiting_user INTEGER NOT NULL DEFAULT 0,
        blocked_reason TEXT,
        created_at   INTEGER NOT NULL,
        completed_at INTEGER,
        created_by   TEXT NOT NULL DEFAULT 'alpha'
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        let _ = sqlx::query("ALTER TABLE pack_channels ADD COLUMN current_layer INTEGER")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query(
            "ALTER TABLE pack_channels ADD COLUMN review_round INTEGER NOT NULL DEFAULT 0",
        )
        .execute(&self.pool)
        .await;
        let _ = sqlx::query(
            "ALTER TABLE pack_channels ADD COLUMN awaiting_user INTEGER NOT NULL DEFAULT 0",
        )
        .execute(&self.pool)
        .await;
        let _ = sqlx::query("ALTER TABLE pack_channels ADD COLUMN blocked_reason TEXT")
            .execute(&self.pool)
            .await;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pack_channels_status_created_at ON pack_channels(status, created_at DESC)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pack_channels_created_at ON pack_channels(created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS channel_messages (
        id            TEXT PRIMARY KEY,
        channel_id    TEXT NOT NULL,
        sender        TEXT NOT NULL,
        content       TEXT NOT NULL,
        msg_type      TEXT NOT NULL DEFAULT 'text',
        artifact_name TEXT,
        status_val    TEXT,
        mentions      TEXT,
        reply_to      TEXT,
        event_payload TEXT,
        timestamp     INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;
        let _ = sqlx::query("ALTER TABLE channel_messages ADD COLUMN event_payload TEXT")
            .execute(&self.pool)
            .await;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_channel_messages_channel
      ON channel_messages(channel_id, timestamp);
      "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_channel_messages_channel_ts_desc
      ON channel_messages(channel_id, timestamp DESC);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS channel_members (
        channel_id  TEXT NOT NULL,
        pup_id      TEXT NOT NULL,
        role        TEXT NOT NULL DEFAULT 'member',
        joined_at   INTEGER NOT NULL,
        PRIMARY KEY (channel_id, pup_id)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS channel_plans (
        channel_id  TEXT PRIMARY KEY,
        plan_json   TEXT NOT NULL,
        updated_at  INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS conversation_spaces (
        id                TEXT PRIMARY KEY,
        kind              TEXT NOT NULL DEFAULT 'group',
        title             TEXT NOT NULL,
        description       TEXT NOT NULL DEFAULT '',
        owner_identity_id TEXT NOT NULL DEFAULT 'owner',
        accent            TEXT NOT NULL DEFAULT '#1D9E75',
        invite_code       TEXT NOT NULL,
        routing_mode      TEXT NOT NULL DEFAULT '显式目标',
        created_at        INTEGER NOT NULL,
        updated_at        INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_conversation_spaces_updated_at
      ON conversation_spaces(updated_at DESC);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS conversation_members (
        id              TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL,
        identity_id     TEXT NOT NULL,
        display_name    TEXT NOT NULL,
        mention_key     TEXT NOT NULL DEFAULT '',
        role            TEXT NOT NULL DEFAULT 'member',
        status          TEXT NOT NULL DEFAULT 'active',
        route_label     TEXT NOT NULL DEFAULT 'app',
        online          INTEGER NOT NULL DEFAULT 0,
        accent          TEXT NOT NULL DEFAULT '#378ADD',
        joined_at       INTEGER NOT NULL,
        last_seen_at    INTEGER,
        network_kind       TEXT,
        transport_inbox_id TEXT,
        client_kind        TEXT,
        client_instance_id TEXT,
        client_display_name TEXT,
        actor_kind         TEXT,
        actor_id           TEXT,
        actor_display_name TEXT,
        via_kind           TEXT,
        via_label          TEXT,
        UNIQUE(conversation_id, identity_id)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;
        let _ = sqlx::query(
            "ALTER TABLE conversation_members ADD COLUMN mention_key TEXT NOT NULL DEFAULT ''",
        )
        .execute(&self.pool)
        .await;
        for column in [
            "network_kind TEXT",
            "transport_inbox_id TEXT",
            "client_kind TEXT",
            "client_instance_id TEXT",
            "client_display_name TEXT",
            "actor_kind TEXT",
            "actor_id TEXT",
            "actor_display_name TEXT",
            "via_kind TEXT",
            "via_label TEXT",
        ] {
            let _ = sqlx::query(&format!(
                "ALTER TABLE conversation_members ADD COLUMN {column}"
            ))
            .execute(&self.pool)
            .await;
        }
        let _ = sqlx::query(
            r#"
            UPDATE conversation_members
            SET mention_key = lower(replace(display_name, ' ', '-'))
            WHERE mention_key = ''
            "#,
        )
        .execute(&self.pool)
        .await;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_conversation_members_conversation
      ON conversation_members(conversation_id, status);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS conversation_messages (
        id                 TEXT PRIMARY KEY,
        conversation_id    TEXT NOT NULL,
        sender_identity_id TEXT NOT NULL,
        sender_route_id    TEXT,
        sender_name        TEXT NOT NULL,
        sender_kind        TEXT NOT NULL DEFAULT 'human',
        route_label        TEXT,
        content            TEXT NOT NULL,
        created_at         INTEGER NOT NULL,
        network_kind       TEXT,
        transport_inbox_id TEXT,
        client_kind        TEXT,
        client_instance_id TEXT,
        client_display_name TEXT,
        actor_kind         TEXT,
        actor_id           TEXT,
        actor_display_name TEXT,
        via_kind           TEXT,
        via_label          TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;
        for column in [
            "network_kind TEXT",
            "transport_inbox_id TEXT",
            "client_kind TEXT",
            "client_instance_id TEXT",
            "client_display_name TEXT",
            "actor_kind TEXT",
            "actor_id TEXT",
            "actor_display_name TEXT",
            "via_kind TEXT",
            "via_label TEXT",
        ] {
            let _ = sqlx::query(&format!(
                "ALTER TABLE conversation_messages ADD COLUMN {column}"
            ))
            .execute(&self.pool)
            .await;
        }

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_conversation_messages_conversation
      ON conversation_messages(conversation_id, created_at);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS conversation_transport_bindings (
        id              TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL,
        kind            TEXT NOT NULL,
        label           TEXT NOT NULL,
        status          TEXT NOT NULL DEFAULT 'active',
        transport_ref   TEXT,
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL,
        UNIQUE(conversation_id, kind, transport_ref)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_conversation_transport_conversation
      ON conversation_transport_bindings(conversation_id, status);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS network_identities (
        id                  TEXT PRIMARY KEY,
        network_kind        TEXT NOT NULL,
        network_identity_id TEXT NOT NULL,
        display_name        TEXT NOT NULL,
        participant_kind    TEXT NOT NULL DEFAULT 'unknown',
        app_kind            TEXT NOT NULL DEFAULT 'unknown',
        agent_key           TEXT,
        capabilities_json   TEXT NOT NULL DEFAULT '{}',
        trust_level         TEXT NOT NULL DEFAULT 'untrusted',
        first_seen_at       INTEGER NOT NULL,
        last_seen_at        INTEGER NOT NULL,
        UNIQUE(network_kind, network_identity_id)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS xmtp_identities (
        id          TEXT PRIMARY KEY,
        inbox_id    TEXT NOT NULL,
        address     TEXT,
        signer_kind TEXT NOT NULL DEFAULT 'helper',
        key_ref     TEXT,
        db_path     TEXT NOT NULL,
        env         TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        UNIQUE(inbox_id, env)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS xmtp_message_map (
        local_message_id     TEXT NOT NULL,
        xmtp_message_id      TEXT NOT NULL,
        xmtp_conversation_id TEXT NOT NULL,
        direction            TEXT NOT NULL,
        created_at           INTEGER NOT NULL,
        PRIMARY KEY(local_message_id, xmtp_message_id)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_xmtp_message_map_remote
      ON xmtp_message_map(xmtp_conversation_id, xmtp_message_id);
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS custom_pups (
        id            TEXT PRIMARY KEY,
        name          TEXT NOT NULL UNIQUE,
        description   TEXT NOT NULL,
        system_prompt TEXT NOT NULL,
        capabilities  TEXT NOT NULL DEFAULT '[]',
        color         TEXT NOT NULL DEFAULT '#888780',
        enabled       INTEGER NOT NULL DEFAULT 1,
        created_at    INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // ── External Bridge tables ─────────────────────────────────────────────

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS external_messages (
        id           TEXT PRIMARY KEY,
        platform     TEXT NOT NULL,
        chat_id      TEXT NOT NULL,
        direction    TEXT NOT NULL CHECK(direction IN ('inbound','outbound')),
        user_id      TEXT,
        message_id   TEXT,
        reply_to_id  TEXT,
        msg_type     TEXT,
        content      TEXT NOT NULL,
        created_at   INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS bridge_connections (
        platform    TEXT PRIMARY KEY,
        status      TEXT NOT NULL DEFAULT 'unconfigured',
        connected   INTEGER NOT NULL DEFAULT 0,
        last_seen   INTEGER,
        error_msg   TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // ── Knowledge Base tables ─────────────────────────────────────────────

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS knowledge_sources (
        id          TEXT PRIMARY KEY,
        title       TEXT NOT NULL,
        source_type TEXT NOT NULL DEFAULT 'file',
        source_path TEXT,
        mime_type   TEXT,
        byte_size   INTEGER,
        tags        TEXT,
        chunk_count INTEGER NOT NULL DEFAULT 0,
        status      TEXT NOT NULL DEFAULT 'pending',
        error_msg   TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS knowledge_chunks (
        id           TEXT PRIMARY KEY,
        source_id    TEXT NOT NULL,
        content      TEXT NOT NULL,
        chunk_index  INTEGER NOT NULL,
        heading_path TEXT,
        char_start   INTEGER,
        char_end     INTEGER,
        embedding    TEXT NOT NULL,
        created_at   INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_knowledge_chunks_source ON knowledge_chunks(source_id)",
        )
        .execute(&self.pool)
        .await?;

        // ── FTS5 full-text index over knowledge chunks (v0.1.10) ────────────
        sqlx::query(
            r#"
      CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_fts
      USING fts5(content, heading_path, chunk_id UNINDEXED, source_id UNINDEXED,
                 content_rowid='rowid', tokenize='unicode61');
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Trigger: auto-populate FTS on chunk insert
        sqlx::query(
            r#"
      CREATE TRIGGER IF NOT EXISTS trg_kb_fts_insert
      AFTER INSERT ON knowledge_chunks
      BEGIN
        INSERT INTO knowledge_chunks_fts(content, heading_path, chunk_id, source_id)
        VALUES (NEW.content, NEW.heading_path, NEW.id, NEW.source_id);
      END;
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Trigger: auto-delete FTS on chunk delete
        sqlx::query(
            r#"
      CREATE TRIGGER IF NOT EXISTS trg_kb_fts_delete
      AFTER DELETE ON knowledge_chunks
      BEGIN
        DELETE FROM knowledge_chunks_fts WHERE chunk_id = OLD.id;
      END;
      "#,
        )
        .execute(&self.pool)
        .await?;

        // Back-fill FTS for any pre-existing chunks (v0.1.9 → v0.1.10 migration).
        // Only runs if FTS table is empty.
        let fts_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_chunks_fts")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if fts_count == 0 {
            sqlx::query(
                r#"
          INSERT INTO knowledge_chunks_fts(content, heading_path, chunk_id, source_id)
          SELECT content, heading_path, id, source_id FROM knowledge_chunks;
          "#,
            )
            .execute(&self.pool)
            .await?;
        }

        // ── Knowledge Graph tables (v0.1.11) ────────────────────────────────

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS kg_entities (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        entity_type TEXT NOT NULL
                    CHECK(entity_type IN
                      ('person','project','concept','tool','org','file','other')),
        description TEXT,
        source_id   TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        UNIQUE(name, entity_type)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS kg_relations (
        id          TEXT PRIMARY KEY,
        from_id     TEXT NOT NULL,
        to_id       TEXT NOT NULL,
        relation    TEXT NOT NULL,
        source_id   TEXT,
        confidence  REAL NOT NULL DEFAULT 0.8,
        created_at  INTEGER NOT NULL,
        UNIQUE(from_id, to_id, relation)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_rel_from ON kg_relations(from_id)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_rel_to ON kg_relations(to_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS kg_chunk_entities (
        chunk_id   TEXT NOT NULL,
        entity_id  TEXT NOT NULL,
        PRIMARY KEY (chunk_id, entity_id)
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        // ── Memory upgrade (v0.1.12): conflict tracking + Weibull decay fields ───

        // Conflict tracking: which memory superseded this one
        let _ = sqlx::query(
            "ALTER TABLE long_term_memory ADD COLUMN superseded_by TEXT REFERENCES long_term_memory(id)",
        )
        .execute(&self.pool)
        .await;

        // Validity window (UNIX timestamp, NULL = permanent)
        let _ = sqlx::query("ALTER TABLE long_term_memory ADD COLUMN valid_until INTEGER")
            .execute(&self.pool)
            .await;

        // Source conversation tracking
        let _ = sqlx::query(
            "ALTER TABLE long_term_memory ADD COLUMN extracted_from INTEGER REFERENCES conversations(id)",
        )
        .execute(&self.pool)
        .await;

        // LLM extraction confidence
        let _ = sqlx::query(
            "ALTER TABLE long_term_memory ADD COLUMN confidence REAL NOT NULL DEFAULT 0.8",
        )
        .execute(&self.pool)
        .await;

        // Cumulative access count (never reset, for Weibull reinforcement)
        let _ = sqlx::query(
            "ALTER TABLE long_term_memory ADD COLUMN access_count_total INTEGER NOT NULL DEFAULT 0",
        )
        .execute(&self.pool)
        .await;

        // v2: role-scoped memory — 'global' memories visible to all agents,
        // per-role memories visible only to that agent.
        let _ = sqlx::query(
            "ALTER TABLE long_term_memory ADD COLUMN role_scope TEXT NOT NULL DEFAULT 'global'",
        )
        .execute(&self.pool)
        .await;

        // FTS5 full-text index for long_term_memory
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                content,
                content='long_term_memory',
                content_rowid='rowid',
                tokenize='unicode61 remove_diacritics 2'
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Back-fill FTS for existing memories (one-time migration)
        let mem_fts_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_fts")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if mem_fts_count == 0 {
            let _ = sqlx::query(
                "INSERT INTO memory_fts(rowid, content)
                 SELECT rowid, content FROM long_term_memory",
            )
            .execute(&self.pool)
            .await;
        }

        // FTS sync triggers
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS ltm_fts_insert
                AFTER INSERT ON long_term_memory BEGIN
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS ltm_fts_update
                AFTER UPDATE OF content ON long_term_memory BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                    VALUES('delete', old.rowid, old.content);
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS ltm_fts_delete
                AFTER DELETE ON long_term_memory BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                    VALUES('delete', old.rowid, old.content);
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        // access_count_total auto-increment trigger
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS ltm_access_total
                AFTER UPDATE OF last_accessed ON long_term_memory
                WHEN new.last_accessed > old.last_accessed BEGIN
                UPDATE long_term_memory
                SET access_count_total = access_count_total + 1
                WHERE id = new.id;
            END;
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Active rules view (for forced injection)
        sqlx::query(
            r#"
            CREATE VIEW IF NOT EXISTS active_rules AS
            SELECT *
            FROM long_term_memory
            WHERE memory_type = 'rule'
              AND superseded_by IS NULL
              AND (valid_until IS NULL OR valid_until > strftime('%s', 'now'))
            ORDER BY importance DESC;
            "#,
        )
        .execute(&self.pool)
        .await?;

        // ── Message feedback (v0.1.13) ─────────────────────────────────────────
        sqlx::query(
            r#"
      CREATE TABLE IF NOT EXISTS message_feedback (
        message_id TEXT PRIMARY KEY,
        channel_id TEXT,
        feedback   TEXT NOT NULL CHECK(feedback IN ('up','down')),
        created_at INTEGER NOT NULL
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Expose the raw pool for retriever/injector/extractor sub-systems.
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Expose the LLM client for sub-systems that need embeddings.
    pub fn llm(&self) -> &Arc<LlmClient> {
        &self.llm
    }

    // ── Pack Channel ──────────────────────────────────────────────────────────

    pub async fn create_channel(
        &self,
        id: &str,
        task_id: &str,
        title: &str,
        members: &[&str],
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
      "INSERT INTO pack_channels (id, task_id, title, status, created_at) VALUES (?1,?2,?3,'active',?4)",
    )
    .bind(id)
    .bind(task_id)
    .bind(title)
    .bind(now)
    .execute(&self.pool)
    .await?;

        for pup in members {
            sqlx::query(
        "INSERT OR IGNORE INTO channel_members (channel_id, pup_id, role, joined_at) VALUES (?1,?2,'member',?3)",
      )
      .bind(id)
      .bind(*pup)
      .bind(now)
      .execute(&self.pool)
      .await?;
        }
        Ok(())
    }

    pub async fn post_channel_message(
        &self,
        id: &str,
        channel_id: &str,
        sender: &str,
        content: &str,
        msg_type: &str,
        artifact_name: Option<&str>,
        status_val: Option<&str>,
        mentions: &[&str],
        reply_to: Option<&str>,
        event_payload: Option<&serde_json::Value>,
    ) -> Result<()> {
        let mentions_json = serde_json::to_string(mentions).unwrap_or_default();
        let event_payload_json = event_payload.map(serde_json::to_string).transpose()?;
        sqlx::query(
            r#"INSERT INTO channel_messages
         (id, channel_id, sender, content, msg_type, artifact_name, status_val, mentions, reply_to, event_payload, timestamp)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"#,
        )
        .bind(id)
        .bind(channel_id)
        .bind(sender)
        .bind(content)
        .bind(msg_type)
        .bind(artifact_name)
        .bind(status_val)
        .bind(&mentions_json)
        .bind(reply_to)
        .bind(event_payload_json)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Conversation Spaces ─────────────────────────────────────────────────

    pub async fn create_conversation_space(
        &self,
        title: &str,
        description: Option<&str>,
        owner_identity_id: Option<&str>,
        accent: Option<&str>,
    ) -> Result<ConversationSpaceRecord> {
        let now = Utc::now().timestamp();
        let id = format!("conv_{}", uuid::Uuid::new_v4());
        let invite_seed = uuid::Uuid::new_v4().simple().to_string();
        let invite_code = format!("G-{}", &invite_seed[..4].to_ascii_uppercase());
        let owner_identity_id = owner_identity_id.unwrap_or("owner");
        let description = description.unwrap_or("Group Space · local-first");
        let accent = accent.unwrap_or("#1D9E75");

        sqlx::query(
            r#"
            INSERT INTO conversation_spaces
              (id, kind, title, description, owner_identity_id, accent, invite_code, routing_mode, created_at, updated_at)
            VALUES (?1, 'group', ?2, ?3, ?4, ?5, ?6, '显式目标', ?7, ?7)
            "#,
        )
        .bind(&id)
        .bind(title)
        .bind(description)
        .bind(owner_identity_id)
        .bind(accent)
        .bind(&invite_code)
        .bind(now)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO conversation_members
              (id, conversation_id, identity_id, display_name, mention_key, role, status, route_label, online, accent, joined_at, last_seen_at)
            VALUES (?1, ?2, ?3, 'Ben', 'ben', 'owner', 'active', 'app', 1, '#378ADD', ?4, ?4)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&id)
        .bind(owner_identity_id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO conversation_members
              (id, conversation_id, identity_id, display_name, mention_key, role, status, route_label, online, accent, joined_at, last_seen_at)
            VALUES (?1, ?2, 'agent_alpha', 'Alpha', 'alpha', 'agent', 'active', 'local agent', 1, ?3, ?4, ?4)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&id)
        .bind(accent)
        .bind(now)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO conversation_transport_bindings
              (id, conversation_id, kind, label, status, transport_ref, created_at, updated_at)
            VALUES (?1, ?2, 'local', 'Local', 'active', NULL, ?3, ?3)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.post_conversation_message(
            &id,
            "system",
            None,
            "系统",
            "system",
            None,
            &format!("Conversation Space「{}」已创建。Alpha 已加入。", title),
        )
        .await?;

        self.get_conversation_space(&id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created conversation space not found"))
    }

    pub async fn list_conversation_spaces(&self) -> Result<Vec<ConversationSpaceRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.kind, s.title, s.description, s.owner_identity_id,
                   s.created_at, s.updated_at, s.accent, s.invite_code, s.routing_mode,
                   COALESCE(mem.member_count, 0) AS member_count,
                   COALESCE(msg.last_message_at, s.updated_at) AS sort_at
            FROM conversation_spaces s
            LEFT JOIN (
              SELECT conversation_id, COUNT(*) AS member_count
              FROM conversation_members
              WHERE status = 'active'
              GROUP BY conversation_id
            ) mem ON mem.conversation_id = s.id
            LEFT JOIN (
              SELECT conversation_id, MAX(created_at) AS last_message_at
              FROM conversation_messages
              GROUP BY conversation_id
            ) msg ON msg.conversation_id = s.id
            ORDER BY sort_at DESC, s.created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut spaces = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get("id");
            spaces.push(ConversationSpaceRecord {
                id: id.clone(),
                kind: row.get("kind"),
                title: row.get("title"),
                description: row.get("description"),
                owner_identity_id: row.get("owner_identity_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                accent: row.get("accent"),
                invite_code: row.get("invite_code"),
                routing_mode: row.get("routing_mode"),
                member_count: row.get("member_count"),
                unread: 0,
                transports: self.list_conversation_transports(&id).await?,
            });
        }
        Ok(spaces)
    }

    pub async fn get_conversation_space(
        &self,
        conversation_id: &str,
    ) -> Result<Option<ConversationSpaceRecord>> {
        let row = sqlx::query(
            r#"
            SELECT s.id, s.kind, s.title, s.description, s.owner_identity_id,
                   s.created_at, s.updated_at, s.accent, s.invite_code, s.routing_mode,
                   COALESCE(mem.member_count, 0) AS member_count
            FROM conversation_spaces s
            LEFT JOIN (
              SELECT conversation_id, COUNT(*) AS member_count
              FROM conversation_members
              WHERE status = 'active'
              GROUP BY conversation_id
            ) mem ON mem.conversation_id = s.id
            WHERE s.id = ?1
            "#,
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(ConversationSpaceRecord {
            id: row.get("id"),
            kind: row.get("kind"),
            title: row.get("title"),
            description: row.get("description"),
            owner_identity_id: row.get("owner_identity_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            accent: row.get("accent"),
            invite_code: row.get("invite_code"),
            routing_mode: row.get("routing_mode"),
            member_count: row.get("member_count"),
            unread: 0,
            transports: self.list_conversation_transports(conversation_id).await?,
        }))
    }

    pub async fn find_conversation_space(
        &self,
        target: &str,
    ) -> Result<Option<ConversationSpaceRecord>> {
        let target = target.trim();
        if target.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query(
            r#"
            SELECT id
            FROM conversation_spaces
            WHERE id = ?1
               OR title = ?1
               OR invite_code = ?1
               OR lower(title) = lower(?1)
               OR lower(invite_code) = lower(?1)
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(target)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id: String = row.get("id");
                self.get_conversation_space(&id).await
            }
            None => Ok(None),
        }
    }

    pub async fn list_conversation_members(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ConversationMemberRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, identity_id, display_name, mention_key, role, status,
                   route_label, online, accent, joined_at, last_seen_at,
                   network_kind, transport_inbox_id, client_kind, client_instance_id,
                   client_display_name, actor_kind, actor_id, actor_display_name,
                   via_kind, via_label
            FROM conversation_members
            WHERE conversation_id = ?1
            ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'agent' THEN 2 ELSE 3 END,
                     joined_at ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ConversationMemberRecord {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                identity_id: row.get("identity_id"),
                display_name: row.get("display_name"),
                mention_key: row.get("mention_key"),
                role: row.get("role"),
                status: row.get("status"),
                route_label: row.get("route_label"),
                online: row.get::<i64, _>("online") != 0,
                accent: row.get("accent"),
                joined_at: row.get("joined_at"),
                last_seen_at: row.get("last_seen_at"),
                network_kind: row.get("network_kind"),
                transport_inbox_id: row.get("transport_inbox_id"),
                client_kind: row.get("client_kind"),
                client_instance_id: row.get("client_instance_id"),
                client_display_name: row.get("client_display_name"),
                actor_kind: row.get("actor_kind"),
                actor_id: row.get("actor_id"),
                actor_display_name: row.get("actor_display_name"),
                via_kind: row.get("via_kind"),
                via_label: row.get("via_label"),
            })
            .collect())
    }

    pub async fn ensure_conversation_member(
        &self,
        conversation_id: &str,
        identity_id: &str,
        display_name: &str,
        mention_key: Option<&str>,
        route_label: &str,
        role: &str,
    ) -> Result<ConversationMemberRecord> {
        let now = Utc::now().timestamp();
        let mention_key = mention_key
            .map(|value| normalize_mention_key(value, identity_id))
            .unwrap_or_else(|| normalize_mention_key(display_name, identity_id));
        let existing = sqlx::query(
            r#"
            SELECT id, conversation_id, identity_id, display_name, mention_key, role, status,
                   route_label, online, accent, joined_at, last_seen_at,
                   network_kind, transport_inbox_id, client_kind, client_instance_id,
                   client_display_name, actor_kind, actor_id, actor_display_name,
                   via_kind, via_label
            FROM conversation_members
            WHERE conversation_id = ?1 AND identity_id = ?2
            "#,
        )
        .bind(conversation_id)
        .bind(identity_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = existing {
            sqlx::query(
                r#"
                UPDATE conversation_members
                SET status = 'active', display_name = ?1, mention_key = ?2, route_label = ?3, last_seen_at = ?4
                WHERE conversation_id = ?5 AND identity_id = ?6
                "#,
            )
            .bind(display_name)
            .bind(&mention_key)
            .bind(route_label)
            .bind(now)
            .bind(conversation_id)
            .bind(identity_id)
            .execute(&self.pool)
            .await?;

            return Ok(ConversationMemberRecord {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                identity_id: row.get("identity_id"),
                display_name: display_name.to_string(),
                mention_key,
                role: row.get("role"),
                status: "active".to_string(),
                route_label: route_label.to_string(),
                online: row.get::<i64, _>("online") != 0,
                accent: row.get("accent"),
                joined_at: row.get("joined_at"),
                last_seen_at: Some(now),
                network_kind: row.get("network_kind"),
                transport_inbox_id: row.get("transport_inbox_id"),
                client_kind: row.get("client_kind"),
                client_instance_id: row.get("client_instance_id"),
                client_display_name: row.get("client_display_name"),
                actor_kind: row.get("actor_kind"),
                actor_id: row.get("actor_id"),
                actor_display_name: row.get("actor_display_name"),
                via_kind: row.get("via_kind"),
                via_label: row.get("via_label"),
            });
        }

        let id = uuid::Uuid::new_v4().to_string();
        let accent = if role == "agent" {
            "#1D9E75"
        } else {
            "#7F77DD"
        };
        sqlx::query(
            r#"
            INSERT INTO conversation_members
              (id, conversation_id, identity_id, display_name, mention_key, role, status, route_label, online, accent, joined_at, last_seen_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, 1, ?8, ?9, ?9)
            "#,
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(identity_id)
        .bind(display_name)
        .bind(&mention_key)
        .bind(role)
        .bind(route_label)
        .bind(accent)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(ConversationMemberRecord {
            id,
            conversation_id: conversation_id.to_string(),
            identity_id: identity_id.to_string(),
            display_name: display_name.to_string(),
            mention_key,
            role: role.to_string(),
            status: "active".to_string(),
            route_label: route_label.to_string(),
            online: true,
            accent: accent.to_string(),
            joined_at: now,
            last_seen_at: Some(now),
            network_kind: None,
            transport_inbox_id: None,
            client_kind: None,
            client_instance_id: None,
            client_display_name: None,
            actor_kind: None,
            actor_id: None,
            actor_display_name: None,
            via_kind: None,
            via_label: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn ensure_conversation_member_with_sender_meta(
        &self,
        conversation_id: &str,
        identity_id: &str,
        display_name: &str,
        mention_key: Option<&str>,
        route_label: &str,
        role: &str,
        network_kind: Option<&str>,
        transport_inbox_id: Option<&str>,
        client_kind: Option<&str>,
        client_instance_id: Option<&str>,
        client_display_name: Option<&str>,
        actor_kind: Option<&str>,
        actor_id: Option<&str>,
        actor_display_name: Option<&str>,
        via_kind: Option<&str>,
        via_label: Option<&str>,
    ) -> Result<ConversationMemberRecord> {
        let member = self
            .ensure_conversation_member(
                conversation_id,
                identity_id,
                display_name,
                mention_key,
                route_label,
                role,
            )
            .await?;
        sqlx::query(
            r#"
            UPDATE conversation_members
            SET network_kind = ?1,
                transport_inbox_id = ?2,
                client_kind = ?3,
                client_instance_id = ?4,
                client_display_name = ?5,
                actor_kind = ?6,
                actor_id = ?7,
                actor_display_name = ?8,
                via_kind = ?9,
                via_label = ?10
            WHERE conversation_id = ?11 AND identity_id = ?12
            "#,
        )
        .bind(network_kind)
        .bind(transport_inbox_id)
        .bind(client_kind)
        .bind(client_instance_id)
        .bind(client_display_name)
        .bind(actor_kind)
        .bind(actor_id)
        .bind(actor_display_name)
        .bind(via_kind)
        .bind(via_label)
        .bind(conversation_id)
        .bind(identity_id)
        .execute(&self.pool)
        .await?;
        Ok(ConversationMemberRecord {
            network_kind: network_kind.map(str::to_string),
            transport_inbox_id: transport_inbox_id.map(str::to_string),
            client_kind: client_kind.map(str::to_string),
            client_instance_id: client_instance_id.map(str::to_string),
            client_display_name: client_display_name.map(str::to_string),
            actor_kind: actor_kind.map(str::to_string),
            actor_id: actor_id.map(str::to_string),
            actor_display_name: actor_display_name.map(str::to_string),
            via_kind: via_kind.map(str::to_string),
            via_label: via_label.map(str::to_string),
            ..member
        })
    }

    pub async fn add_conversation_member(
        &self,
        conversation_id: &str,
        display_name: &str,
        mention_key: Option<&str>,
        route_label: Option<&str>,
        role: Option<&str>,
    ) -> Result<ConversationMemberRecord> {
        let now = Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        let identity_id = format!("member_{}", uuid::Uuid::new_v4());
        let mention_key = normalize_mention_key(mention_key.unwrap_or(display_name), &identity_id);
        let role = role.unwrap_or("member");
        let route_label = route_label.unwrap_or("app");
        let accent = match role {
            "agent" => "#1D9E75",
            "admin" => "#BA7517",
            _ => "#7F77DD",
        };

        sqlx::query(
            r#"
            INSERT INTO conversation_members
              (id, conversation_id, identity_id, display_name, mention_key, role, status, route_label, online, accent, joined_at, last_seen_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, 0, ?8, ?9, NULL)
            "#,
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(&identity_id)
        .bind(display_name)
        .bind(&mention_key)
        .bind(role)
        .bind(route_label)
        .bind(accent)
        .bind(now)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE conversation_spaces SET updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;

        self.post_conversation_message(
            conversation_id,
            "system",
            None,
            "系统",
            "system",
            None,
            &format!("{} 已加入群聊。", display_name),
        )
        .await?;

        Ok(ConversationMemberRecord {
            id,
            conversation_id: conversation_id.to_string(),
            identity_id,
            display_name: display_name.to_string(),
            mention_key,
            role: role.to_string(),
            status: "active".to_string(),
            route_label: route_label.to_string(),
            online: false,
            accent: accent.to_string(),
            joined_at: now,
            last_seen_at: None,
            network_kind: None,
            transport_inbox_id: None,
            client_kind: None,
            client_instance_id: None,
            client_display_name: None,
            actor_kind: None,
            actor_id: None,
            actor_display_name: None,
            via_kind: None,
            via_label: None,
        })
    }

    pub async fn remove_conversation_member(
        &self,
        conversation_id: &str,
        identity_id: &str,
    ) -> Result<()> {
        if identity_id == "owner" || identity_id == "agent_alpha" {
            anyhow::bail!("cannot remove core member");
        }

        let row = sqlx::query(
            r#"
            SELECT display_name
            FROM conversation_members
            WHERE conversation_id = ?1 AND identity_id = ?2
            "#,
        )
        .bind(conversation_id)
        .bind(identity_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            anyhow::bail!("member not found");
        };
        let display_name: String = row.get("display_name");
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            UPDATE conversation_members
            SET status = 'inactive', online = 0, last_seen_at = ?1
            WHERE conversation_id = ?2 AND identity_id = ?3
            "#,
        )
        .bind(now)
        .bind(conversation_id)
        .bind(identity_id)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE conversation_spaces SET updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;

        self.post_conversation_message(
            conversation_id,
            "system",
            None,
            "系统",
            "system",
            None,
            &format!("{display_name} 已离开群聊。"),
        )
        .await?;

        Ok(())
    }

    pub async fn delete_conversation_space(&self, conversation_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM xmtp_message_map WHERE local_message_id IN (SELECT id FROM conversation_messages WHERE conversation_id = ?1)")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM conversation_messages WHERE conversation_id = ?1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM conversation_members WHERE conversation_id = ?1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM conversation_transport_bindings WHERE conversation_id = ?1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM conversation_spaces WHERE id = ?1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_conversation_transports(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ConversationTransportRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT kind, label, status, transport_ref
            FROM conversation_transport_bindings
            WHERE conversation_id = ?1
            ORDER BY CASE status WHEN 'active' THEN 0 ELSE 1 END, label ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ConversationTransportRecord {
                kind: row.get("kind"),
                label: row.get("label"),
                status: row.get("status"),
                transport_ref: row.get("transport_ref"),
            })
            .collect())
    }

    pub async fn bind_conversation_transport(
        &self,
        conversation_id: &str,
        kind: &str,
        label: &str,
        transport_ref: &str,
    ) -> Result<ConversationTransportRecord> {
        let now = Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO conversation_transport_bindings
              (id, conversation_id, kind, label, status, transport_ref, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?6)
            ON CONFLICT(conversation_id, kind, transport_ref) DO UPDATE SET
              label = excluded.label,
              status = 'active',
              updated_at = excluded.updated_at
            "#,
        )
        .bind(id)
        .bind(conversation_id)
        .bind(kind)
        .bind(label)
        .bind(transport_ref)
        .bind(now)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE conversation_spaces SET updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;

        Ok(ConversationTransportRecord {
            kind: kind.to_string(),
            label: label.to_string(),
            status: "active".to_string(),
            transport_ref: Some(transport_ref.to_string()),
        })
    }

    pub async fn find_conversation_by_transport(
        &self,
        kind: &str,
        transport_ref: &str,
    ) -> Result<Option<String>> {
        let row = sqlx::query(
            r#"
            SELECT conversation_id
            FROM conversation_transport_bindings
            WHERE kind = ?1 AND transport_ref = ?2 AND status = 'active'
            LIMIT 1
            "#,
        )
        .bind(kind)
        .bind(transport_ref)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get("conversation_id")))
    }

    pub async fn insert_xmtp_message_map(
        &self,
        local_message_id: &str,
        xmtp_message_id: &str,
        xmtp_conversation_id: &str,
        direction: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO xmtp_message_map
              (local_message_id, xmtp_message_id, xmtp_conversation_id, direction, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(local_message_id)
        .bind(xmtp_message_id)
        .bind(xmtp_conversation_id)
        .bind(direction)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn xmtp_message_seen(
        &self,
        xmtp_conversation_id: &str,
        xmtp_message_id: &str,
    ) -> Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT 1
            FROM xmtp_message_map
            WHERE xmtp_conversation_id = ?1 AND xmtp_message_id = ?2
            LIMIT 1
            "#,
        )
        .bind(xmtp_conversation_id)
        .bind(xmtp_message_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    pub async fn upsert_network_identity(
        &self,
        network_kind: &str,
        network_identity_id: &str,
        display_name: &str,
        participant_kind: &str,
        app_kind: &str,
        agent_key: Option<&str>,
    ) -> Result<String> {
        let now = Utc::now().timestamp();
        let id = format!("net_{}", uuid::Uuid::new_v4());
        sqlx::query(
            r#"
            INSERT INTO network_identities
              (id, network_kind, network_identity_id, display_name, participant_kind,
               app_kind, agent_key, capabilities_json, trust_level, first_seen_at, last_seen_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}', 'untrusted', ?8, ?8)
            ON CONFLICT(network_kind, network_identity_id) DO UPDATE SET
              display_name = excluded.display_name,
              participant_kind = excluded.participant_kind,
              app_kind = excluded.app_kind,
              agent_key = excluded.agent_key,
              last_seen_at = excluded.last_seen_at
            "#,
        )
        .bind(&id)
        .bind(network_kind)
        .bind(network_identity_id)
        .bind(display_name)
        .bind(participant_kind)
        .bind(app_kind)
        .bind(agent_key)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let row = sqlx::query(
            r#"
            SELECT id
            FROM network_identities
            WHERE network_kind = ?1 AND network_identity_id = ?2
            "#,
        )
        .bind(network_kind)
        .bind(network_identity_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn list_conversation_messages(
        &self,
        conversation_id: &str,
        limit: i64,
    ) -> Result<Vec<ConversationMessageRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, sender_identity_id, sender_route_id, sender_name,
                   sender_kind, route_label, content, created_at,
                   network_kind, transport_inbox_id, client_kind, client_instance_id,
                   client_display_name, actor_kind, actor_id, actor_display_name,
                   via_kind, via_label
            FROM conversation_messages
            WHERE conversation_id = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#,
        )
        .bind(conversation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut messages: Vec<ConversationMessageRecord> = rows
            .into_iter()
            .map(|row| ConversationMessageRecord {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                sender_identity_id: row.get("sender_identity_id"),
                sender_route_id: row.get("sender_route_id"),
                sender_name: row.get("sender_name"),
                sender_kind: row.get("sender_kind"),
                route_label: row.get("route_label"),
                content: row.get("content"),
                created_at: row.get("created_at"),
                network_kind: row.get("network_kind"),
                transport_inbox_id: row.get("transport_inbox_id"),
                client_kind: row.get("client_kind"),
                client_instance_id: row.get("client_instance_id"),
                client_display_name: row.get("client_display_name"),
                actor_kind: row.get("actor_kind"),
                actor_id: row.get("actor_id"),
                actor_display_name: row.get("actor_display_name"),
                via_kind: row.get("via_kind"),
                via_label: row.get("via_label"),
            })
            .collect();
        messages.reverse();
        Ok(messages)
    }

    pub async fn post_conversation_message(
        &self,
        conversation_id: &str,
        sender_identity_id: &str,
        sender_route_id: Option<&str>,
        sender_name: &str,
        sender_kind: &str,
        route_label: Option<&str>,
        content: &str,
    ) -> Result<ConversationMessageRecord> {
        self.post_conversation_message_with_sender_meta(
            conversation_id,
            sender_identity_id,
            sender_route_id,
            sender_name,
            sender_kind,
            route_label,
            content,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post_conversation_message_with_sender_meta(
        &self,
        conversation_id: &str,
        sender_identity_id: &str,
        sender_route_id: Option<&str>,
        sender_name: &str,
        sender_kind: &str,
        route_label: Option<&str>,
        content: &str,
        network_kind: Option<&str>,
        transport_inbox_id: Option<&str>,
        client_kind: Option<&str>,
        client_instance_id: Option<&str>,
        client_display_name: Option<&str>,
        actor_kind: Option<&str>,
        actor_id: Option<&str>,
        actor_display_name: Option<&str>,
        via_kind: Option<&str>,
        via_label: Option<&str>,
    ) -> Result<ConversationMessageRecord> {
        let now = Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO conversation_messages
              (id, conversation_id, sender_identity_id, sender_route_id, sender_name, sender_kind, route_label, content, created_at,
               network_kind, transport_inbox_id, client_kind, client_instance_id, client_display_name,
               actor_kind, actor_id, actor_display_name, via_kind, via_label)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(sender_identity_id)
        .bind(sender_route_id)
        .bind(sender_name)
        .bind(sender_kind)
        .bind(route_label)
        .bind(content)
        .bind(now)
        .bind(network_kind)
        .bind(transport_inbox_id)
        .bind(client_kind)
        .bind(client_instance_id)
        .bind(client_display_name)
        .bind(actor_kind)
        .bind(actor_id)
        .bind(actor_display_name)
        .bind(via_kind)
        .bind(via_label)
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE conversation_spaces SET updated_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;

        Ok(ConversationMessageRecord {
            id,
            conversation_id: conversation_id.to_string(),
            sender_identity_id: sender_identity_id.to_string(),
            sender_route_id: sender_route_id.map(str::to_string),
            sender_name: sender_name.to_string(),
            sender_kind: sender_kind.to_string(),
            route_label: route_label.map(str::to_string),
            content: content.to_string(),
            created_at: now,
            network_kind: network_kind.map(str::to_string),
            transport_inbox_id: transport_inbox_id.map(str::to_string),
            client_kind: client_kind.map(str::to_string),
            client_instance_id: client_instance_id.map(str::to_string),
            client_display_name: client_display_name.map(str::to_string),
            actor_kind: actor_kind.map(str::to_string),
            actor_id: actor_id.map(str::to_string),
            actor_display_name: actor_display_name.map(str::to_string),
            via_kind: via_kind.map(str::to_string),
            via_label: via_label.map(str::to_string),
        })
    }

    // ── Message feedback ───────────────────────────────────────────────────

    pub async fn upsert_message_feedback(
        &self,
        message_id: &str,
        channel_id: Option<&str>,
        feedback: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO message_feedback (message_id, channel_id, feedback, created_at)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(message_id) DO UPDATE SET feedback = ?3, created_at = ?4"#,
        )
        .bind(message_id)
        .bind(channel_id)
        .bind(feedback)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_message_feedback(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM message_feedback WHERE message_id = ?1")
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_channel_workflow_state(
        &self,
        channel_id: &str,
        status: &str,
        current_layer: Option<i64>,
        review_round: i64,
        awaiting_user: bool,
        blocked_reason: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE pack_channels
            SET status = ?1,
                current_layer = ?2,
                review_round = ?3,
                awaiting_user = ?4,
                blocked_reason = ?5
            WHERE id = ?6
            "#,
        )
        .bind(status)
        .bind(current_layer)
        .bind(review_round)
        .bind(if awaiting_user { 1 } else { 0 })
        .bind(blocked_reason)
        .bind(channel_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_channel_workflow_state(
        &self,
        channel_id: &str,
    ) -> Result<Option<ChannelWorkflowState>> {
        let row = sqlx::query(
            r#"
            SELECT id, status, current_layer, review_round, awaiting_user, blocked_reason
            FROM pack_channels
            WHERE id = ?1
            "#,
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(ChannelWorkflowState {
            channel_id: row.get("id"),
            status: row.get("status"),
            current_layer: row.get("current_layer"),
            review_round: row.get("review_round"),
            awaiting_user: row.get::<i64, _>("awaiting_user") != 0,
            blocked_reason: row.get("blocked_reason"),
        }))
    }

    pub async fn save_channel_plan(&self, plan: &DelegationPlan) -> Result<()> {
        let now = Utc::now().timestamp();
        let plan_json = serde_json::to_string(plan)?;
        sqlx::query(
            r#"
            INSERT INTO channel_plans (channel_id, plan_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(channel_id) DO UPDATE SET
              plan_json = excluded.plan_json,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(&plan.channel_id)
        .bind(plan_json)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_channel_plan(&self, channel_id: &str) -> Result<Option<DelegationPlan>> {
        let row = sqlx::query(
            r#"
            SELECT plan_json
            FROM channel_plans
            WHERE channel_id = ?1
            "#,
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let plan_json: String = row.get("plan_json");
        Ok(Some(serde_json::from_str(&plan_json)?))
    }

    pub async fn active_channel_count(&self) -> Result<i64> {
        let row = sqlx::query(
            "SELECT COUNT(*) as cnt FROM pack_channels WHERE status IN ('active', 'awaiting_review')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("cnt"))
    }

    pub async fn complete_channel(&self, channel_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE pack_channels SET status='completed', completed_at=?1, current_layer=NULL, awaiting_user=0, blocked_reason=NULL WHERE id=?2",
        )
            .bind(Utc::now().timestamp())
            .bind(channel_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_completed_channels(&self) -> Result<i64> {
        sqlx::query(
            r#"
            DELETE FROM channel_plans
            WHERE channel_id IN (
              SELECT id FROM pack_channels WHERE status = 'completed'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        let deleted_messages = sqlx::query(
            r#"
            DELETE FROM channel_messages
            WHERE channel_id IN (
              SELECT id FROM pack_channels WHERE status = 'completed'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM channel_members
            WHERE channel_id IN (
              SELECT id FROM pack_channels WHERE status = 'completed'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("DELETE FROM pack_channels WHERE status = 'completed'")
            .execute(&self.pool)
            .await?;

        Ok(deleted_messages.rows_affected() as i64)
    }

    pub async fn clear_stale_active_channels(&self, max_age_seconds: i64) -> Result<i64> {
        let cutoff = Utc::now().timestamp() - max_age_seconds;

        sqlx::query(
            r#"
            DELETE FROM channel_plans
            WHERE channel_id IN (
              SELECT c.id
              FROM pack_channels c
              WHERE c.status = 'active'
                AND COALESCE(
                  (SELECT MAX(msg.timestamp) FROM channel_messages msg WHERE msg.channel_id = c.id),
                  c.created_at
                ) < ?1
            )
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        let deleted_channels = sqlx::query(
            r#"
            DELETE FROM channel_messages
            WHERE channel_id IN (
              SELECT c.id
              FROM pack_channels c
              WHERE c.status = 'active'
                AND COALESCE(
                  (SELECT MAX(msg.timestamp) FROM channel_messages msg WHERE msg.channel_id = c.id),
                  c.created_at
                ) < ?1
            )
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM channel_members
            WHERE channel_id IN (
              SELECT c.id
              FROM pack_channels c
              WHERE c.status = 'active'
                AND COALESCE(
                  (SELECT MAX(msg.timestamp) FROM channel_messages msg WHERE msg.channel_id = c.id),
                  c.created_at
                ) < ?1
            )
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM pack_channels
            WHERE status = 'active'
              AND COALESCE(
                (SELECT MAX(msg.timestamp) FROM channel_messages msg WHERE msg.channel_id = pack_channels.id),
                created_at
              ) < ?1
            "#,
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;

        Ok(deleted_channels.rows_affected() as i64)
    }

    pub async fn list_channels(&self, limit: i64) -> Result<Vec<ChannelRecord>> {
        let rows = sqlx::query(
            r#"
      SELECT c.id, c.task_id, c.title, c.status, c.created_at, c.completed_at,
             c.current_layer, c.review_round, c.awaiting_user, c.blocked_reason,
             COALESCE(
               msg.updated_at,
               c.completed_at,
               c.created_at
             ) AS updated_at,
             COALESCE(mem.members, '') AS members
      FROM pack_channels c
      LEFT JOIN (
        SELECT channel_id, MAX(timestamp) AS updated_at
        FROM channel_messages
        GROUP BY channel_id
      ) msg ON msg.channel_id = c.id
      LEFT JOIN (
        SELECT channel_id, GROUP_CONCAT(pup_id) AS members
        FROM channel_members
        GROUP BY channel_id
      ) mem ON mem.channel_id = c.id
      ORDER BY c.created_at DESC
      LIMIT ?1
      "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let members_str: String = row.get("members");
                let members: Vec<String> = members_str
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                ChannelRecord {
                    id: row.get("id"),
                    task_id: row.get("task_id"),
                    title: row.get("title"),
                    status: row.get("status"),
                    created_at: row.get("created_at"),
                    completed_at: row.get("completed_at"),
                    updated_at: row.get("updated_at"),
                    current_layer: row.get("current_layer"),
                    review_round: row.get("review_round"),
                    awaiting_user: row.get::<i64, _>("awaiting_user") != 0,
                    blocked_reason: row.get("blocked_reason"),
                    members,
                }
            })
            .collect())
    }

    pub async fn get_channel_messages(
        &self,
        channel_id: &str,
    ) -> Result<Vec<ChannelMessageRecord>> {
        let rows = sqlx::query(
      r#"
      SELECT id, channel_id, sender, content, msg_type, artifact_name, status_val, mentions, reply_to, event_payload, timestamp
      FROM channel_messages
      WHERE channel_id = ?1
      ORDER BY timestamp ASC
      "#,
    )
    .bind(channel_id)
    .fetch_all(&self.pool)
    .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let mentions_json: String =
                    row.get::<Option<String>, _>("mentions").unwrap_or_default();
                let mentions: Vec<String> =
                    serde_json::from_str(&mentions_json).unwrap_or_default();
                let event_payload_json: String = row
                    .get::<Option<String>, _>("event_payload")
                    .unwrap_or_default();
                ChannelMessageRecord {
                    id: row.get("id"),
                    channel_id: row.get("channel_id"),
                    sender: row.get("sender"),
                    content: row.get("content"),
                    msg_type: row.get("msg_type"),
                    artifact_name: row.get("artifact_name"),
                    status_val: row.get("status_val"),
                    mentions,
                    reply_to: row.get("reply_to"),
                    event_payload: serde_json::from_str(&event_payload_json).ok(),
                    timestamp: row.get("timestamp"),
                }
            })
            .collect())
    }

    pub async fn add_conversation(&self, pup: &str, role: &str, content: &str) -> Result<()> {
        sqlx::query(
            r#"
      INSERT INTO conversations (pup, role, content, timestamp)
      VALUES (?1, ?2, ?3, ?4);
      "#,
        )
        .bind(pup)
        .bind(role)
        .bind(content)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn record_external_inbound(&self, id: &str, message: &InboundMessage) -> Result<()> {
        sqlx::query(
            r#"
      INSERT INTO external_messages
      (id, platform, chat_id, direction, user_id, message_id, content, created_at)
      VALUES (?1, ?2, ?3, 'inbound', ?4, ?5, ?6, ?7)
      "#,
        )
        .bind(id)
        .bind(message.platform.as_str())
        .bind(&message.chat_id)
        .bind(&message.user_id)
        .bind(&message.message_id)
        .bind(&message.text)
        .bind(message.timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_external_outbound(
        &self,
        id: &str,
        message: &OutboundMessage,
    ) -> Result<()> {
        sqlx::query(
            r#"
      INSERT INTO external_messages
      (id, platform, chat_id, direction, reply_to_id, msg_type, content, created_at)
      VALUES (?1, ?2, ?3, 'outbound', ?4, ?5, ?6, ?7)
      "#,
        )
        .bind(id)
        .bind(message.platform.as_str())
        .bind(&message.chat_id)
        .bind(message.reply_to_id.as_deref())
        .bind(format!("{:?}", message.msg_type).to_lowercase())
        .bind(&message.text)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_bridge_connection(
        &self,
        platform: &str,
        status: &BridgeConnectionState,
        connected: bool,
        last_seen: Option<i64>,
        error_msg: Option<&str>,
    ) -> Result<()> {
        let status_str = match status {
            BridgeConnectionState::Unconfigured => "unconfigured",
            BridgeConnectionState::Connecting => "connecting",
            BridgeConnectionState::Connected => "connected",
            BridgeConnectionState::Error => "error",
        };
        sqlx::query(
            r#"
      INSERT INTO bridge_connections (platform, status, connected, last_seen, error_msg)
      VALUES (?1, ?2, ?3, ?4, ?5)
      ON CONFLICT(platform) DO UPDATE SET
        status=excluded.status,
        connected=excluded.connected,
        last_seen=excluded.last_seen,
        error_msg=excluded.error_msg
      "#,
        )
        .bind(platform)
        .bind(status_str)
        .bind(if connected { 1 } else { 0 })
        .bind(last_seen)
        .bind(error_msg)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_bridge_connections(&self) -> Result<Vec<BridgeConnectionStatus>> {
        let rows = sqlx::query(
            r#"
      SELECT platform, status, connected, last_seen, error_msg
      FROM bridge_connections
      ORDER BY platform ASC
      "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let status = match row.get::<String, _>("status").as_str() {
                    "connected" => BridgeConnectionState::Connected,
                    "connecting" => BridgeConnectionState::Connecting,
                    "error" => BridgeConnectionState::Error,
                    _ => BridgeConnectionState::Unconfigured,
                };
                BridgeConnectionStatus {
                    platform: row.get("platform"),
                    status,
                    connected: row.get::<i64, _>("connected") != 0,
                    last_seen: row.get("last_seen"),
                    error_msg: row.get("error_msg"),
                }
            })
            .collect())
    }

    /// Semantic search over long-term memories using cosine similarity.
    /// Falls back to an importance-ordered list if embeddings are unavailable.
    pub async fn search_long_term(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        // Attempt to get query embedding; fall back to importance-based if it fails
        let query_vec = match self.llm.embed(query).await {
            Ok(v) => v,
            Err(_) => {
                let rows = sqlx::query(
                    r#"SELECT content FROM long_term_memory
                       WHERE memory_type != 'invalidated' AND superseded_by IS NULL
                       ORDER BY importance DESC LIMIT ?1"#,
                )
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?;
                return Ok(rows
                    .into_iter()
                    .map(|r| r.get::<String, _>("content"))
                    .collect());
            }
        };

        // Load all embeddings and compute cosine similarity in-process
        let rows = sqlx::query(
            r#"
      SELECT m.id, m.content, e.embedding
      FROM long_term_memory m
      JOIN memory_embeddings e ON m.id = e.memory_id
      WHERE m.memory_type != 'invalidated' AND m.superseded_by IS NULL
      ORDER BY m.importance DESC
      LIMIT 200
      "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut scored: Vec<(f32, String)> = rows
            .into_iter()
            .filter_map(|row| {
                let content: String = row.get("content");
                let emb_json: String = row.get("embedding");
                let emb: Vec<f32> = serde_json::from_str(&emb_json).ok()?;
                Some((cosine_similarity(&query_vec, &emb), content))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(_, c)| c).collect())
    }

    /// Store an extracted fact/preference/rule and generate its embedding.
    pub async fn add_long_term_memory(
        &self,
        content: &str,
        memory_type: &str,
        importance: f32,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
      INSERT INTO long_term_memory (id, content, memory_type, importance, created_at)
      VALUES (?1, ?2, ?3, ?4, ?5);
      "#,
        )
        .bind(&id)
        .bind(content)
        .bind(memory_type)
        .bind(importance as f64)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;

        // Generate and store embedding (best-effort — silently skip if API unavailable)
        if let Ok(emb) = self.llm.embed(content).await {
            if let Ok(emb_json) = serde_json::to_string(&emb) {
                let _ = sqlx::query(
          r#"INSERT OR REPLACE INTO memory_embeddings (memory_id, embedding) VALUES (?1, ?2)"#,
        )
        .bind(&id)
        .bind(emb_json)
        .execute(&self.pool)
        .await;
            }
        }

        Ok(())
    }

    /// Fetch last N conversation messages across all agents (newest-first).
    /// v2: shared conversation history — all agents read the same stream.
    /// The `pup` column is preserved as speaker metadata, not used for filtering.
    pub async fn recent_conversations(&self, limit: i64) -> Result<Vec<(String, String, String)>> {
        let rows = sqlx::query(
            r#"
      SELECT role, content, pup
      FROM conversations
      ORDER BY id DESC
      LIMIT ?1;
      "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("role"),
                    row.get::<String, _>("content"),
                    row.get::<String, _>("pup"),
                )
            })
            .collect())
    }

    /// Compat alias: fetch recent conversations as (role, content) pairs.
    /// Used by router classify_history and other callers that don't need speaker info.
    pub async fn recent_conversations_global(&self, limit: i64) -> Result<Vec<(String, String)>> {
        let rows = self.recent_conversations(limit).await?;
        Ok(rows
            .into_iter()
            .map(|(role, content, _)| (role, content))
            .collect())
    }

    /// Fetch conversation for display: returns (role, content, timestamp) ordered oldest-first.
    /// v2: shared history — pup_key parameter kept for API compat but ignored in query.
    /// If `before_timestamp` is set, only fetches messages older than that timestamp.
    pub async fn get_pup_conversation_display(
        &self,
        _pup: &str,
        limit: i64,
        before_timestamp: Option<i64>,
    ) -> Result<Vec<(String, String, i64)>> {
        let rows = if let Some(ts) = before_timestamp {
            sqlx::query(
                r#"
          SELECT role, content, timestamp
          FROM conversations
          WHERE timestamp < ?1
          ORDER BY id DESC
          LIMIT ?2;
          "#,
            )
            .bind(ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
          SELECT role, content, timestamp
          FROM conversations
          ORDER BY id DESC
          LIMIT ?1;
          "#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        let mut result: Vec<(String, String, i64)> = rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("role"),
                    row.get::<String, _>("content"),
                    row.get::<i64, _>("timestamp"),
                )
            })
            .collect();
        result.reverse();
        Ok(result)
    }

    /// Count messages for a pup.
    /// v2: global message count (pup_key kept for API compat but ignored).
    pub async fn get_pup_message_count(&self, _pup: &str) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM conversations")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("cnt"))
    }

    /// Delete all conversation history + context summary.
    /// v2: clears everything (shared history). pup_key kept for API compat.
    pub async fn clear_pup_conversation(&self, _pup: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversations")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM context_summaries WHERE scope = 'global'")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Returns total number of conversation rows stored (user + assistant combined).
    pub async fn conversation_count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM conversations")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("cnt"))
    }

    /// List memories with optional query filter.
    /// Returns (id, content, memory_type, importance, created_at, superseded_by).
    pub async fn list_long_term_memories(
        &self,
        offset: i64,
        limit: i64,
        query: Option<&str>,
    ) -> Result<Vec<(String, String, String, f32, i64, Option<String>)>> {
        let mut sql = String::from(
            r#"
      SELECT id, content, memory_type, importance, created_at, superseded_by
      FROM long_term_memory
      "#,
        );
        let mut params: Vec<String> = Vec::new();

        if let Some(q) = query {
            sql.push_str("WHERE content LIKE ?1 ");
            params.push(format!("%{}%", q));
            sql.push_str("ORDER BY created_at DESC LIMIT ?2 OFFSET ?3");
        } else {
            sql.push_str("ORDER BY created_at DESC LIMIT ?1 OFFSET ?2");
        }

        let mut q = sqlx::query(&sql);
        if let Some(p) = params.get(0) {
            q = q.bind(p);
        }
        q = q.bind(limit).bind(offset);

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let content: String = row.get("content");
                let memory_type: String = row.get("memory_type");
                let importance: f32 = row.get::<f64, _>("importance") as f32;
                let created_at: i64 = row.get("created_at");
                let superseded_by: Option<String> = row.get("superseded_by");
                (
                    id,
                    content,
                    memory_type,
                    importance,
                    created_at,
                    superseded_by,
                )
            })
            .collect())
    }

    pub async fn update_long_term_memory(
        &self,
        id: &str,
        content: &str,
        memory_type: &str,
        importance: f32,
    ) -> Result<()> {
        sqlx::query(
            r#"
      UPDATE long_term_memory
      SET content = ?1,
          memory_type = ?2,
          importance = ?3
      WHERE id = ?4;
      "#,
        )
        .bind(content)
        .bind(memory_type)
        .bind(importance as f64)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_long_term_memory(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
      DELETE FROM long_term_memory
      WHERE id = ?1;
      "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch the most important memories for display as context chips.
    pub async fn get_top_memories(&self, limit: i64) -> Result<Vec<(String, String, f32)>> {
        let rows = sqlx::query(
            r#"
      SELECT content, memory_type, importance
      FROM long_term_memory
      WHERE memory_type != 'invalidated' AND superseded_by IS NULL
      ORDER BY importance DESC, created_at DESC
      LIMIT ?1;
      "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let content: String = row.get("content");
                let memory_type: String = row.get("memory_type");
                let importance: f32 = row.get::<f64, _>("importance") as f32;
                (content, memory_type, importance)
            })
            .collect())
    }

    /// Fetch recent conversations with timestamps for the Timeline view.
    pub async fn list_conversations_for_timeline(
        &self,
        limit: i64,
    ) -> Result<Vec<(String, String, i64)>> {
        let rows = sqlx::query(
            r#"
      SELECT role, content, timestamp
      FROM conversations
      ORDER BY id DESC
      LIMIT ?1;
      "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let role: String = row.get("role");
                let content: String = row.get("content");
                let timestamp: i64 = row.get("timestamp");
                (role, content, timestamp)
            })
            .collect())
    }

    /// Full-text LIKE search over conversation history.
    pub async fn search_conversations(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<(String, String, i64)>> {
        let like_pattern = format!("%{}%", query);
        let rows = sqlx::query(
            r#"SELECT role, content, timestamp FROM conversations
         WHERE content LIKE ?1 ORDER BY id DESC LIMIT ?2"#,
        )
        .bind(like_pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("role"), row.get("content"), row.get("timestamp")))
            .collect())
    }
}

// ── Task records ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: String,
    pub description: String,
    pub assigned_pup: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub result: Option<String>,
}

impl MemorySystem {
    pub async fn create_task(
        &self,
        description: &str,
        assigned_pup: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"INSERT INTO tasks (id, description, assigned_pup, status, created_at)
         VALUES (?1, ?2, ?3, 'pending', ?4)"#,
        )
        .bind(&id)
        .bind(description)
        .bind(assigned_pup)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<TaskRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, description, assigned_pup, status, created_at, completed_at, result
         FROM tasks ORDER BY created_at DESC LIMIT ?1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| TaskRecord {
                id: row.get("id"),
                description: row.get("description"),
                assigned_pup: row.get("assigned_pup"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                completed_at: row.get("completed_at"),
                result: row.get("result"),
            })
            .collect())
    }

    pub async fn update_task_status(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
    ) -> Result<()> {
        let completed_at: Option<i64> = if status == "done" || status == "failed" {
            Some(Utc::now().timestamp())
        } else {
            None
        };
        sqlx::query(
            r#"UPDATE tasks SET status = ?1, result = ?2, completed_at = ?3 WHERE id = ?4"#,
        )
        .bind(status)
        .bind(result)
        .bind(completed_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_task(&self, id: &str) -> Result<()> {
        sqlx::query(r#"DELETE FROM tasks WHERE id = ?1"#)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// ── Skill run records ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SkillRunRecord {
    pub id: String,
    pub skill_name: String,
    pub triggered_by: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub output: Option<String>,
}

impl MemorySystem {
    pub async fn record_skill_run(
        &self,
        id: &str,
        skill_name: &str,
        triggered_by: &str,
        job_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO skill_runs (id, skill_name, triggered_by, started_at, status, job_id)
         VALUES (?1, ?2, ?3, ?4, 'running', ?5)"#,
        )
        .bind(id)
        .bind(skill_name)
        .bind(triggered_by)
        .bind(Utc::now().timestamp())
        .bind(job_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return the most recent skill run for a given job ID.
    pub async fn last_run_for_job(&self, job_id: &str) -> Result<Option<SkillRunRecord>> {
        let row = sqlx::query(
            r#"SELECT id, skill_name, triggered_by, started_at, completed_at, status, output
               FROM skill_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| SkillRunRecord {
            id: row.get("id"),
            skill_name: row.get("skill_name"),
            triggered_by: row.get("triggered_by"),
            started_at: row.get("started_at"),
            completed_at: row.get("completed_at"),
            status: row.get("status"),
            output: row.get("output"),
        }))
    }

    /// Return (total_runs, fail_count) for a given job ID.
    pub async fn job_run_stats(&self, job_id: &str) -> Result<(i64, i64)> {
        let row: (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*), COALESCE(SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END), 0)
               FROM skill_runs WHERE job_id = ?1"#,
        )
        .bind(job_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn complete_skill_run(&self, id: &str, status: &str, output: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE skill_runs SET status = ?1, output = ?2, completed_at = ?3 WHERE id = ?4"#,
        )
        .bind(status)
        .bind(output)
        .bind(Utc::now().timestamp())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Returns true if any stored memory has cosine similarity ≥ min_similarity to text.
    /// Uses the same embed+cosine logic as search_long_term.
    pub async fn has_similar_memory(&self, text: &str, min_similarity: f32) -> bool {
        let query_vec = match self.llm.embed(text).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        let rows = match sqlx::query(
            r#"SELECT e.embedding FROM memory_embeddings e
               JOIN long_term_memory m ON e.memory_id = m.id
               WHERE m.memory_type != 'invalidated' AND m.superseded_by IS NULL
               LIMIT 500"#,
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(r) => r,
            Err(_) => return false,
        };
        rows.iter().any(|row| {
            let emb_json: String = row.get("embedding");
            serde_json::from_str::<Vec<f32>>(&emb_json)
                .map(|emb| cosine_similarity(&query_vec, &emb) >= min_similarity)
                .unwrap_or(false)
        })
    }

    /// Return the `started_at` timestamp of the most recent completed run for a
    /// given skill name, or `None` if no completed run exists.
    pub async fn last_skill_run_time(&self, skill_name: &str) -> Result<Option<i64>> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"SELECT started_at FROM skill_runs
               WHERE skill_name = ?1 AND status = 'completed'
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(skill_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(ts,)| ts))
    }

    pub async fn list_skill_runs(&self, limit: i64) -> Result<Vec<SkillRunRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, skill_name, triggered_by, started_at, completed_at, status, output
         FROM skill_runs ORDER BY started_at DESC LIMIT ?1"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| SkillRunRecord {
                id: row.get("id"),
                skill_name: row.get("skill_name"),
                triggered_by: row.get("triggered_by"),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                status: row.get("status"),
                output: row.get("output"),
            })
            .collect())
    }
}

// ── Context compression ───────────────────────────────────────────────────────

impl MemorySystem {
    /// Returns the stored rolling summary and the rowid of the last
    /// conversation row it covers (0 = no summary yet).
    /// v2: uses global scope (pup='') instead of per-pup.
    pub async fn get_context_summary(&self) -> Result<Option<(String, i64)>> {
        let row = sqlx::query(
            "SELECT summary, covers_through_row FROM context_summaries WHERE scope = 'global'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            (
                r.get::<String, _>("summary"),
                r.get::<i64, _>("covers_through_row"),
            )
        }))
    }

    /// Returns compression status — whether conversation has been compressed
    /// and which row is covered by the compression.
    /// v2: uses global scope (pup='') instead of per-pup.
    pub async fn get_compression_status(&self) -> Result<(bool, i64)> {
        let row =
            sqlx::query("SELECT covers_through_row FROM context_summaries WHERE scope = 'global'")
                .fetch_optional(&self.pool)
                .await?;

        match row {
            Some(r) => {
                let covers_through_row: i64 = r.get("covers_through_row");
                let is_compressed = covers_through_row > 0;
                Ok((is_compressed, covers_through_row))
            }
            None => Ok((false, 0)),
        }
    }

    // ── Knowledge Base ────────────────────────────────────────────────────────

    /// Expose LLM embed for knowledge module use.
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        self.llm.embed(text).await
    }

    pub async fn insert_knowledge_source(
        &self,
        id: &str,
        title: &str,
        source_type: &str,
        source_path: Option<&str>,
        mime_type: Option<&str>,
        byte_size: Option<i64>,
        tags: Option<&str>,
        now: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO knowledge_sources
            (id, title, source_type, source_path, mime_type, byte_size, tags,
             chunk_count, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 'pending', ?8, ?8)"#,
        )
        .bind(id)
        .bind(title)
        .bind(source_type)
        .bind(source_path)
        .bind(mime_type)
        .bind(byte_size)
        .bind(tags)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_knowledge_source_status(
        &self,
        id: &str,
        status: &str,
        error_msg: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE knowledge_sources SET status=?1, error_msg=?2, updated_at=?3 WHERE id=?4",
        )
        .bind(status)
        .bind(error_msg)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_knowledge_source_mime(&self, id: &str, mime_type: &str) -> Result<()> {
        sqlx::query("UPDATE knowledge_sources SET mime_type=?1 WHERE id=?2")
            .bind(mime_type)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_knowledge_source_indexed(&self, id: &str, chunk_count: i64) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE knowledge_sources SET status='indexed', chunk_count=?1, updated_at=?2 WHERE id=?3",
        )
        .bind(chunk_count)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_knowledge_chunk(
        &self,
        id: &str,
        source_id: &str,
        content: &str,
        chunk_index: i64,
        heading_path: Option<&str>,
        char_start: Option<i64>,
        char_end: Option<i64>,
        embedding_json: &str,
        created_at: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO knowledge_chunks
            (id, source_id, content, chunk_index, heading_path,
             char_start, char_end, embedding, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
        )
        .bind(id)
        .bind(source_id)
        .bind(content)
        .bind(chunk_index)
        .bind(heading_path)
        .bind(char_start)
        .bind(char_end)
        .bind(embedding_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_knowledge_sources(
        &self,
    ) -> Result<Vec<crate::knowledge::types::KnowledgeSource>> {
        let rows = sqlx::query(
            "SELECT id, title, source_type, source_path, mime_type, byte_size, tags, chunk_count, status, error_msg, created_at, updated_at FROM knowledge_sources ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::knowledge::types::KnowledgeSource {
                id: r.get("id"),
                title: r.get("title"),
                source_type: r.get("source_type"),
                source_path: r.get("source_path"),
                mime_type: r.get("mime_type"),
                byte_size: r.get("byte_size"),
                tags: r.get("tags"),
                chunk_count: r.get("chunk_count"),
                status: r.get("status"),
                error_msg: r.get("error_msg"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    pub async fn delete_knowledge_source(&self, id: &str) -> Result<()> {
        // Clean up graph relations linked to this source
        sqlx::query("DELETE FROM kg_relations WHERE source_id=?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        // Clean up chunk↔entity links for chunks of this source
        sqlx::query(
            "DELETE FROM kg_chunk_entities WHERE chunk_id IN \
             (SELECT id FROM knowledge_chunks WHERE source_id=?1)",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        // Remove entities that were only created by this source
        sqlx::query(
            "DELETE FROM kg_entities WHERE source_id=?1 \
             AND id NOT IN (SELECT DISTINCT from_id FROM kg_relations) \
             AND id NOT IN (SELECT DISTINCT to_id FROM kg_relations)",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        // Delete chunks and source
        sqlx::query("DELETE FROM knowledge_chunks WHERE source_id=?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM knowledge_sources WHERE id=?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Full-text search over knowledge chunks using FTS5.
    pub async fn fts_search_knowledge_chunks(
        &self,
        query: &str,
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<crate::knowledge::types::KbSearchResult>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        // Escape special FTS5 characters and use implicit AND
        let sanitized: String = query
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();
        if sanitized.trim().is_empty() {
            return Ok(vec![]);
        }

        let rows = sqlx::query(
            r#"
            SELECT f.chunk_id, f.source_id, f.content, f.heading_path,
                   kc.chunk_index, ks.title, ks.source_path, ks.tags,
                   rank
            FROM knowledge_chunks_fts f
            JOIN knowledge_chunks kc ON f.chunk_id = kc.id
            JOIN knowledge_sources ks ON f.source_id = ks.id
            WHERE knowledge_chunks_fts MATCH ?1
              AND ks.status = 'indexed'
            ORDER BY rank
            LIMIT ?2
            "#,
        )
        .bind(&sanitized)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            // Tag filter
            if let Some(filter_tags) = tags {
                if !filter_tags.is_empty() {
                    let src_tags: String = row.get::<Option<String>, _>("tags").unwrap_or_default();
                    let src_tags_vec: Vec<String> =
                        serde_json::from_str(&src_tags).unwrap_or_default();
                    if !filter_tags.iter().any(|t| src_tags_vec.contains(t)) {
                        continue;
                    }
                }
            }

            results.push(crate::knowledge::types::KbSearchResult {
                chunk_id: row.get("chunk_id"),
                source_id: row.get("source_id"),
                source_title: row.get("title"),
                source_path: row.get("source_path"),
                content: row.get("content"),
                heading_path: row.get("heading_path"),
                score: 0.0, // will be set by RRF fusion
                chunk_index: row.get("chunk_index"),
            });
        }

        Ok(results)
    }

    /// Semantic search over knowledge chunks using cosine similarity.
    pub async fn search_knowledge_chunks(
        &self,
        query_vec: &[f32],
        limit: usize,
        tags: Option<&[String]>,
    ) -> Result<Vec<crate::knowledge::types::KbSearchResult>> {
        // Load all chunk embeddings from indexed sources (capped at 2000)
        let rows = sqlx::query(
            r#"
            SELECT kc.id, kc.source_id, kc.content, kc.heading_path, kc.chunk_index, kc.embedding,
                   ks.title, ks.source_path, ks.tags
            FROM knowledge_chunks kc
            JOIN knowledge_sources ks ON kc.source_id = ks.id
            WHERE ks.status = 'indexed'
            LIMIT 2000
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut scored: Vec<(f32, crate::knowledge::types::KbSearchResult)> = rows
            .into_iter()
            .filter_map(|row| {
                let emb_json: String = row.get("embedding");
                let emb: Vec<f32> = serde_json::from_str(&emb_json).ok()?;
                let score = cosine_similarity(query_vec, &emb);
                if score < 0.4 {
                    return None;
                }

                // Tag filter
                if let Some(filter_tags) = tags {
                    if !filter_tags.is_empty() {
                        let src_tags: String =
                            row.get::<Option<String>, _>("tags").unwrap_or_default();
                        let src_tags_vec: Vec<String> =
                            serde_json::from_str(&src_tags).unwrap_or_default();
                        if !filter_tags.iter().any(|t| src_tags_vec.contains(t)) {
                            return None;
                        }
                    }
                }

                Some((
                    score,
                    crate::knowledge::types::KbSearchResult {
                        chunk_id: row.get("id"),
                        source_id: row.get("source_id"),
                        source_title: row.get("title"),
                        source_path: row.get("source_path"),
                        content: row.get("content"),
                        heading_path: row.get("heading_path"),
                        score,
                        chunk_index: row.get("chunk_index"),
                    },
                ))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(_, r)| r).collect())
    }

    // ── Knowledge Graph (v0.1.11) ─────────────────────────────────────────────

    /// Upsert an entity; returns the actual id (existing or new).
    pub async fn upsert_kg_entity(
        &self,
        id: &str,
        name: &str,
        entity_type: &str,
        description: Option<&str>,
        source_id: &str,
        now: i64,
    ) -> Result<String> {
        sqlx::query(
            r#"INSERT INTO kg_entities
            (id, name, entity_type, description, source_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
            ON CONFLICT(name, entity_type) DO UPDATE
            SET updated_at=excluded.updated_at,
                description=COALESCE(excluded.description, kg_entities.description)"#,
        )
        .bind(id)
        .bind(name)
        .bind(entity_type)
        .bind(description)
        .bind(source_id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let actual_id: String =
            sqlx::query_scalar("SELECT id FROM kg_entities WHERE name=?1 AND entity_type=?2")
                .bind(name)
                .bind(entity_type)
                .fetch_one(&self.pool)
                .await?;

        Ok(actual_id)
    }

    /// Link a chunk to an entity.
    pub async fn link_chunk_entity(&self, chunk_id: &str, entity_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO kg_chunk_entities (chunk_id, entity_id) VALUES (?1, ?2)",
        )
        .bind(chunk_id)
        .bind(entity_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Insert a relation (idempotent via UNIQUE constraint).
    pub async fn insert_kg_relation(
        &self,
        id: &str,
        from_id: &str,
        to_id: &str,
        relation: &str,
        source_id: &str,
        confidence: f32,
        now: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT OR IGNORE INTO kg_relations
            (id, from_id, to_id, relation, source_id, confidence, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        )
        .bind(id)
        .bind(from_id)
        .bind(to_id)
        .bind(relation)
        .bind(source_id)
        .bind(confidence)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Find entities by fuzzy name match.
    pub async fn find_kg_entities(
        &self,
        name: &str,
    ) -> Result<Vec<(String, String, String, Option<String>)>> {
        let rows = sqlx::query(
            "SELECT id, name, entity_type, description FROM kg_entities WHERE name LIKE ?1",
        )
        .bind(format!("%{name}%"))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get("id"),
                    r.get("name"),
                    r.get("entity_type"),
                    r.get("description"),
                )
            })
            .collect())
    }

    /// Get neighbor entity IDs (both directions).
    pub async fn kg_neighbors(&self, entity_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT to_id FROM kg_relations WHERE from_id=?1
             UNION
             SELECT from_id FROM kg_relations WHERE to_id=?1",
        )
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get chunks linked to an entity.
    pub async fn kg_entity_chunks(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::knowledge::types::KbSearchResult>> {
        let rows = sqlx::query(
            r#"
            SELECT kc.id, kc.source_id, kc.content, kc.heading_path, kc.chunk_index,
                   ks.title, ks.source_path
            FROM kg_chunk_entities kce
            JOIN knowledge_chunks kc ON kce.chunk_id = kc.id
            JOIN knowledge_sources ks ON kc.source_id = ks.id
            WHERE kce.entity_id = ?1 AND ks.status = 'indexed'
            LIMIT ?2
            "#,
        )
        .bind(entity_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::knowledge::types::KbSearchResult {
                chunk_id: r.get("id"),
                source_id: r.get("source_id"),
                source_title: r.get("title"),
                source_path: r.get("source_path"),
                content: r.get("content"),
                heading_path: r.get("heading_path"),
                score: 0.7,
                chunk_index: r.get("chunk_index"),
            })
            .collect())
    }

    /// List all entities, optionally filtered by type.
    pub async fn list_kg_entities(
        &self,
        entity_type: Option<&str>,
    ) -> Result<Vec<(String, String, String, Option<String>)>> {
        let rows = if let Some(et) = entity_type {
            sqlx::query(
                "SELECT id, name, entity_type, description FROM kg_entities WHERE entity_type=?1 ORDER BY name",
            )
            .bind(et)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, name, entity_type, description FROM kg_entities ORDER BY entity_type, name",
            )
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get("id"),
                    r.get("name"),
                    r.get("entity_type"),
                    r.get("description"),
                )
            })
            .collect())
    }

    /// Get relations for an entity (both directions).
    pub async fn kg_entity_relations(
        &self,
        entity_id: &str,
    ) -> Result<Vec<(String, String, String, String, f32)>> {
        // (relation, other_entity_name, other_entity_type, direction, confidence)
        let rows = sqlx::query(
            r#"
            SELECT r.relation, r.confidence, e.name, e.entity_type, 'out' AS direction
            FROM kg_relations r
            JOIN kg_entities e ON r.to_id = e.id
            WHERE r.from_id = ?1
            UNION ALL
            SELECT r.relation, r.confidence, e.name, e.entity_type, 'in' AS direction
            FROM kg_relations r
            JOIN kg_entities e ON r.from_id = e.id
            WHERE r.to_id = ?1
            "#,
        )
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>("relation"),
                    r.get::<String, _>("name"),
                    r.get::<String, _>("entity_type"),
                    r.get::<String, _>("direction"),
                    r.get::<f32, _>("confidence"),
                )
            })
            .collect())
    }

    /// Get chunks for a knowledge source.
    pub async fn get_source_chunks(
        &self,
        source_id: &str,
    ) -> Result<Vec<crate::knowledge::types::KnowledgeChunk>> {
        let rows = sqlx::query(
            "SELECT id, source_id, content, chunk_index, heading_path, char_start, char_end, created_at
             FROM knowledge_chunks WHERE source_id=?1 ORDER BY chunk_index",
        )
        .bind(source_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::knowledge::types::KnowledgeChunk {
                id: r.get("id"),
                source_id: r.get("source_id"),
                content: r.get("content"),
                chunk_index: r.get("chunk_index"),
                heading_path: r.get("heading_path"),
                char_start: r.get("char_start"),
                char_end: r.get("char_end"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}
