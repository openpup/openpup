use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, Row};

use crate::config::openpup_dir;

pub fn db_path() -> Result<String> {
    let path = openpup_dir()?.join("database.db");
    Ok(format!("sqlite:{}", path.display()))
}

pub async fn open(db_url: &str) -> Result<SqlitePool> {
    SqlitePool::connect(db_url)
        .await
        .with_context(|| format!("Cannot open database at {}", db_url))
}

// ── Memory ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub importance: f64,
    pub created_at: i64,
}

pub async fn list_memories(
    pool: &SqlitePool,
    filter_type: Option<&str>,
    limit: i64,
) -> Result<Vec<MemoryRow>> {
    let rows = if let Some(t) = filter_type {
        sqlx::query(
            "SELECT id, content, COALESCE(memory_type,'') as memory_type, COALESCE(importance,0.5) as importance, created_at \
             FROM long_term_memory WHERE memory_type = ?1 ORDER BY importance DESC, created_at DESC LIMIT ?2",
        )
        .bind(t)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, content, COALESCE(memory_type,'') as memory_type, COALESCE(importance,0.5) as importance, created_at \
             FROM long_term_memory ORDER BY importance DESC, created_at DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    rows.into_iter()
        .map(|r| {
            Ok(MemoryRow {
                id: r.try_get("id")?,
                content: r.try_get("content")?,
                memory_type: r.try_get("memory_type")?,
                importance: r.try_get("importance")?,
                created_at: r.try_get("created_at")?,
            })
        })
        .collect()
}

pub async fn search_memories(pool: &SqlitePool, query: &str, limit: i64) -> Result<Vec<MemoryRow>> {
    let pattern = format!("%{}%", query);
    let rows = sqlx::query(
        "SELECT id, content, COALESCE(memory_type,'') as memory_type, COALESCE(importance,0.5) as importance, created_at \
         FROM long_term_memory WHERE content LIKE ?1 ORDER BY importance DESC LIMIT ?2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(MemoryRow {
                id: r.try_get("id")?,
                content: r.try_get("content")?,
                memory_type: r.try_get("memory_type")?,
                importance: r.try_get("importance")?,
                created_at: r.try_get("created_at")?,
            })
        })
        .collect()
}

pub async fn count_memories(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM long_term_memory")
        .fetch_one(pool)
        .await?;
    Ok(row.try_get("cnt")?)
}

pub async fn get_top_memories(pool: &SqlitePool, limit: i64) -> Result<Vec<MemoryRow>> {
    list_memories(pool, None, limit).await
}

// ── Skills ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SkillRow {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub manifest: String,
}

pub async fn list_skills(pool: &SqlitePool) -> Result<Vec<SkillRow>> {
    let rows = sqlx::query(
        "SELECT name, COALESCE(description,'') as description, COALESCE(enabled,1) as enabled, COALESCE(manifest,'') as manifest \
         FROM skills ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|r| {
            Ok(SkillRow {
                name: r.try_get("name")?,
                description: r.try_get("description")?,
                enabled: r.try_get::<i64, _>("enabled")? != 0,
                manifest: r.try_get("manifest")?,
            })
        })
        .collect()
}

pub async fn get_skill(pool: &SqlitePool, name: &str) -> Result<Option<SkillRow>> {
    let row = sqlx::query(
        "SELECT name, COALESCE(description,'') as description, COALESCE(enabled,1) as enabled, COALESCE(manifest,'') as manifest \
         FROM skills WHERE name = ?1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    row.map(|r| {
        Ok(SkillRow {
            name: r.try_get("name")?,
            description: r.try_get("description")?,
            enabled: r.try_get::<i64, _>("enabled")? != 0,
            manifest: r.try_get("manifest")?,
        })
    })
    .transpose()
}

pub async fn count_skills(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM skills WHERE enabled = 1")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| {
            // skills table may not exist in older DBs
            panic!("skills table missing")
        });
    Ok(row.try_get("cnt")?)
}

// ── Tasks ────────────────────────────────────────────────────────────────────

pub async fn count_tasks(pool: &SqlitePool) -> Result<i64> {
    let row =
        sqlx::query("SELECT COUNT(*) as cnt FROM tasks WHERE status IN ('pending','in_progress')")
            .fetch_one(pool)
            .await?;
    Ok(row.try_get("cnt")?)
}

// ── Conversations ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ConvRow {
    pub role: String,
    pub content: String,
    #[allow(dead_code)]
    pub pup_name: Option<String>,
    #[allow(dead_code)]
    pub timestamp: i64,
}

pub async fn get_recent_history(pool: &SqlitePool, limit: i64) -> Result<Vec<ConvRow>> {
    let rows = sqlx::query(
        "SELECT role, content, pup_name, timestamp \
         FROM conversations ORDER BY timestamp DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut result: Vec<ConvRow> = rows
        .into_iter()
        .map(|r| {
            Ok(ConvRow {
                role: r.try_get("role")?,
                content: r.try_get("content")?,
                pup_name: r.try_get("pup_name")?,
                timestamp: r.try_get("timestamp")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    result.reverse();
    Ok(result)
}

pub async fn save_conversation(
    pool: &SqlitePool,
    role: &str,
    content: &str,
    pup_name: Option<&str>,
) -> Result<()> {
    let id = uuid_simple();
    let ts = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO conversations (id, role, content, pup_name, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(role)
    .bind(content)
    .bind(pup_name)
    .bind(ts)
    .execute(pool)
    .await?;
    Ok(())
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:032x}", t)
}
