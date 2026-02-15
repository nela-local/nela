//! Whisper.cpp backend (stub).
//!
//! Will spawn `whisper-cli` (or similar whisper.cpp binary) as a child process
//! to transcribe audio files. Currently a placeholder — returns an error
//! until the whisper binary and model are available.

use crate::backends::ModelBackend;
use crate::registry::types::{ModelDef, ModelHandle, TaskRequest, TaskResponse};
use async_trait::async_trait;
use std::path::Path;

#[derive(Debug)]
pub struct WhisperCppBackend;

impl WhisperCppBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModelBackend for WhisperCppBackend {
    async fn start(&self, _def: &ModelDef, _models_dir: &Path) -> Result<ModelHandle, String> {
        Err("WhisperCpp backend not yet implemented. Add whisper binary to bin/ and model to models/".into())
    }

    async fn is_healthy(&self, _handle: &ModelHandle) -> bool {
        false
    }

    async fn execute(
        &self,
        _handle: &ModelHandle,
        _request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        Err("WhisperCpp backend not yet implemented".into())
    }

    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        Ok(())
    }
}
