//! Model management commands — list, start, stop, status.

use crate::process::ProcessManager;
use crate::registry::types::{ModelInfo, ModelStatus};
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

/// List available .gguf LLM model files (excludes TTS models).
/// Legacy-compatible with the original `list_models` command.
#[tauri::command]
pub fn list_models() -> Vec<ModelFile> {
    let dir = get_models_dir();
    let mut models = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("gguf") {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    // Exclude TTS models
                    if name.starts_with("t3_")
                        || name.starts_with("s3gen")
                        || name.starts_with("ve_")
                    {
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

/// List available TTS audio model files.
/// Legacy-compatible with the original `list_audio_models` command.
#[tauri::command]
pub fn list_audio_models() -> Vec<ModelFile> {
    let dir = get_models_dir();
    let search_dirs = vec![dir.clone(), dir.join("tts-chatterbox-q4-k-m")];
    let mut models = Vec::new();

    for d in search_dirs {
        if let Ok(entries) = std::fs::read_dir(d) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("gguf") {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name.starts_with("s3gen") {
                            models.push(ModelFile {
                                name: name.to_string(),
                                path: path.to_string_lossy().to_string(),
                            });
                        }
                    }
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

/// Switch to a different LLM model by file path.
/// Legacy-compatible: kills existing llama-server, starts new one.
#[tauri::command]
pub async fn switch_model(
    model_path: String,
    state: State<'_, ProcessManagerState>,
) -> Result<String, String> {
    let path = PathBuf::from(&model_path);
    if !path.exists() {
        return Err(format!("Model file not found: {model_path}"));
    }

    // Stop the current LLM model
    state.0.stop_model("lfm-1_2b").await?;

    // Start the new one — re-use the lfm-1_2b slot
    // (In the future, this should create a dynamic registry entry)
    let instance_id = state.0.ensure_running("lfm-1_2b", false).await?;

    Ok(format!("server started (instance: {})", &instance_id[..8]))
}

/// Stop the LLM server. Legacy-compatible.
#[tauri::command]
pub async fn stop_llama(state: State<'_, ProcessManagerState>) -> Result<(), String> {
    state.0.stop_model("lfm-1_2b").await
}

/// Get the port of the running llama-server (for frontend SSE streaming).
#[tauri::command]
pub async fn get_llama_port(
    state: State<'_, ProcessManagerState>,
) -> Result<u16, String> {
    // Ensure it's running first
    let _ = state.0.ensure_running("lfm-1_2b", false).await?;
    state
        .0
        .get_llama_port("lfm-1_2b")
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

/// Resolve the models directory. Checks GENHAT_MODEL_PATH env var first,
/// then falls back to `../../models` relative to the crate root.
pub fn get_models_dir() -> PathBuf {
    if let Ok(val) = std::env::var("GENHAT_MODEL_PATH") {
        let p = PathBuf::from(val);
        if p.is_file() {
            if let Some(parent) = p.parent() {
                return parent.to_path_buf();
            }
        } else if p.is_dir() {
            return p;
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let models = manifest_dir.join("../../models");
    models.canonicalize().unwrap_or(models)
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
