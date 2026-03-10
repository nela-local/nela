//! Instance pool management — concurrency limits, priority queuing.
//!
//! This module extends the ProcessManager with per-model pool logic.
//! Currently the pool logic is inlined in ProcessManager::ensure_running().
//! This file is reserved for future advanced pool features:
//! - Priority request queuing (High for user tasks, Low for background)
//! - Work-stealing between models
//! - Weighted fair scheduling

use crate::registry::types::{TaskPriority, TaskType};

/// Determine the priority of a task type.
pub fn task_priority(task: &TaskType) -> TaskPriority {
    match task {
        // User-facing = high priority
        TaskType::Chat
        | TaskType::VisionChat
        | TaskType::Summarize
        | TaskType::Mindmap
        | TaskType::Tts
        | TaskType::PodcastScript
        | TaskType::Transcribe
        | TaskType::Stt => TaskPriority::High,

        // Background / RAG-internal = low priority
        TaskType::Embed
        | TaskType::Classify
        | TaskType::Enrich
        | TaskType::Grade
        | TaskType::Hyde => TaskPriority::Low,

        // Custom tasks default to high (assume user-facing)
        TaskType::Custom(_) => TaskPriority::High,
    }
}

/// Whether a task type is ephemeral (instance should be killed after completion).
pub fn is_ephemeral_task(task: &TaskType) -> bool {
    matches!(
        task,
        TaskType::Enrich | TaskType::Grade | TaskType::Hyde | TaskType::Embed
    )
}
