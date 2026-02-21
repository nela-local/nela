//! Backend trait and implementations for model inference.
//!
//! Each backend knows how to start, health-check, execute, and stop a model.
//! The `ModelBackend` trait is the single abstraction for both child-process
//! and in-process models.

pub mod llama_server;
pub mod llama_cli;
pub mod whisper_cpp;
pub mod tts_inference;
pub mod onnx_classifier;

use crate::registry::types::{ModelDef, ModelHandle, TaskRequest, TaskResponse};
use async_trait::async_trait;
use std::path::Path;

/// Trait implemented by every model backend.
///
/// - **Child-process backends** (`LlamaServer`, `PythonExe`, `WhisperCpp`):
///   `start()` spawns a binary, `execute()` communicates via HTTP/CLI.
///   `start()` loads model into memory, `execute()` runs inference directly.
#[async_trait]
pub trait ModelBackend: Send + Sync + std::fmt::Debug {
    /// Start the model: spawn process or load into memory.
    /// `models_dir` is the absolute path to the models root directory.
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String>;

    /// Check if the model instance is alive and responsive.
    async fn is_healthy(&self, handle: &ModelHandle) -> bool;

    /// Execute a task on a running model instance.
    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        models_dir: &Path,
    ) -> Result<TaskResponse, String>;

    /// Gracefully shut down / unload the model instance.
    async fn stop(&self, handle: &ModelHandle) -> Result<(), String>;

    /// Estimated memory usage in MB (from config, not measured).
    fn estimated_memory_mb(&self, def: &ModelDef) -> u32 {
        def.memory_mb
    }
}

/// Create the appropriate backend for a given model definition.
pub fn create_backend(def: &ModelDef) -> Box<dyn ModelBackend> {
    use crate::registry::types::BackendKind;
    match &def.backend {
        BackendKind::LlamaServer => Box::new(llama_server::LlamaServerBackend::new()),
        BackendKind::LlamaCli => Box::new(llama_cli::LlamaCliBackend::new()),
        BackendKind::WhisperCpp => Box::new(whisper_cpp::WhisperCppBackend::new()),
        BackendKind::TtsInference => Box::new(tts_inference::TtsInferenceBackend::new()),
        BackendKind::OnnxClassifier => Box::new(onnx_classifier::OnnxClassifierBackend::new()),
    }
}
