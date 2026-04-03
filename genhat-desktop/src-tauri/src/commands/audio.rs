//! Audio commands — TTS generation and speech-to-text.
//!
//! These are convenience wrappers around route_request for audio-specific tasks.

use crate::commands::inference::TaskRouterState;
use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;
use tauri::State;
use tempfile::NamedTempFile;

/// Generate speech from text using the KittenTTS engine.
///
/// # Arguments
/// * `input` — Text to synthesize
/// * `voice` — Optional voice name (e.g. "Leo", "Bella")
/// * `speed` — Optional speaking speed (e.g. 1.0)
///
/// # Returns
/// A `data:audio/wav;base64,…` URL that can be used directly in an `<audio>` element.
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

    let file_path = match router_state.0.route(&request).await? {
        crate::registry::types::TaskResponse::FilePath(path) => path,
        other => return Err(format!("Unexpected TTS response: {other:?}")),
    };

    // Read the WAV file and return it as a base64 data URL so the webview can
    // play it without needing asset-protocol scope permissions.
    let wav_bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read generated WAV file: {e}"))?;
    let b64 = STANDARD.encode(&wav_bytes);

    // Clean up the temp file (best-effort)
    let _ = std::fs::remove_file(&file_path);

    Ok(format!("data:audio/wav;base64,{b64}"))
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

/// Read an audio file and return it as a base64-encoded data URL.
///
/// This avoids the need for the Tauri asset protocol scope — the webview
/// can play the audio directly from the data URL.
#[tauri::command]
pub fn read_audio_base64(path: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(format!("Audio file not found: {}", path.display()));
    }

    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("ogg") | Some("opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("aac") | Some("m4a") => "audio/aac",
        _ => "audio/wav",
    };

    let data = std::fs::read(path).map_err(|e| format!("Failed to read audio file: {e}"))?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("data:{};base64,{}", mime, b64))
}

/// Transcribe audio from base64-encoded WAV data.
///
/// This is used for real-time voice input from the browser's microphone.
/// The frontend records audio, converts to WAV, base64 encodes it, and
/// sends it here for transcription.
///
/// # Arguments
/// * `audio_base64` — Base64-encoded WAV audio data (without data URL prefix)
///
/// # Returns
/// Transcribed text from the audio.
#[tauri::command]
pub async fn transcribe_audio_base64(
    audio_base64: String,
    router_state: State<'_, TaskRouterState>,
) -> Result<String, String> {
    // Decode base64 to bytes
    let audio_bytes = STANDARD
        .decode(&audio_base64)
        .map_err(|e| format!("Failed to decode base64 audio: {e}"))?;

    // Write to a temporary file (Parakeet expects a file path)
    let temp_file = NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp file: {e}"))?;
    let temp_path = temp_file.path().to_path_buf();
    
    // Keep temp file path but allow the file to be written to
    let temp_path_str = temp_path.to_string_lossy().to_string();
    let wav_path = format!("{}.wav", temp_path_str);
    
    std::fs::write(&wav_path, &audio_bytes)
        .map_err(|e| format!("Failed to write temp audio file: {e}"))?;

    // Route the transcription request
    let request = TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Transcribe,
        input: wav_path.clone(),
        model_override: None,
        extra: HashMap::new(),
    };

    let result = router_state.0.route(&request).await;

    // Clean up temp file
    let _ = std::fs::remove_file(&wav_path);

    match result {
        Ok(TaskResponse::Text(text)) => Ok(text),
        Ok(other) => Err(format!("Unexpected transcription response: {other:?}")),
        Err(e) => Err(e),
    }
}

/// Generate speech chunk by chunk for streaming TTS.
///
/// Splits the input text into sentences and generates audio for each one,
/// allowing the frontend to play audio as it's generated.
///
/// # Arguments
/// * `text` — Text to synthesize
/// * `voice` — Voice name (e.g. "Leo", "Bella")
/// * `speed` — Speaking speed (e.g. 1.0)
///
/// # Returns
/// A base64-encoded WAV audio chunk for this sentence.
#[tauri::command]
pub async fn generate_speech_chunk(
    text: String,
    voice: Option<String>,
    speed: Option<f32>,
    router_state: State<'_, TaskRouterState>,
) -> Result<String, String> {
    // Skip empty text
    if text.trim().is_empty() {
        return Ok(String::new());
    }

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
        input: text,
        model_override: None,
        extra,
    };

    let file_path = match router_state.0.route(&request).await? {
        TaskResponse::FilePath(path) => path,
        other => return Err(format!("Unexpected TTS response: {other:?}")),
    };

    // Read the WAV file and return as base64
    let wav_bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read generated WAV file: {e}"))?;
    let b64 = STANDARD.encode(&wav_bytes);

    // Clean up temp file
    let _ = std::fs::remove_file(&file_path);

    Ok(format!("data:audio/wav;base64,{b64}"))
}
