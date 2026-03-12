//! 本地记忆存储：短期会话记忆 + 长期语义记忆（Phase 1 最小实现）。
//!
//! 目标：
//! - 为 Roadmap Phase 1 提供一个可用的本地记忆后端（SQLite）
//! - 提供压缩/整理入口，供 `openpup memory compact` 与未来 runtime/Agent OS 集成调用

use anyhow::{Context, Result};
use dirs::home_dir;
use rusqlite::{params, Connection};
use time::OffsetDateTime;

pub fn db_path() -> Result<std::path::PathBuf> {
    let home = home_dir().context("failed to locate home directory")?;
    Ok(home.join(".openpup").join("memory.db"))
}

fn open_db() -> Result<Connection> {
    let path = db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create memory db dir {:?}", dir))?;
    }
    Connection::open(path).context("failed to open memory.db")
}

/// 初始化 SQLite schema（若不存在则创建）。幂等。
pub fn init_schema() -> Result<()> {
    let conn = open_db()?;

    // 短期会话记忆：记录对话消息（由上层 runtime/Agent 写入）。
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS session_messages (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL,
            ts_utc      INTEGER NOT NULL,
            role        TEXT NOT NULL, -- user/assistant/system
            content     TEXT NOT NULL
        );

        -- 语义记忆：长期偏好、项目、投资洞见等，由上层定期写入。
        CREATE TABLE IF NOT EXISTS semantic_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            kind        TEXT NOT NULL, -- preferences/projects/invest_insights/...
            content     TEXT NOT NULL,
            tags        TEXT,          -- 逗号分隔或 JSON，留给上层约定
            created_ts  INTEGER NOT NULL
        );
        "#,
    )
    .context("failed to initialize memory schema")?;

    Ok(())
}

/// 简单压缩策略（Phase 1）：每个会话只保留最近 N 条消息。
///
/// - 不做自动摘要（由上层负责），这里只做硬裁剪，避免无限膨胀。
pub fn compact_sessions(max_messages_per_session: i64) -> Result<()> {
    let conn = open_db()?;

    // 找出所有 session_id；若表不存在则尽量静默失败
    let mut stmt = conn
        .prepare("SELECT DISTINCT session_id FROM session_messages")
        .context("failed to prepare session_ids query")?;
    let session_ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    for sid in session_ids {
        // 删除每个会话中多余的旧消息，只保留最近 N 条
        conn.execute(
            r#"
            DELETE FROM session_messages
            WHERE id IN (
                SELECT id FROM session_messages
                WHERE session_id = ?
                ORDER BY ts_utc DESC
                LIMIT -1 OFFSET ?
            )
            "#,
            params![sid, max_messages_per_session],
        )
        .ok(); // 单个会话失败不应影响整体
    }

    Ok(())
}

/// 语义记忆的简单 VACUUM/清理入口。
///
/// 当前仅执行 `VACUUM`，后续可根据时间/主题做更细粒度压缩。
pub fn vacuum() -> Result<()> {
    let conn = open_db()?;
    conn.execute_batch("VACUUM;")
        .context("failed to run VACUUM on memory.db")?;
    Ok(())
}

/// Phase 1 统一压缩入口，供 CLI 调用：
/// - 确保 schema 存在
/// - 对所有会话做硬裁剪
/// - 对整个 DB 做 VACUUM
pub fn compact_all() -> Result<()> {
    init_schema()?;
    // 默认每个会话最多保留 200 条消息，后续可配置化
    compact_sessions(200)?;
    vacuum()?;
    Ok(())
}

/// 新增一条语义记忆（长期偏好/项目/投资洞见等）。
/// kind 建议使用小写短标签，如 "work_log" / "invest_log" / "life_log" / "preference"。
pub fn add_semantic_item(kind: &str, content: &str, tags: Option<&str>) -> Result<()> {
    init_schema()?;
    let conn = open_db()?;
    let ts = now_unix_ts();
    conn.execute(
        r#"
        INSERT INTO semantic_items (kind, content, tags, created_ts)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![kind, content, tags.unwrap_or(""), ts],
    )
    .context("failed to insert semantic item")?;
    Ok(())
}

