use serde::Serialize;
use tauri::State;

use crate::memory::system::TaskRecord;

use super::AppState;

#[derive(Serialize)]
pub struct TaskItem {
    pub id: String,
    pub description: String,
    pub assigned_pup: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub result: Option<String>,
}

impl From<TaskRecord> for TaskItem {
    fn from(r: TaskRecord) -> Self {
        Self {
            id: r.id,
            description: r.description,
            assigned_pup: r.assigned_pup,
            status: r.status,
            created_at: r.created_at,
            completed_at: r.completed_at,
            result: r.result,
        }
    }
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    description: String,
    assigned_pup: Option<String>,
) -> Result<String, String> {
    state
        .app
        .create_task(&description, assigned_pup.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>, limit: i64) -> Result<Vec<TaskItem>, String> {
    let rows = state
        .app
        .list_tasks(limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(TaskItem::from).collect())
}

#[tauri::command]
pub async fn update_task_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
    result: Option<String>,
) -> Result<(), String> {
    state
        .app
        .update_task_status(&id, &status, result.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_task(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.app.delete_task(&id).await.map_err(|e| e.to_string())
}
