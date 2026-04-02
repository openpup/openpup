use serde::Serialize;
use tauri::State;

use super::AppState;

#[tauri::command]
pub async fn kb_ingest_file(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    path: String,
    title: Option<String>,
    tags: Vec<String>,
) -> Result<String, String> {
    let handle = app_handle.clone();
    let source_id = state
        .app
        .ingest_knowledge_file(path, title, tags, move |evt| {
            let _ = tauri::Emitter::emit(&handle, "kb_ingest_progress", &evt);
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(source_id)
}

#[tauri::command]
pub async fn kb_list_sources(
    state: State<'_, AppState>,
) -> Result<Vec<crate::knowledge::types::KnowledgeSource>, String> {
    state
        .app
        .list_knowledge_sources()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_delete_source(state: State<'_, AppState>, source_id: String) -> Result<(), String> {
    state
        .app
        .delete_knowledge_source(&source_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<crate::knowledge::types::KbSearchResult>, String> {
    state
        .app
        .kb_search(&query, limit.unwrap_or(10))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn kb_get_auto_ingest(state: State<'_, AppState>) -> bool {
    state.app.kb_auto_ingest()
}

#[tauri::command]
pub fn kb_set_auto_ingest(state: State<'_, AppState>, enabled: bool) {
    state.app.set_kb_auto_ingest(enabled);
}

// ─── Knowledge Graph ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct KgEntityInfo {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub description: Option<String>,
    pub relations: Vec<KgRelationInfo>,
}

#[derive(Serialize)]
pub struct KgRelationInfo {
    pub relation: String,
    pub other_name: String,
    pub other_type: String,
    pub direction: String,
    pub confidence: f32,
}

#[tauri::command]
pub async fn kg_list_entities(
    state: State<'_, AppState>,
    entity_type: Option<String>,
) -> Result<Vec<KgEntityInfo>, String> {
    let entities = state
        .app
        .list_kg_entities(entity_type.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for (id, name, etype, desc) in entities {
        let rels = state.app.kg_entity_relations(&id).await.unwrap_or_default();
        result.push(KgEntityInfo {
            id,
            name,
            entity_type: etype,
            description: desc,
            relations: rels
                .into_iter()
                .map(
                    |(rel, other_name, other_type, direction, confidence)| KgRelationInfo {
                        relation: rel,
                        other_name,
                        other_type,
                        direction,
                        confidence,
                    },
                )
                .collect(),
        });
    }
    Ok(result)
}
