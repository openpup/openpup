use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Pool, Row, Sqlite,
};

use crate::bridge::types::{
    BridgeConnectionState, BridgeConnectionStatus, InboundMessage, OutboundMessage,
};
use crate::channel::types::{
    ChannelMessageRecord, ChannelRecord, ChannelWorkflowState, DelegationPlan,
};
use crate::llm::client::LlmClient;

#[derive(Clone)]
pub struct MemorySystem {
    pool: Pool<Sqlite>,
    llm: Arc<LlmClient>,
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
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
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

        // Migration: add pup column to context_summaries (idempotent)
        let _ = sqlx::query(
            "ALTER TABLE context_summaries ADD COLUMN pup TEXT NOT NULL DEFAULT 'alpha'",
        )
        .execute(&self.pool)
        .await;

        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_context_summaries_pup ON context_summaries(pup)",
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
        output TEXT
      );
      "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
      CREATE INDEX IF NOT EXISTS idx_skill_runs_started_at
      ON skill_runs(started_at);
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

        Ok(())
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
               (SELECT MAX(msg.timestamp) FROM channel_messages msg WHERE msg.channel_id = c.id),
               c.completed_at,
               c.created_at
             ) AS updated_at,
             COALESCE(GROUP_CONCAT(m.pup_id), '') AS members
      FROM pack_channels c
      LEFT JOIN channel_members m ON c.id = m.channel_id
      GROUP BY c.id
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
                    r#"SELECT content FROM long_term_memory ORDER BY importance DESC LIMIT ?1"#,
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

    /// Fetch last N conversation messages for a specific pup (newest-first).
    pub async fn recent_conversations(
        &self,
        pup: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            r#"
      SELECT role, content
      FROM conversations
      WHERE pup = ?1
      ORDER BY id DESC
      LIMIT ?2;
      "#,
        )
        .bind(pup)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("role"),
                    row.get::<String, _>("content"),
                )
            })
            .collect())
    }

    /// Fetch last N conversation messages across ALL pups (for intent classification context).
    pub async fn recent_conversations_global(&self, limit: i64) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            r#"
      SELECT role, content
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
                )
            })
            .collect())
    }

    /// Fetch conversation for display: returns (role, content, timestamp) ordered oldest-first.
    /// If `before_timestamp` is set, only fetches messages older than that timestamp.
    pub async fn get_pup_conversation_display(
        &self,
        pup: &str,
        limit: i64,
        before_timestamp: Option<i64>,
    ) -> Result<Vec<(String, String, i64)>> {
        let rows = if let Some(ts) = before_timestamp {
            sqlx::query(
                r#"
          SELECT role, content, timestamp
          FROM conversations
          WHERE pup = ?1 AND timestamp < ?2
          ORDER BY id DESC
          LIMIT ?3;
          "#,
            )
            .bind(pup)
            .bind(ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
          SELECT role, content, timestamp
          FROM conversations
          WHERE pup = ?1
          ORDER BY id DESC
          LIMIT ?2;
          "#,
            )
            .bind(pup)
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
    pub async fn get_pup_message_count(&self, pup: &str) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM conversations WHERE pup = ?1")
            .bind(pup)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("cnt"))
    }

    /// Delete all conversation history + context summary for a pup.
    pub async fn clear_pup_conversation(&self, pup: &str) -> Result<()> {
        sqlx::query("DELETE FROM conversations WHERE pup = ?1")
            .bind(pup)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM context_summaries WHERE pup = ?1")
            .bind(pup)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// Returns total number of conversation rows stored (user + assistant combined).
    pub async fn conversation_count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM conversations")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("cnt"))
    }

    pub async fn list_long_term_memories(
        &self,
        offset: i64,
        limit: i64,
        query: Option<&str>,
    ) -> Result<Vec<(String, String, String, f32, i64)>> {
        let mut sql = String::from(
            r#"
      SELECT id, content, memory_type, importance, created_at
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
                (id, content, memory_type, importance, created_at)
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
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO skill_runs (id, skill_name, triggered_by, started_at, status)
         VALUES (?1, ?2, ?3, ?4, 'running')"#,
        )
        .bind(id)
        .bind(skill_name)
        .bind(triggered_by)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
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
        let rows = match sqlx::query(r#"SELECT embedding FROM memory_embeddings LIMIT 500"#)
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
    /// Returns the stored rolling summary for a pup and the rowid of the last
    /// conversation row it covers (0 = no summary yet).
    pub async fn get_context_summary(&self, pup: &str) -> Result<Option<(String, i64)>> {
        let row =
            sqlx::query("SELECT summary, covers_through_row FROM context_summaries WHERE pup = ?1")
                .bind(pup)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| {
            (
                r.get::<String, _>("summary"),
                r.get::<i64, _>("covers_through_row"),
            )
        }))
    }

    /// Returns compression status for a pup — whether it has been compressed
    /// and which row is covered by the compression.
    pub async fn get_compression_status(&self, pup: &str) -> Result<(bool, i64)> {
        let row = sqlx::query("SELECT covers_through_row FROM context_summaries WHERE pup = ?1")
            .bind(pup)
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

    /// Upsert the rolling summary for a pup.
    pub async fn save_context_summary(
        &self,
        pup: &str,
        summary: &str,
        covers_through_row: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
      INSERT INTO context_summaries (pup, summary, covers_through_row, updated_at)
      VALUES (?1, ?2, ?3, ?4)
      ON CONFLICT(pup) DO UPDATE SET
        summary = excluded.summary,
        covers_through_row = excluded.covers_through_row,
        updated_at = excluded.updated_at
      "#,
        )
        .bind(pup)
        .bind(summary)
        .bind(covers_through_row)
        .bind(Utc::now().timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch up to `limit` rows for a specific pup with rowid > `after_row`, oldest-first.
    pub async fn conversations_after_row(
        &self,
        pup: &str,
        after_row: i64,
        limit: i64,
    ) -> Result<Vec<(i64, String, String)>> {
        let rows = sqlx::query(
      "SELECT id, role, content FROM conversations WHERE pup = ?1 AND id > ?2 ORDER BY id ASC LIMIT ?3",
    )
    .bind(pup)
    .bind(after_row)
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get("id"), r.get("role"), r.get("content")))
            .collect())
    }

    /// Return the max rowid for a pup's conversations (0 if none).
    pub async fn max_conversation_row(&self, pup: &str) -> Result<i64> {
        let row = sqlx::query("SELECT COALESCE(MAX(id), 0) as m FROM conversations WHERE pup = ?1")
            .bind(pup)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("m"))
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
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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
