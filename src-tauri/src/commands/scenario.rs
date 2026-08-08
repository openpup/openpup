use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSettingsSnapshot {
    pub finance: crate::config::FinanceScenarioConfig,
}

#[tauri::command]
pub fn get_scenario_settings_snapshot(state: State<'_, AppState>) -> ScenarioSettingsSnapshot {
    ScenarioSettingsSnapshot {
        finance: state.app.finance_scenario_config(),
    }
}

#[tauri::command]
pub async fn save_finance_scenario_settings(
    state: State<'_, AppState>,
    finance: crate::config::FinanceScenarioConfig,
) -> Result<ScenarioSettingsSnapshot, String> {
    let saved = state
        .app
        .save_finance_scenario_config(finance)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ScenarioSettingsSnapshot { finance: saved })
}
