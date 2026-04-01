use tauri::State;

use crate::skills::permissions::{ExecutionMode, PermissionChecker};

/// Called by the frontend when the user clicks "Allow" in the PermissionDialog.
#[tauri::command]
pub async fn approve_permission(
    checker: State<'_, PermissionChecker>,
    request_id: String,
    skill_name: String,
    remember: bool,
) -> Result<(), String> {
    if remember {
        checker.trust_skill(&skill_name).await;
    }
    checker.respond(&request_id, true);
    Ok(())
}

/// Called by the frontend when the user clicks "Deny" in the PermissionDialog.
#[tauri::command]
pub async fn deny_permission(
    checker: State<'_, PermissionChecker>,
    request_id: String,
) -> Result<(), String> {
    checker.respond(&request_id, false);
    Ok(())
}

/// Return the current execution mode ("leashed" | "free_run").
#[tauri::command]
pub async fn get_execution_mode(checker: State<'_, PermissionChecker>) -> Result<String, String> {
    let mode = checker.get_mode().await;
    Ok(match mode {
        ExecutionMode::Leashed => "leashed".to_string(),
        ExecutionMode::FreeRun => "free_run".to_string(),
    })
}

/// Switch between leashed and free_run modes.
#[tauri::command]
pub async fn set_execution_mode(
    checker: State<'_, PermissionChecker>,
    mode: String,
) -> Result<(), String> {
    let m = match mode.as_str() {
        "free_run" => ExecutionMode::FreeRun,
        _ => ExecutionMode::Leashed,
    };
    checker.set_mode(m).await;
    Ok(())
}
