//! Workspace commands — create/list/open and metadata wiring for .nela persistence.

use crate::commands::inference::TaskRouterState;
use crate::commands::rag::RagPipelineState;
use crate::rag::pipeline::RagPipeline;
use crate::workspace::{WorkspaceManager, WorkspaceOpenResult, WorkspaceRecord};
use std::sync::Arc;
use tauri::State;

/// Tauri-managed workspace state.
pub struct WorkspaceState(pub Arc<WorkspaceManager>);

/// List all known workspaces.
#[tauri::command]
pub fn list_workspaces(state: State<'_, WorkspaceState>) -> Result<Vec<WorkspaceRecord>, String> {
    state.0.list_workspaces()
}

/// Get the currently active workspace.
#[tauri::command]
pub fn get_active_workspace(state: State<'_, WorkspaceState>) -> Result<WorkspaceRecord, String> {
    state.0.get_active_workspace()
}

/// Create a new workspace and make it active.
#[tauri::command]
pub fn create_workspace(
    name: Option<String>,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    let ws = state.0.create_workspace(name)?;
    reload_rag_for_active_workspace(&state, &rag_state, &router_state)?;
    Ok(ws)
}

/// Open an existing workspace by id and make it active.
#[tauri::command]
pub fn open_workspace(
    workspace_id: String,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    let ws = state.0.open_workspace(&workspace_id)?;
    reload_rag_for_active_workspace(&state, &rag_state, &router_state)?;
    Ok(ws)
}

/// Associate a workspace with its saved .nela file path.
#[tauri::command]
pub fn set_workspace_file(
    workspace_id: String,
    nela_path: String,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceRecord, String> {
    state.0.set_workspace_file(&workspace_id, &nela_path)
}

/// Read currently active workspace frontend state blob.
#[tauri::command]
pub fn get_workspace_frontend_state(
    state: State<'_, WorkspaceState>,
) -> Result<Option<String>, String> {
    state.0.get_active_frontend_state()
}

/// Persist currently active workspace frontend state blob.
#[tauri::command]
pub fn save_workspace_frontend_state(
    frontend_state_json: String,
    state: State<'_, WorkspaceState>,
) -> Result<(), String> {
    state.0.save_active_frontend_state(&frontend_state_json)
}

/// Save active workspace cache and metadata into a target .nela file.
#[tauri::command]
pub fn save_workspace_as_nela(
    nela_path: String,
    frontend_state_json: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceRecord, String> {
    state
        .0
        .save_active_workspace_as_nela(&nela_path, frontend_state_json.as_deref())
}

#[tauri::command]
pub fn delete_workspace(
    workspace_id: String,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    let active = state.0.delete_workspace(&workspace_id)?;
    reload_rag_for_active_workspace(&state, &rag_state, &router_state)?;
    Ok(active)
}

/// Save active workspace into its already-associated .nela path.
#[tauri::command]
pub fn save_workspace_nela(
    frontend_state_json: Option<String>,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceRecord, String> {
    state
        .0
        .save_active_workspace_nela(frontend_state_json.as_deref())
}

/// Open/import a .nela file into a workspace and make it active.
#[tauri::command]
pub fn open_workspace_nela(
    nela_path: String,
    name: Option<String>,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceOpenResult, String> {
    let out = state.0.open_workspace_nela(&nela_path, name)?;
    reload_rag_for_active_workspace(&state, &rag_state, &router_state)?;
    Ok(out)
}

fn reload_rag_for_active_workspace(
    workspace_state: &State<'_, WorkspaceState>,
    rag_state: &State<'_, RagPipelineState>,
    router_state: &State<'_, TaskRouterState>,
) -> Result<(), String> {
    let rag_dir = workspace_state.0.active_rag_dir()?;
    let new_pipeline = Arc::new(
        RagPipeline::open(&rag_dir, router_state.0.clone())
            .map_err(|e| format!("Failed to re-open RAG pipeline for workspace cache {}: {e}", rag_dir.display()))?,
    );
    rag_state.replace_pipeline(new_pipeline)
}
