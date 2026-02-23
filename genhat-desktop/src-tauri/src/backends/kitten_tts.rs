//! ModelBackend implementation for KittenTTS — in-process ONNX TTS.
//!
//! Follows the same `InMemoryHandle` pattern as `onnx_classifier.rs`:
//! the ONNX session and all data are loaded into memory in `start()`,
//! and `execute()` runs inference directly without spawning any process.
//!
//! The engine only needs `espeak-ng` installed on the system for
//! phonemization (a tiny CLI call, not a Python dependency).

use crate::registry::types::{
    InMemoryHandle, ModelDef, ModelHandle, TaskRequest, TaskResponse,
};
use crate::tts::inference::KittenTtsEngine;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────────────────
// Backend
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct KittenTtsBackend;

impl KittenTtsBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl super::ModelBackend for KittenTtsBackend {
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        // model_file points to the model directory (e.g. "kittenTTS/mini")
        let model_dir = models_dir.join(&def.model_file);

        if !model_dir.exists() {
            return Err(format!(
                "KittenTTS model directory not found: {}",
                model_dir.display()
            ));
        }

        log::info!("[KittenTTS] Starting from {}", model_dir.display());

        let engine = KittenTtsEngine::load(&model_dir)?;

        log::info!(
            "[KittenTTS] Engine loaded — voices: {:?}",
            engine.voice_names()
        );

        Ok(ModelHandle::InMemory(InMemoryHandle {
            model: Arc::new(engine),
            loaded_at: Instant::now(),
        }))
    }

    async fn is_healthy(&self, handle: &ModelHandle) -> bool {
        matches!(handle, ModelHandle::InMemory(_))
    }

    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let mem = match handle {
            ModelHandle::InMemory(h) => h,
            _ => return Err("KittenTTS requires InMemoryHandle".into()),
        };

        let engine = mem
            .model
            .downcast_ref::<KittenTtsEngine>()
            .ok_or("Failed to downcast to KittenTtsEngine")?;

        let voice = request
            .extra
            .get("voice")
            .map(|s| s.as_str())
            .unwrap_or("Leo");

        let speed: f32 = request
            .extra
            .get("speed")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0);

        // Generate to a temp WAV file
        let output_dir = std::env::temp_dir().join("genhat-tts");
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("Failed to create TTS output dir: {e}"))?;

        let filename = format!("{}.wav", uuid::Uuid::new_v4());
        let output_path = output_dir.join(&filename);

        let path_str = engine.generate_to_file(
            &request.input,
            voice,
            speed,
            &output_path,
        )?;

        Ok(TaskResponse::FilePath(path_str))
    }

    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        log::info!("[KittenTTS] Stopped (memory freed on drop)");
        Ok(())
    }

    fn estimated_memory_mb(&self, def: &ModelDef) -> u32 {
        if def.memory_mb > 0 {
            def.memory_mb
        } else {
            200 // KittenTTS mini is ~150 MB ONNX + voice embeddings
        }
    }
}
