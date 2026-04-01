use serde::Serialize;
use tauri::State;

use crate::memory::system::SkillRunRecord;
use crate::skills::registry::InstalledSkill;

use super::AppState;

fn registry<'a>(state: &'a State<'_, AppState>) -> &'a crate::skills::registry::SkillRegistry {
    &state.alpha.skill_executor.registry
}

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<InstalledSkill>, String> {
    Ok(registry(&state).list_installed().await)
}

#[tauri::command]
pub async fn set_skill_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    registry(&state)
        .set_enabled(&name, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_skill(
    state: State<'_, AppState>,
    name: String,
    input: String,
) -> Result<String, String> {
    state
        .alpha
        .skill_executor
        .execute_skill(&name, &input)
        .await
        .map_err(|e| e.to_string())
}

// ─── Skill run history ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SkillRunItem {
    pub id: String,
    pub skill_name: String,
    pub triggered_by: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
    pub output: Option<String>,
}

impl From<SkillRunRecord> for SkillRunItem {
    fn from(r: SkillRunRecord) -> Self {
        Self {
            id: r.id,
            skill_name: r.skill_name,
            triggered_by: r.triggered_by,
            started_at: r.started_at,
            completed_at: r.completed_at,
            status: r.status,
            output: r.output,
        }
    }
}

#[tauri::command]
pub async fn list_skill_runs(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<SkillRunItem>, String> {
    let memory = state.alpha.memory.clone();
    let rows = memory
        .list_skill_runs(limit)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(SkillRunItem::from).collect())
}

// ─── Scheduled jobs ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_scheduled_jobs(
    _state: State<'_, AppState>,
) -> Result<Vec<crate::skills::job_registry::ScheduledJob>, String> {
    let home_dir = dirs::home_dir().ok_or("cannot determine home directory")?;
    let jobs_path = home_dir.join(".openpup").join("scheduled_jobs.json");
    let registry = crate::skills::job_registry::JobRegistry::new(jobs_path);
    Ok(registry.load())
}

#[tauri::command]
pub async fn delete_scheduled_job(_state: State<'_, AppState>, id: String) -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("cannot determine home directory")?;
    let jobs_path = home_dir.join(".openpup").join("scheduled_jobs.json");
    let registry = crate::skills::job_registry::JobRegistry::new(jobs_path);
    registry.delete(&id).map_err(|e| e.to_string())
}
