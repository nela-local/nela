//! Model management commands — list, start, stop, status.

use crate::process::ProcessManager;
use crate::commands::workspace::WorkspaceState;
use crate::registry::custom::{self, CustomModelEntry, CustomModelProfile};
use crate::registry::types::{
    BackendKind, ModelDef, ModelInfo, ModelKind, ModelStatus, TaskType,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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
    // Keep the previous model warm instead of stopping on every switch.

    // First check if the identifier is a registered model ID
    if let Some(def) = state.0.get_model_def(&model_identifier).await {
        let instance_id = state.0.ensure_running(&def.id, false).await?;
        if let Some(evict) = state.0.rotate_active_llm_keep_previous(&def.id).await {
            let _ = state.0.stop_model(&evict).await;
        }
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
        gdrive_id: None,
        is_zip: false,
    };

    // Register and start the new model
    state.0.register_model(def).await?;
    let instance_id = state.0.ensure_running(&model_id, false).await?;
    if let Some(evict) = state.0.rotate_active_llm_keep_previous(&model_id).await {
        let _ = state.0.stop_model(&evict).await;
    }

    Ok(format!("server started (instance: {})", &instance_id[..8]))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportModelProfile {
    Llm,
    Vlm,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ImportDownloadedModelRequest {
    pub folder: String,
    pub filename: String,
    pub profile: ImportModelProfile,
    pub display_name: Option<String>,
    pub mmproj_file: Option<String>,
    pub engine_adapter: Option<String>,
}

/// Import a downloaded GGUF model into the runtime and persist it in custom registry.
///
/// This enables downloaded Hugging Face models to be recognized by NELA
/// immediately without requiring app restart.
#[tauri::command]
pub async fn import_downloaded_model(
    req: ImportDownloadedModelRequest,
    state: State<'_, ProcessManagerState>,
) -> Result<ModelInfo, String> {
    let models_dir = crate::paths::resolve_models_dir();
    let model_rel = PathBuf::from(&req.folder).join(&req.filename);
    let model_path = models_dir.join(&model_rel);

    if !model_path.exists() {
        return Err(format!(
            "Model file not found: {}",
            model_path.display()
        ));
    }
    if model_path.extension().and_then(|s| s.to_str()) != Some("gguf") {
        return Err(format!(
            "Only .gguf models can be imported with this command: {}",
            model_path.display()
        ));
    }

    let model_id = build_custom_model_id(&req.folder, &req.filename);
    let display_name = req
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(&req.filename)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| req.filename.clone())
        });

    let mut params = HashMap::new();
    let engine_adapter = req
        .engine_adapter
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "llama_cpp".to_string());

    if engine_adapter != "llama_cpp" {
        return Err(format!(
            "Unsupported engine adapter '{}'. Currently supported: llama_cpp",
            engine_adapter
        ));
    }

    params.insert("engine_adapter".to_string(), engine_adapter.clone());

    match req.profile {
        ImportModelProfile::Llm => {
            params.insert("custom_profile".to_string(), "llm".to_string());
        }
        ImportModelProfile::Vlm => {
            params.insert("custom_profile".to_string(), "vlm".to_string());
            let mmproj = req
                .mmproj_file
                .clone()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    "VLM imports require an mmproj companion file. Provide mmproj_file.".to_string()
                })?;

            let mmproj_path = std::path::Path::new(&mmproj);
            let mmproj_exists = if mmproj_path.is_absolute() {
                mmproj_path.exists()
            } else {
                models_dir.join(&mmproj).exists()
            };
            if !mmproj_exists {
                return Err(format!(
                    "VLM companion file not found: {}",
                    mmproj
                ));
            }
            params.insert("mmproj_file".to_string(), mmproj);
        }
    }

    let tasks = match req.profile {
        ImportModelProfile::Llm => vec![
            TaskType::Chat,
            TaskType::Summarize,
            TaskType::Mindmap,
            TaskType::Grade,
            TaskType::Hyde,
            TaskType::PodcastScript,
        ],
        ImportModelProfile::Vlm => vec![TaskType::VisionChat],
    };

    let def = ModelDef {
        id: model_id.clone(),
        name: display_name.clone(),
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: model_rel.to_string_lossy().to_string(),
        tasks,
        auto_start: false,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 12,
        memory_mb: 1600,
        params: params.clone(),
        task_priorities: HashMap::new(),
        gdrive_id: None,
        is_zip: false,
    };

    state.0.register_model(def).await?;

    let entry = CustomModelEntry {
        id: model_id.clone(),
        name: display_name,
        model_file: model_rel.to_string_lossy().to_string(),
        profile: match req.profile {
            ImportModelProfile::Llm => CustomModelProfile::Llm,
            ImportModelProfile::Vlm => CustomModelProfile::Vlm,
        },
        engine_adapter,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 12,
        memory_mb: 1600,
        params,
    };

    custom::upsert_custom_model(&models_dir, entry)?;

    let imported = state
        .0
        .list_models()
        .await
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| "Imported model was not found in runtime list".to_string())?;

    Ok(imported)
}

/// Unregister a previously imported custom model.
#[tauri::command]
pub async fn unregister_custom_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<(), String> {
    let models_dir = crate::paths::resolve_models_dir();
    if !custom::is_custom_model(&models_dir, &model_id)? {
        return Err(format!("Model '{}' is not a custom imported model", model_id));
    }

    state.0.unregister_model(&model_id).await?;
    let _ = custom::remove_custom_model(&models_dir, &model_id)?;
    Ok(())
}

/// Stop the LLM server. Legacy-compatible.
#[tauri::command]
pub async fn stop_llama(state: State<'_, ProcessManagerState>) -> Result<(), String> {
    let active_id = state.0.active_llm_id().await;
    let prev_id = state.0.previous_llm_id().await;
    state.0.stop_model(&active_id).await?;
    if let Some(prev) = prev_id {
        if prev != active_id {
            let _ = state.0.stop_model(&prev).await;
        }
    }
    Ok(())
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

/// Return the current working directory so frontend state can be scoped per workspace.
#[tauri::command]
pub fn get_workspace_scope(workspace: State<'_, WorkspaceState>) -> Result<String, String> {
    workspace.0.get_workspace_scope()
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

fn build_custom_model_id(folder: &str, filename: &str) -> String {
    let stem = PathBuf::from(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.to_string());

    let normalized = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    folder.hash(&mut hasher);
    filename.hash(&mut hasher);
    let suffix = hasher.finish();

    format!("custom-{}-{:08x}", normalized, suffix as u32)
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