/// 语义记忆条目结构，供查询使用。
#[derive(Debug, Clone)]
pub struct SemanticItem {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub tags: Option<String>,
    pub created_ts: i64,
}

/// 简单基于 LIKE 的语义记忆检索接口。
///
/// - kind: 若提供，则仅在对应 kind 中检索；否则在全部语义记忆中检索。
/// - query: 关键字子串；若为空字符串，则按时间倒序取最近的若干条。
/// - limit: 返回条数上限。
pub fn search_semantic_items(
    kind: Option<&str>,
    query: &str,
    limit: usize,
) -> Result<Vec<SemanticItem>> {
    init_schema()?;
    let conn = open_db()?;

    let limit_i64 = if limit == 0 { 10 } else { limit as i64 };
    let mut items = Vec::new();

    if query.trim().is_empty() {
        // 仅按时间倒序返回最近的若干条。
        if let Some(k) = kind {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, kind, content, tags, created_ts
                FROM semantic_items
                WHERE kind = ?
                ORDER BY created_ts DESC
                LIMIT ?
                "#,
            )?;
            let rows = stmt.query_map(params![k, limit_i64], |row| {
                Ok(SemanticItem {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    content: row.get(2)?,
                    tags: row.get::<_, String>(3).ok(),
                    created_ts: row.get(4)?,
                })
            })?;
            for it in rows.flatten() {
                items.push(it);
            }
        } else {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, kind, content, tags, created_ts
                FROM semantic_items
                ORDER BY created_ts DESC
                LIMIT ?
                "#,
            )?;
            let rows = stmt.query_map(params![limit_i64], |row| {
                Ok(SemanticItem {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    content: row.get(2)?,
                    tags: row.get::<_, String>(3).ok(),
                    created_ts: row.get(4)?,
                })
            })?;
            for it in rows.flatten() {
                items.push(it);
            }
        }
        return Ok(items);
    }

    let pattern = format!("%{}%", query.trim());

    if let Some(k) = kind {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, content, tags, created_ts
            FROM semantic_items
            WHERE kind = ?1 AND content LIKE ?2
            ORDER BY created_ts DESC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(params![k, pattern, limit_i64], |row| {
            Ok(SemanticItem {
                id: row.get(0)?,
                kind: row.get(1)?,
                content: row.get(2)?,
                tags: row.get::<_, String>(3).ok(),
                created_ts: row.get(4)?,
            })
        })?;
        for it in rows.flatten() {
            items.push(it);
        }
    } else {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, content, tags, created_ts
            FROM semantic_items
            WHERE content LIKE ?1
            ORDER BY created_ts DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![pattern, limit_i64], |row| {
            Ok(SemanticItem {
                id: row.get(0)?,
                kind: row.get(1)?,
                content: row.get(2)?,
                tags: row.get::<_, String>(3).ok(),
                created_ts: row.get(4)?,
            })
        })?;
        for it in rows.flatten() {
            items.push(it);
        }
    }

    Ok(items)
}

/// 根据 id 删除一条语义记忆。
///
/// 返回值表示是否实际删除了某条记录（id 是否存在）。
pub fn delete_semantic_item(id: i64) -> Result<bool> {
    init_schema()?;
    let conn = open_db()?;
    let rows_affected = conn
        .execute(
            r#"
            DELETE FROM semantic_items
            WHERE id = ?
            "#,
            params![id],
        )
        .context("failed to delete semantic item")?;
    Ok(rows_affected > 0)
}

/// 便捷函数：记录一次「外部触发的整理动作」到审计日志时需要的时间戳。
pub fn now_unix_ts() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}
