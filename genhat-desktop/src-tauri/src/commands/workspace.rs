//! Workspace commands — create/list/open and metadata wiring for .nela persistence.

use crate::commands::inference::TaskRouterState;
use crate::commands::rag::RagPipelineState;
use crate::rag::pipeline::RagPipeline;
use crate::workspace::{RagModelPreferences, WorkspaceManager, WorkspaceOpenResult, WorkspaceRecord};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

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

/// Clear the active workspace (allows startup modal to appear on next app load).
#[tauri::command]
pub fn clear_active_workspace(state: State<'_, WorkspaceState>) -> Result<(), String> {
    state.0.clear_active_workspace()
}

/// Create a new workspace and make it active.
#[tauri::command]
pub fn create_workspace(
    name: Option<String>,
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    let ws = state.0.create_workspace(name)?;
    reload_rag_for_active_workspace(&app, &state, &rag_state, &router_state)?;
    Ok(ws)
}

/// Open an existing workspace by id and make it active.
#[tauri::command]
pub fn open_workspace(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    let ws = state.0.open_workspace(&workspace_id)?;
    reload_rag_for_active_workspace(&app, &state, &rag_state, &router_state)?;
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

/// Rename a workspace and persist the change in the workspace registry.
#[tauri::command]
pub fn rename_workspace(
    workspace_id: String,
    name: String,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceRecord, String> {
    state.0.rename_workspace(&workspace_id, &name)
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
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceRecord, String> {
    // Stop the enrichment worker BEFORE deleting files to release DB handles
    if let Ok(pipeline) = rag_state.active_pipeline() {
        pipeline.stop_enrichment();
        // Give the worker a moment to stop and release resources
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    
    let active = state.0.delete_workspace(&workspace_id)?;
    reload_rag_for_active_workspace(&app, &state, &rag_state, &router_state)?;
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
    app: AppHandle,
    state: State<'_, WorkspaceState>,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<WorkspaceOpenResult, String> {
    let out = state.0.open_workspace_nela(&nela_path, name)?;
    reload_rag_for_active_workspace(&app, &state, &rag_state, &router_state)?;
    Ok(out)
}

/// Get RAG model preferences for a workspace.
#[tauri::command]
pub fn get_rag_model_preferences(
    workspace_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<RagModelPreferences, String> {
    state.0.get_rag_model_preferences(&workspace_id)
}

/// Save RAG model preferences for a workspace.
#[tauri::command]
pub fn save_rag_model_preferences(
    workspace_id: String,
    prefs: RagModelPreferences,
    state: State<'_, WorkspaceState>,
) -> Result<(), String> {
    state.0.save_rag_model_preferences(&workspace_id, &prefs)
}

fn reload_rag_for_active_workspace(
    app: &AppHandle,
    workspace_state: &State<'_, WorkspaceState>,
    rag_state: &State<'_, RagPipelineState>,
    router_state: &State<'_, TaskRouterState>,
) -> Result<(), String> {
    let rag_dir = workspace_state.0.active_rag_dir()?;

    // If we're reopening the same workspace, keep the existing pipeline alive.
    // Reopening Tantivy on the same directory while the previous handle exists
    // can fail and block workspace activation.
    let active_rag_dir = rag_state.active_data_dir()?;
    if active_rag_dir == rag_dir {
        // Emit ready even when keeping existing pipeline
        let workspace_id = workspace_state.0.active_workspace_id().unwrap_or_default();
        let _ = app.emit("workspace-ready", serde_json::json!({
            "workspace_id": workspace_id,
            "status": "ready"
        }));
        return Ok(());
    }

    let new_pipeline = Arc::new(
        RagPipeline::open(&rag_dir, router_state.0.clone())
            .map_err(|e| format!("Failed to re-open RAG pipeline for workspace cache {}: {e}", rag_dir.display()))?,
    );
    
    // Replace pipeline (this stops the old enrichment worker)
    rag_state.replace_pipeline(new_pipeline.clone())?;
    
    // Start new enrichment worker for the new pipeline
    RagPipeline::start_enrichment_worker(new_pipeline, app.clone());
    log::info!("Started new enrichment worker for workspace");
    
    // Emit workspace-ready event so frontend knows it's safe to restore state
    let workspace_id = workspace_state.0.active_workspace_id().unwrap_or_default();
    let _ = app.emit("workspace-ready", serde_json::json!({
        "workspace_id": workspace_id,
        "status": "ready"
    }));
    
    Ok(())
}
