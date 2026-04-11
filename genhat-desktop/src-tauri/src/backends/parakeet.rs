//! ModelBackend implementation for Parakeet ASR — in-process ONNX speech recognition.
//!
//! Follows the same `InMemoryHandle` pattern as `kitten_tts.rs` and
//! `onnx_classifier.rs`: the ONNX session and preprocessing tables are
//! loaded into memory in `start()`, and `execute()` runs inference
//! directly without spawning any external process.
//!
//! Replaces the previous `whisper_cpp.rs` backend — no external binary
//! or Python sidecar needed.

use crate::asr::inference::ParakeetEngine;
use crate::registry::types::{
    InMemoryHandle, ModelDef, ModelHandle, TaskRequest, TaskResponse, TranscriptSegment,
};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────────────────
// Backend
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ParakeetBackend;

impl ParakeetBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl super::ModelBackend for ParakeetBackend {
    /// Load the Parakeet ONNX model, vocabulary, and pre-compute DSP tables.
    ///
    /// Runs on a blocking thread to avoid stalling the tokio async runtime
    /// while loading ~700 MB of ONNX sessions.
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        // model_file points to the model directory (e.g. "asr/parakeet")
        let mut model_dir = models_dir.join(&def.model_file);
        if !model_dir.exists() && def.model_file == "asr/parakeet" {
            let legacy = models_dir.join("parakeet");
            if legacy.exists() {
                model_dir = legacy;
            }
        }
        let overrides = def.params.clone();

        if !model_dir.exists() {
            return Err(format!(
                "Parakeet model directory not found: {}",
                model_dir.display()
            ));
        }

        log::info!("[Parakeet] Starting from {}", model_dir.display());

        // Heavy synchronous I/O — offload to a blocking thread so the async
        // runtime stays responsive and the frontend doesn't hang.
        let handle = tokio::task::spawn_blocking(move || {
            let engine = ParakeetEngine::load_with_overrides(&model_dir, &overrides)?;
            log::info!("[Parakeet] Engine loaded and ready");
            Ok::<ModelHandle, String>(ModelHandle::InMemory(InMemoryHandle {
                model: Arc::new(engine),
                loaded_at: Instant::now(),
            }))
        })
        .await
        .map_err(|e| format!("Parakeet model loading thread panicked: {e}"))??;

        Ok(handle)
    }

    async fn is_healthy(&self, handle: &ModelHandle) -> bool {
        matches!(handle, ModelHandle::InMemory(_))
    }

    /// Transcribe an audio file.
    ///
    /// `request.input` must be the absolute path to an audio file.
    /// Returns `TaskResponse::Transcription` with a single segment
    /// containing the full transcription text.
    ///
    /// Runs ONNX inference on a blocking thread so the async runtime
    /// stays responsive.
    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let mem = match handle {
            ModelHandle::InMemory(h) => h,
            _ => return Err("Parakeet requires InMemoryHandle".into()),
        };

        // Clone the Arc so we can move it into the blocking thread.
        let model_arc = mem.model.clone();
        let audio_input = request.input.clone();

        let result = tokio::task::spawn_blocking(move || {
            let engine = model_arc
                .downcast_ref::<ParakeetEngine>()
                .ok_or("Failed to downcast to ParakeetEngine")?;

            let audio_path = std::path::Path::new(&audio_input);
            if !audio_path.exists() {
                return Err(format!("Audio file not found: {}", audio_path.display()));
            }

            let start = Instant::now();
            let text = engine.transcribe(audio_path)?;
            let elapsed = start.elapsed();

            log::info!(
                "[Parakeet] Transcribed {} in {:.2}s",
                audio_path.display(),
                elapsed.as_secs_f64(),
            );

            // Return as a single-segment transcription (no sub-segment timestamps
            // from greedy TDT decode — the text covers the entire file).
            let segments = if text.is_empty() {
                Vec::new()
            } else {
                vec![TranscriptSegment {
                    text,
                    start_ms: 0,
                    end_ms: 0,
                }]
            };

            Ok::<TaskResponse, String>(TaskResponse::Transcription { segments })
        })
        .await
        .map_err(|e| format!("Parakeet transcription thread panicked: {e}"))??;

        Ok(result)
    }

    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        log::info!("[Parakeet] Stopped (memory freed on drop)");
        Ok(())
    }

    fn estimated_memory_mb(&self, def: &ModelDef) -> u32 {
        if def.memory_mb > 0 {
            def.memory_mb
        } else {
            700 // Parakeet TDT 0.6B INT8: encoder ~622MB + decoder ~12MB + joiner ~6MB
        }
    }
}
