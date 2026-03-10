//! Core type definitions for the GenHat process control module.
//!
//! These types are the shared vocabulary across config, registry, backends,
//! process manager, router, and commands.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Arc;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════════════════════════
// Enums
// ═══════════════════════════════════════════════════════════════════════════════

/// Which backend implementation runs this model.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendKind {
    LlamaServer,
    LlamaCli,
    Parakeet,
    OnnxClassifier,
    CrossEncoder,
    KittenTts,
}

/// Whether the model runs as a child process or in the Tauri process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelKind {
    ChildProcess,
    InProcess,
}

/// Every kind of task the system can handle.
/// New tasks can be added here; make sure to also update `config/mod.rs::parse_task`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskType {
    // User-facing
    Chat,
    VisionChat,
    Summarize,
    Mindmap,
    Tts,
    Transcribe,
    Stt,
    // Podcast
    PodcastScript,
    // RAG-internal
    Embed,
    Classify,
    Enrich,
    Grade,
    Hyde,
    // Extensibility
    Custom(String),
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::Chat => write!(f, "chat"),
            TaskType::VisionChat => write!(f, "vision_chat"),
            TaskType::Summarize => write!(f, "summarize"),
            TaskType::Mindmap => write!(f, "mindmap"),
            TaskType::Tts => write!(f, "tts"),
            TaskType::PodcastScript => write!(f, "podcast_script"),
            TaskType::Transcribe => write!(f, "transcribe"),
            TaskType::Stt => write!(f, "stt"),
            TaskType::Embed => write!(f, "embed"),
            TaskType::Classify => write!(f, "classify"),
            TaskType::Enrich => write!(f, "enrich"),
            TaskType::Grade => write!(f, "grade"),
            TaskType::Hyde => write!(f, "hyde"),
            TaskType::Custom(s) => write!(f, "{s}"),
        }
    }
}

/// Runtime status of a model instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Ready,
    Busy,
    Error(String),
    ShuttingDown,
}

/// Priority level for task requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    /// Background tasks (enrichment, grading) — can be preempted.
    Low = 0,
    /// User-facing tasks (chat, summarize) — served first.
    High = 1,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Model Definition (from config)
// ═══════════════════════════════════════════════════════════════════════════════

/// A model definition parsed from `models.toml`. Immutable after load.
#[derive(Debug, Clone)]
pub struct ModelDef {
    pub id: String,
    pub name: String,
    pub backend: BackendKind,
    pub kind: ModelKind,
    /// Path relative to the models directory.
    pub model_file: String,
    pub tasks: Vec<TaskType>,
    pub auto_start: bool,
    pub max_instances: u32,
    pub idle_timeout_s: u64,
    pub priority: u32,
    pub memory_mb: u32,
    /// Backend-specific parameters (all as strings, parsed by each backend).
    pub params: HashMap<String, String>,
    /// Optional per-task priority overrides. If a task is not listed here,
    /// the model's default `priority` field is used.
    pub task_priorities: HashMap<TaskType, u32>,
}

impl ModelDef {
    /// Get a param value, returning the default if missing.
    pub fn param_or(&self, key: &str, default: &str) -> String {
        self.params.get(key).cloned().unwrap_or_else(|| default.to_string())
    }

    /// Check if this model can handle a given task type.
    pub fn supports_task(&self, task: &TaskType) -> bool {
        self.tasks.contains(task)
    }

    /// Get the effective priority for a specific task.
    /// Uses the task-specific override if present, otherwise falls back
    /// to the model's default `priority`.
    pub fn priority_for_task(&self, task: &TaskType) -> u32 {
        self.task_priorities.get(task).copied().unwrap_or(self.priority)
    }

