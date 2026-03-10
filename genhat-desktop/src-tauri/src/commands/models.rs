//! Model management commands — list, start, stop, status.

use crate::process::ProcessManager;
use crate::registry::types::{
    BackendKind, ModelDef, ModelInfo, ModelKind, ModelStatus, TaskType,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Managed state wrapper for the ProcessManager.
pub struct ProcessManagerState(pub Arc<ProcessManager>);

// ── Legacy-compatible model listing ─────────────────────────────────────────
// These maintain backward compatibility with the existing frontend.

#[derive(serde::Serialize)]
pub struct ModelFile {
    pub name: String,
    pub path: String,
}

/// List available .gguf LLM model files from the LiquidAI-LLM subfolder.
/// Legacy-compatible with the original `list_models` command.
#[tauri::command]
pub fn list_models() -> Vec<ModelFile> {
    let dir = get_models_dir();
    let llm_dir = dir.join("LLM");
    let mut models = Vec::new();

    // Scan the LiquidAI-LLM subfolder for .gguf files
    if let Ok(entries) = std::fs::read_dir(&llm_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("gguf") {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    models.push(ModelFile {
                        name: name.to_string(),
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    models
}

/// List available .gguf VLM model files from the LiquidAI-VLM subfolder.
/// Excludes mmproj files (those are companion projector weights, not selectable models).
#[tauri::command]
pub fn list_vision_models() -> Vec<ModelFile> {
    let dir = get_models_dir();
    let vlm_dir = dir.join("LiquidAI-VLM");
    let mut models = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&vlm_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("gguf") {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    // Exclude mmproj companion files
                    if name.starts_with("mmproj") {
                        continue;
                    }
                    models.push(ModelFile {
                        name: name.to_string(),
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }
    models
}

// ── New unified model commands ──────────────────────────────────────────────

/// List all registered models with their current status.
#[tauri::command]
pub async fn list_registered_models(
    state: State<'_, ProcessManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    Ok(state.0.list_models().await)
}

/// Get the status of a specific model.
#[tauri::command]
pub async fn get_model_status(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<ModelStatus, String> {
    state
        .0
        .model_status(&model_id)
        .await
        .ok_or_else(|| format!("Model '{model_id}' not found"))
}

/// Manually start (pre-warm) a model.
#[tauri::command]
pub async fn start_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<String, String> {
    let instance_id = state.0.ensure_running(&model_id, false).await?;
    Ok(instance_id)
}

/// Stop a model (all instances).
#[tauri::command]
pub async fn stop_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<(), String> {
    state.0.stop_model(&model_id).await
}

/// Switch to a different LLM model by file path or registry ID.
/// If it's a registered ID, it uses the config from models.toml.
/// Otherwise, it falls back to dynamically registering the model from the file path.
#[tauri::command]
pub async fn switch_model(
    model_identifier: String,
    state: State<'_, ProcessManagerState>,
) -> Result<String, String> {
    // Stop the currently-active LLM
    let prev_id = state.0.active_llm_id().await;
    let _ = state.0.stop_model(&prev_id).await;

    // First check if the identifier is a registered model ID
    if let Some(def) = state.0.get_model_def(&model_identifier).await {
        let instance_id = state.0.ensure_running(&def.id, false).await?;
        state.0.set_active_llm(&def.id).await;
        return Ok(format!("server started (instance: {})", &instance_id[..8]));
    }

    // Fallback: assume it's a file path for dynamic registration
    let path = PathBuf::from(&model_identifier);
    if !path.exists() {
        return Err(format!("Model file or ID not found: {model_identifier}"));
    }

    // Derive a model ID from the filename (e.g. "MyModel-Q4_0.gguf" → "mymodel-q4_0")
    let file_name = path
        .file_stem()
        .ok_or("Invalid model path: no filename")?
        .to_string_lossy()
        .to_string();
    let model_id = file_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>();

    // Build a ModelDef for the new model.
    // Use the absolute path as model_file — Path::join with an absolute path
    // ignores the base, so the backend will use it directly.
    // NOTE: Enrich is excluded — the dedicated enrichment model (LFM) handles
    // that in the background so it doesn't block the user's chat model.
    let def = ModelDef {
        id: model_id.clone(),
        name: file_name.clone(),
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: model_identifier.clone(),
        tasks: vec![
            TaskType::Chat,
            TaskType::Summarize,
            TaskType::Mindmap,
            TaskType::Grade,
            TaskType::Hyde,
        ],
        auto_start: false,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 10,
        memory_mb: 1400,
        params: HashMap::new(),
        task_priorities: HashMap::new(),
    };

    // Register and start the new model
    state.0.register_model(def).await?;
    let instance_id = state.0.ensure_running(&model_id, false).await?;
    state.0.set_active_llm(&model_id).await;

    Ok(format!("server started (instance: {})", &instance_id[..8]))
}

/// Stop the LLM server. Legacy-compatible.
#[tauri::command]
pub async fn stop_llama(state: State<'_, ProcessManagerState>) -> Result<(), String> {
    let active_id = state.0.active_llm_id().await;
    state.0.stop_model(&active_id).await
}

/// Get the port of the running llama-server (for frontend SSE streaming).
#[tauri::command]
pub async fn get_llama_port(
    state: State<'_, ProcessManagerState>,
) -> Result<u16, String> {
    let active_id = state.0.active_llm_id().await;
    // Ensure it's running first
    let _ = state.0.ensure_running(&active_id, false).await?;
    state
        .0
        .get_llama_port(&active_id)
        .await
        .ok_or_else(|| "LLM not running or no port assigned".to_string())
}

/// Get estimated total memory usage of all loaded models (MB).
#[tauri::command]
pub async fn get_memory_usage(
    state: State<'_, ProcessManagerState>,
) -> Result<u32, String> {
    Ok(state.0.memory_usage().await)
}

// ── Helper ──────────────────────────────────────────────────────────────────

/// Resolve the models directory.
///
/// Resolution order:
///   1. `GENHAT_MODEL_PATH` environment variable (absolute path to dir or any file inside it)
///   2. `models/` folder next to the running executable (production install location)
///   3. Tauri resource directory (Linux: `/usr/lib/GenHat/models/`)
///   4. `../../models` relative to the crate root at compile time (dev / cargo run fallback)
pub fn get_models_dir() -> PathBuf {
    crate::paths::resolve_models_dir()
}

// ── File utilities ──────────────────────────────────────────────────────────

use base64::{engine::general_purpose::STANDARD, Engine};

/// Read an image file and return it as a base64-encoded data URL.
/// Used for image preview in the frontend.
#[tauri::command]
pub fn read_image_base64(path: String) -> Result<String, String> {
    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    // Determine MIME type from extension
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };

    let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {e}"))?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("data:{};base64,{}", mime, b64))
}
