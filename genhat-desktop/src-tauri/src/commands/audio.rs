//! Audio commands — TTS generation and speech-to-text.
//!
//! These are convenience wrappers around route_request for audio-specific tasks.

use crate::commands::inference::TaskRouterState;
use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::State;
use tempfile::NamedTempFile;

// ── Native microphone recording state ──

/// Shared state for native microphone recording via cpal.
/// The WebView on macOS doesn't expose navigator.mediaDevices,
/// so we record audio natively and send it to the frontend.
///
/// cpal::Stream is !Send, so we run the recording on a dedicated thread
/// and use a channel to signal stop.
pub struct MicRecorderState(pub Arc<Mutex<Option<MicRecorderHandle>>>);

pub struct MicRecorderHandle {
    /// Recorded samples (mono, at the device's native sample rate).
    samples: Arc<Mutex<Vec<f32>>>,
    /// The device's native sample rate.
    sample_rate: Arc<Mutex<u32>>,
    /// Send () to signal the recording thread to stop.
    stop_tx: std::sync::mpsc::Sender<()>,
    /// Wait for the recording thread to finish.
    join_handle: Option<std::thread::JoinHandle<Result<(), String>>>,
}

// Safety: The non-Send cpal::Stream lives exclusively on the recording thread.
// MicRecorderHandle only holds Send types (Arc, Sender, JoinHandle).
unsafe impl Send for MicRecorderHandle {}
unsafe impl Sync for MicRecorderHandle {}

impl Default for MicRecorderState {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
}

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
        Ok(TaskResponse::Transcription { segments }) => {
            let text = segments
                .into_iter()
                .map(|s| s.text)
                .collect::<Vec<_>>()
                .join(" ");
            Ok(text)
        }
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

// ── Native microphone recording commands ──

/// Start recording audio from the default input device.
///
/// Audio is captured on a dedicated thread (cpal::Stream is !Send) at
/// the device's native sample rate. Call `stop_mic_recording` to stop
/// and retrieve the recorded audio as base64-encoded 16 kHz mono WAV.
#[tauri::command]
pub fn start_mic_recording(
    recorder_state: State<'_, MicRecorderState>,
) -> Result<(), String> {
    let mut guard = recorder_state.0.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err("Recording is already in progress".into());
    }

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let sample_rate_out: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

    let samples_for_thread = samples.clone();
    let sr_for_thread = sample_rate_out.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

    let join_handle = std::thread::spawn(move || -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No microphone found. Please connect a microphone.")?;

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get input config: {e}"))?;

        let sr = config.sample_rate().0;
        *sr_for_thread.lock().map_err(|e| e.to_string())? = sr;
        let channels = config.channels() as usize;
        let buf = samples_for_thread;

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let buf2 = buf.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data.iter().step_by(channels).copied().collect();
                        if let Ok(mut b) = buf2.lock() {
                            b.extend_from_slice(&mono);
                        }
                    },
                    |err| log::error!("Mic stream error: {err}"),
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let buf2 = buf.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data
                            .iter()
                            .step_by(channels)
                            .map(|&s| s as f32 / i16::MAX as f32)
                            .collect();
                        if let Ok(mut b) = buf2.lock() {
                            b.extend_from_slice(&mono);
                        }
                    },
                    |err| log::error!("Mic stream error: {err}"),
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let buf2 = buf.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data
                            .iter()
                            .step_by(channels)
                            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect();
                        if let Ok(mut b) = buf2.lock() {
                            b.extend_from_slice(&mono);
                        }
                    },
                    |err| log::error!("Mic stream error: {err}"),
                    None,
                )
            }
            fmt => return Err(format!("Unsupported sample format: {fmt:?}")),
        }
        .map_err(|e| format!("Failed to open microphone stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start recording: {e}"))?;

        log::info!("Mic recording started (sample rate: {sr} Hz)");

        // Block this thread until stop is signalled (stream stays alive).
        let _ = stop_rx.recv();
        // stream is dropped here, stopping the capture.
        Ok(())
    });

    *guard = Some(MicRecorderHandle {
        samples,
        sample_rate: sample_rate_out,
        stop_tx,
        join_handle: Some(join_handle),
    });

    Ok(())
}

/// Stop recording and return the audio as a base64-encoded 16 kHz mono WAV.
///
/// The WAV data can be sent directly to `transcribe_audio_base64`.
#[tauri::command]
pub fn stop_mic_recording(
    recorder_state: State<'_, MicRecorderState>,
) -> Result<String, String> {
    let mut guard = recorder_state.0.lock().map_err(|e| e.to_string())?;
    let mut handle = guard.take().ok_or("No recording in progress")?;

    // Signal the recording thread to stop
    let _ = handle.stop_tx.send(());
    if let Some(jh) = handle.join_handle.take() {
        let _ = jh.join().map_err(|_| "Recording thread panicked")?;
    }

    let sample_rate = *handle.sample_rate.lock().map_err(|e| e.to_string())?;
    let raw_samples = handle.samples.lock().map_err(|e| e.to_string())?.clone();

    if raw_samples.is_empty() {
        return Err("No audio was recorded".into());
    }

    // Resample to 16 kHz for Parakeet ASR
    let target_rate: u32 = 16000;
    let resampled = if sample_rate == target_rate {
        raw_samples
    } else {
        let ratio = sample_rate as f64 / target_rate as f64;
        let new_len = (raw_samples.len() as f64 / ratio).round() as usize;
        let mut out = Vec::with_capacity(new_len);
        for i in 0..new_len {
            let src_idx = i as f64 * ratio;
            let floor = src_idx.floor() as usize;
            let ceil = (floor + 1).min(raw_samples.len() - 1);
            let frac = src_idx - floor as f64;
            out.push(
                raw_samples[floor] * (1.0 - frac as f32)
                    + raw_samples[ceil] * frac as f32,
            );
        }
        out
    };

    // Encode as 16-bit PCM WAV
    let num_samples = resampled.len();
    let bytes_per_sample: u32 = 2;
    let data_len: u32 = (num_samples as u32) * bytes_per_sample;
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    // fmt
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&target_rate.to_le_bytes());
    wav.extend_from_slice(&(target_rate * bytes_per_sample).to_le_bytes());
    wav.extend_from_slice(&(bytes_per_sample as u16).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    // data
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for &sample in &resampled {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample: i16 = if clamped < 0.0 {
            (clamped * 32768.0) as i16
        } else {
            (clamped * 32767.0) as i16
        };
        wav.extend_from_slice(&int_sample.to_le_bytes());
    }

    let b64 = STANDARD.encode(&wav);
    log::info!(
        "Mic recording stopped: {} samples, {:.1}s",
        num_samples,
        num_samples as f64 / target_rate as f64
    );

    Ok(b64)
}