    /// Check whether all required model files exist under `models_dir`.
    ///
    /// Checks the primary `model_file` (as file **or** directory, since some
    /// models like KittenTTS and Parakeet use a directory) and every param
    /// whose key ends with `_file` (e.g. `mmproj_file`, `config_file`,
    /// `tokenizer_file`).
    ///
    /// Returns a list of missing paths (empty = all present).
    pub fn missing_files(&self, models_dir: &Path) -> Vec<String> {
        let mut missing = Vec::new();

        // Primary model file / directory
        let primary = models_dir.join(&self.model_file);
        if !primary.exists() {
            missing.push(self.model_file.clone());
        }

        // Companion files declared in params (convention: key ends with "_file")
        for (key, val) in &self.params {
            if key.ends_with("_file") {
                let p = models_dir.join(val);
                if !p.exists() {
                    missing.push(val.clone());
                }
            }
        }

        missing
    }

    /// Convenience: returns `true` when every required file is present.
    pub fn files_exist(&self, models_dir: &Path) -> bool {
        self.missing_files(models_dir).is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Model Handle (runtime)
// ═══════════════════════════════════════════════════════════════════════════════

/// Handle to a running child-process model instance.
#[derive(Debug)]
pub struct ProcessHandle {
    pub child: Child,
    pub pid: u32,
    /// Assigned port (for server-based backends like llama-server).
    pub port: Option<u16>,
    pub started_at: Instant,
    /// Working directory of the process.
    pub work_dir: PathBuf,
}

/// Handle to an in-process model instance (placeholder for candle models).
pub struct InMemoryHandle {
    /// The actual loaded model — `Arc<dyn Any + Send + Sync>`.
    pub model: Arc<dyn std::any::Any + Send + Sync>,
    pub loaded_at: Instant,
}

impl std::fmt::Debug for InMemoryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryHandle")
            .field("loaded_at", &self.loaded_at)
            .finish()
    }
}

/// Unified handle for both kinds of model.
#[derive(Debug)]
pub enum ModelHandle {
    Process(ProcessHandle),
    InMemory(InMemoryHandle),
}

// ═══════════════════════════════════════════════════════════════════════════════
// Managed Instance (held by ProcessManager)
// ═══════════════════════════════════════════════════════════════════════════════

/// A single running instance of a model, tracked by the process manager.
#[derive(Debug)]
pub struct ManagedInstance {
    pub instance_id: String,
    /// Handle to the running model. None when the instance is still loading.
    /// Wrapped in Arc so the handle can be shared without holding a lock
    /// across long-running backend calls.
    pub handle: Option<Arc<ModelHandle>>,
    pub status: ModelStatus,
    /// If true, this instance will be killed once its current task completes.
    pub ephemeral: bool,
    pub last_activity: Instant,
    /// Number of in-flight requests on this instance.
    pub active_requests: u32,
}

/// All instances + metadata for a single registered model.
pub struct ManagedModel {
    pub def: ModelDef,
    pub instances: Vec<ManagedInstance>,
}

impl std::fmt::Debug for ManagedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedModel")
            .field("id", &self.def.id)
            .field("instances", &self.instances.len())
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task Request / Response
// ═══════════════════════════════════════════════════════════════════════════════

/// A request to execute a task, sent to the router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    /// Unique request identifier.
    pub request_id: String,
    /// What kind of task to perform.
    pub task_type: TaskType,
    /// Primary input (text, prompt, file path, etc.).
    pub input: String,
    /// Optional: force a specific model instead of auto-routing.
    pub model_override: Option<String>,
    /// Optional: additional key-value parameters.
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// Response from a completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskResponse {
    /// Plain text output (chat, summarize, mindmap, enrich, grade).
    Text(String),
    /// Path to a generated file (TTS wav, etc.).
    FilePath(String),
    /// Embedding vectors.
    Embeddings(Vec<Vec<f32>>),
    /// Classification result.
    Classification {
        label: String,
        confidence: f32,
    },
    /// Relevance score (0-1) from cross-encoder.
    Score(f32),
    /// Transcription segments.
    Transcription {
        segments: Vec<TranscriptSegment>,
    },
    /// Error with message.
    Error(String),
}

/// A segment of transcribed audio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Model Info (for frontend display)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serializable model info sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub kind: String,
    pub model_file: String,
    pub tasks: Vec<String>,
    pub status: ModelStatus,
    pub instance_count: u32,
    pub memory_mb: u32,
    pub priority: u32,
}
