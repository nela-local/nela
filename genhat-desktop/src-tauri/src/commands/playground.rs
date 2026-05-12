//! Playground Tauri commands — frontend-facing IPC API.

use crate::commands::inference::TaskRouterState;
use crate::playground::{
    executor::run_pipeline,
    store::PipelineStore,
    types::{Pipeline, PipelineRun},
};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;

// ─── State ────────────────────────────────────────────────────────────────────

pub struct PlaygroundState {
    pub store: Arc<PipelineStore>,
    /// Active run tracking — wrapped in Arc so it can be shared with spawned tasks.
    pub active_run: Arc<Mutex<Option<PipelineRun>>>,
    /// Cancel signal — send `true` to abort the current run.
    pub cancel_tx: Arc<tokio::sync::watch::Sender<bool>>,
}

impl PlaygroundState {
    pub fn new(app_data_dir: &PathBuf) -> Result<Self, String> {
        let store = PipelineStore::new(app_data_dir)
            .map_err(|e| format!("Failed to init pipeline store: {e}"))?;
        let (cancel_tx, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            store: Arc::new(store),
            active_run: Arc::new(Mutex::new(None)),
            cancel_tx: Arc::new(cancel_tx),
        })
    }
}

// ─── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn playground_list_pipelines(
    state: State<'_, PlaygroundState>,
) -> Result<Vec<Pipeline>, String> {
    state.store.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playground_load_pipeline(
    id: String,
    state: State<'_, PlaygroundState>,
) -> Result<Pipeline, String> {
    state.store.load(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playground_save_pipeline(
    pipeline: Pipeline,
    state: State<'_, PlaygroundState>,
) -> Result<(), String> {
    state.store.save(&pipeline).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playground_delete_pipeline(
    id: String,
    state: State<'_, PlaygroundState>,
) -> Result<(), String> {
    state.store.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playground_run_pipeline(
    pipeline_id: String,
    state: State<'_, PlaygroundState>,
    router_state: State<'_, TaskRouterState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let pipeline = state
        .store
        .load(&pipeline_id)
        .map_err(|e| format!("Pipeline not found: {e}"))?;

    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir error: {e}"))?;

    let router = router_state.0.clone();
    let active_run = state.active_run.clone();

    // Reset cancel flag so any previous cancellation doesn't bleed into this run.
    state.cancel_tx.send(false).ok();
    let cancel_rx = state.cancel_tx.subscribe();

    // Generate a stable run_id up front and return it immediately.
    // The pipeline runs in a background task and emits `playground-run-update`
    // / `playground-run-complete` events as each node finishes.
    let run_id = uuid::Uuid::new_v4().to_string();
    let run_id_clone = run_id.clone();

    tokio::spawn(async move {
        let run = run_pipeline(&pipeline, router, app_data_dir, app_handle, cancel_rx).await;
        let mut guard = active_run.lock().await;
        *guard = Some(run);
        log::info!("Pipeline run '{run_id_clone}' complete");
    });

    Ok(run_id)
}

#[tauri::command]
pub async fn playground_cancel_run(
    _run_id: String,
    state: State<'_, PlaygroundState>,
) -> Result<(), String> {
    // Signal the background task to stop between nodes.
    state.cancel_tx.send(true).ok();
    let mut guard = state.active_run.lock().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn playground_store_credential(
    key: String,
    value: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir error: {e}"))?;

    let creds_dir = app_data_dir.join("creds");
    std::fs::create_dir_all(&creds_dir)
        .map_err(|e| format!("Failed to create creds dir: {e}"))?;

    // Sanitize key into a safe filename
    let safe_key: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let file_path = creds_dir.join(safe_key);

    std::fs::write(&file_path, value.as_bytes())
        .map_err(|e| format!("Failed to write credential: {e}"))?;

    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set cred permissions: {e}"))?;
    }

    Ok(())
}
