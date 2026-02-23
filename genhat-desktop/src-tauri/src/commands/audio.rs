//! Audio commands — TTS generation and speech-to-text.
//!
//! These are convenience wrappers around route_request for audio-specific tasks.

use crate::commands::inference::TaskRouterState;
use crate::registry::types::{TaskRequest, TaskType};
use std::collections::HashMap;
use tauri::State;

/// Generate speech from text using the KittenTTS engine.
///
/// # Arguments
/// * `input` — Text to synthesize
/// * `voice` — Optional voice name (e.g. "Leo", "Bella")
/// * `speed` — Optional speaking speed (e.g. 1.0)
///
/// # Returns
/// Absolute path to the generated `.wav` file.
#[tauri::command]
pub async fn generate_speech(
    input: String,
    voice: Option<String>,
    speed: Option<f32>,
    router_state: State<'_, TaskRouterState>,
) -> Result<String, String> {
    let mut extra = HashMap::new();

    if let Some(v) = voice {
        extra.insert("voice".to_string(), v);
    }
    if let Some(s) = speed {
        extra.insert("speed".to_string(), s.to_string());
    }

    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Tts,
        input,
        model_override: None,
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
