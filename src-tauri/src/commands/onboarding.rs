use std::sync::Arc;

use serde::Deserialize;
use tauri::State;
use tracing::warn;

use crate::memory::system::MemorySystem;

use super::AppState;

#[tauri::command]
pub async fn check_onboarding_completed(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.file_layer.is_onboarding_completed())
}

#[derive(Deserialize)]
pub struct OnboardingData {
    pub name: String,
    pub boundaries: String,
    pub pain_points: String,
    pub language: String,
    pub work_schedule: String,
    pub tools: String,
}

#[tauri::command]
pub async fn save_onboarding_data(
    state: State<'_, AppState>,
    data: OnboardingData,
) -> Result<(), String> {
    let content = format!(
        "# Owner Profile\n\n\
     ## Name\n{}\n\n\
     ## Boundaries\n{}\n\n\
     ## Pain Points\n{}\n\n\
     ## Language\n{}\n\n\
     ## Work Schedule\n{}\n\n\
     ## Tools\n{}\n",
        data.name.trim(),
        data.boundaries.trim(),
        data.pain_points.trim(),
        data.language.trim(),
        data.work_schedule.trim(),
        data.tools.trim(),
    );
    state
        .file_layer
        .write_owner_profile(&content)
        .map_err(|e| e.to_string())?;

    // ── Skills path first-run init ────────────────────────────────────────────
    {
        let mut cfg = crate::config::load();
        if cfg.skills.search_paths.is_empty() {
            let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            let default_skills_dir = home.join(".openpup").join("skills");
            let _ = std::fs::create_dir_all(&default_skills_dir);
            cfg.skills.search_paths = vec!["~/.openpup/skills/".to_string()];
            if let Err(e) = crate::config::save(&cfg) {
                warn!("failed to persist default skills path: {e}");
            }
        }
    }

    // Seed long-term memory DB with the key facts from onboarding
    let memory: Arc<MemorySystem> = state.alpha.memory.clone();
    let _ = memory
        .add_long_term_memory(
            &format!("行为边界：{}", data.boundaries.trim()),
            "rule",
            0.99,
        )
        .await;
    if !data.pain_points.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("常见痛点/重复工作：{}", data.pain_points.trim()),
                "fact",
                0.85,
            )
            .await;
    }
    if !data.language.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("语言偏好：{}", data.language.trim()),
                "preference",
                0.95,
            )
            .await;
    }
    if !data.name.trim().is_empty() {
        let _ = memory
            .add_long_term_memory(
                &format!("用户名字/称呼：{}", data.name.trim()),
                "preference",
                0.99,
            )
            .await;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_owner_profile(state: State<'_, AppState>) -> Result<String, String> {
    state
        .file_layer
        .read_owner_profile()
        .map_err(|e| e.to_string())
}
