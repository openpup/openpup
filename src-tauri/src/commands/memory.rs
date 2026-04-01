use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::memory::system::MemorySystem;

use super::AppState;

#[derive(Serialize)]
pub struct LongTermMemoryItem {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub importance: f32,
    pub created_at: i64,
    pub superseded_by: Option<String>,
}

fn memory_system(state: &State<'_, AppState>) -> Arc<MemorySystem> {
    state.alpha.memory.clone()
}

#[tauri::command]
pub async fn list_long_term_memories(
    state: State<'_, AppState>,
    offset: i64,
    limit: i64,
    query: Option<String>,
) -> Result<Vec<LongTermMemoryItem>, String> {
    let memory = memory_system(&state);
    let rows = memory
        .list_long_term_memories(offset, limit, query.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(
            |(id, content, memory_type, importance, created_at, superseded_by)| {
                LongTermMemoryItem {
                    id,
                    content,
                    memory_type,
                    importance,
                    created_at,
                    superseded_by,
                }
            },
        )
        .collect())
}

#[tauri::command]
pub async fn update_long_term_memory(
    state: State<'_, AppState>,
    id: String,
    content: String,
    memory_type: String,
    importance: f32,
) -> Result<(), String> {
    let memory = memory_system(&state);
    memory
        .update_long_term_memory(&id, &content, &memory_type, importance)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_long_term_memory(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let memory = memory_system(&state);
    memory
        .delete_long_term_memory(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Top memories by importance — shown as context chips in the chat header.
#[derive(Serialize)]
pub struct MemoryChip {
    pub content: String,
    pub memory_type: String,
    pub importance: f32,
}

#[tauri::command]
pub async fn get_top_memories(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<MemoryChip>, String> {
    let memory = memory_system(&state);
    let rows = memory
        .get_top_memories(limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(content, memory_type, importance)| MemoryChip {
            content,
            memory_type,
            importance,
        })
        .collect())
}
