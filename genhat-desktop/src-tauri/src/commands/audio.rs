//! Audio commands — TTS generation and speech-to-text.
//!
//! These are convenience wrappers around route_request for audio-specific tasks.

use crate::commands::inference::TaskRouterState;
use crate::registry::types::{TaskRequest, TaskType};
use std::collections::HashMap;
use tauri::State;

/// Generate speech from text using the TTS model.
/// Legacy-compatible with the original `generate_speech` command.
///
/// # Arguments
/// * `model_path` — Path to the S3Gen GGUF model file
/// * `input` — Text to synthesize
///
/// # Returns
/// Absolute path to the generated `.wav` file.
#[tauri::command]
pub async fn generate_speech(
    model_path: String,
    input: String,
    router_state: State<'_, TaskRouterState>,
) -> Result<String, String> {
    let mut extra = HashMap::new();
    extra.insert("model_path".to_string(), model_path.clone());

    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Tts,
        input,
        model_override: None, // Add the model at later point of time
        extra,
    };

    match router_state.0.route(&request).await? {
        crate::registry::types::TaskResponse::FilePath(path) => Ok(path),
        other => Err(format!("Unexpected TTS response: {other:?}")),
    }
}

/// Transcribe an audio file to text using Whisper.
///
/// # Arguments
/// * `audio_path` — Absolute path to the audio file
///
/// # Returns
/// Transcription result with timestamps.
#[tauri::command]
pub async fn transcribe_audio(
    audio_path: String,
    router_state: State<'_, TaskRouterState>,
) -> Result<crate::registry::types::TaskResponse, String> {
    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Transcribe,
        input: audio_path,
        model_override: None,
        extra: HashMap::new(),
    };

    router_state.0.route(&request).await
}
