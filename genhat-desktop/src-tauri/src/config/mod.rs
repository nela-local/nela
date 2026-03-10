//! Configuration loading for the GenHat process control module.
//!
//! The model registry is defined in `models.toml` (embedded at compile time).
//! Adding a new model requires only a new `[[models]]` entry in that file.

use crate::registry::types::{BackendKind, ModelDef, ModelKind, TaskType};
use serde::Deserialize;
use std::collections::HashMap;

/// Raw TOML file structure — deserialized then converted to domain types.
#[derive(Debug, Deserialize)]
struct RawConfig {
    models: Vec<RawModelDef>,
}

#[derive(Debug, Deserialize)]
struct RawModelDef {
    id: String,
    name: String,
    backend: String,
    kind: String,
    model_file: String,
    tasks: Vec<String>,
    #[serde(default)]
    auto_start: bool,
    #[serde(default = "default_max_instances")]
    max_instances: u32,
    #[serde(default)]
    idle_timeout_s: u64,
    #[serde(default = "default_priority")]
    priority: u32,
    #[serde(default)]
    memory_mb: u32,
    #[serde(default)]
    params: HashMap<String, String>,
    #[serde(default)]
    task_priorities: HashMap<String, u32>,
}

fn default_max_instances() -> u32 {
    1
}
fn default_priority() -> u32 {
    10
}

/// Embed models.toml at compile time so it's always available.
const MODELS_TOML: &str = include_str!("models.toml");

/// Parse the embedded config and return a list of model definitions.
pub fn load_model_definitions() -> Result<Vec<ModelDef>, String> {
    let raw: RawConfig =
        toml::from_str(MODELS_TOML).map_err(|e| format!("Failed to parse models.toml: {e}"))?;

    raw.models
        .into_iter()
        .map(|r| {
            let backend = parse_backend(&r.backend)?;
            let kind = parse_kind(&r.kind)?;
            let tasks: Result<Vec<TaskType>, String> =
                r.tasks.iter().map(|t| parse_task(t)).collect();

            let task_priorities: HashMap<TaskType, u32> = r
                .task_priorities
                .iter()
                .filter_map(|(k, v)| parse_task(k).ok().map(|t| (t, *v)))
                .collect();

            Ok(ModelDef {
                id: r.id,
                name: r.name,
                backend,
                kind,
                model_file: r.model_file,
                tasks: tasks?,
                auto_start: r.auto_start,
                max_instances: r.max_instances,
                idle_timeout_s: r.idle_timeout_s,
                priority: r.priority,
                memory_mb: r.memory_mb,
                params: r.params,
                task_priorities,
            })
        })
        .collect()
}

fn parse_backend(s: &str) -> Result<BackendKind, String> {
    match s {
        "llama_server" => Ok(BackendKind::LlamaServer),
        "llama_cli" => Ok(BackendKind::LlamaCli),
        "parakeet" => Ok(BackendKind::Parakeet),
        "onnx_classifier" => Ok(BackendKind::OnnxClassifier),
        "cross_encoder" => Ok(BackendKind::CrossEncoder),
        "kitten_tts" => Ok(BackendKind::KittenTts),
        other => Err(format!("Unknown backend: {other}")),
    }
}

fn parse_kind(s: &str) -> Result<ModelKind, String> {
    match s {
        "child_process" => Ok(ModelKind::ChildProcess),
        "in_process" => Ok(ModelKind::InProcess),
        other => Err(format!("Unknown kind: {other}")),
    }
}

fn parse_task(s: &str) -> Result<TaskType, String> {
    match s {
        "chat" => Ok(TaskType::Chat),
        "vision_chat" => Ok(TaskType::VisionChat),
        "summarize" => Ok(TaskType::Summarize),
        "mindmap" => Ok(TaskType::Mindmap),
        "tts" => Ok(TaskType::Tts),
        "podcast_script" => Ok(TaskType::PodcastScript),
        "transcribe" => Ok(TaskType::Transcribe),
        "stt" => Ok(TaskType::Stt),
        "embed" => Ok(TaskType::Embed),
        "classify" => Ok(TaskType::Classify),
        "enrich" => Ok(TaskType::Enrich),
        "grade" => Ok(TaskType::Grade),
        "hyde" => Ok(TaskType::Hyde),
        other => Ok(TaskType::Custom(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_embedded_config() {
        let defs = load_model_definitions().expect("Should parse models.toml");
        assert!(!defs.is_empty(), "Should have at least one model defined");

        let lfm = defs.iter().find(|d| d.id == "lfm-1_2b");
        assert!(lfm.is_some(), "Should find lfm-1_2b model");

        let lfm = lfm.unwrap();
        assert_eq!(lfm.backend, BackendKind::LlamaServer);
        assert!(lfm.tasks.contains(&TaskType::Chat));
        assert_eq!(lfm.max_instances, 2);
    }
}
