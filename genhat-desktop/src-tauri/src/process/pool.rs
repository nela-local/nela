//! Instance pool management — concurrency limits, priority queuing, yield-on-demand.
//!
//! This module extends the ProcessManager with per-model pool logic.
//! Currently the pool logic is inlined in ProcessManager::ensure_running().
//! This file provides:
//! - Priority classification (High for user tasks, Low for background)
//! - Yield-on-demand constants used by the router when a user is active

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

/// Returns `true` if the task is high-priority (user-facing).
pub fn is_high_priority(task: &TaskType) -> bool {
    task_priority(task) == TaskPriority::High
}

/// Number of times a low-priority request will retry waiting for a free slot
/// before proceeding anyway. Each retry sleeps `LOW_PRIORITY_YIELD_MS` ms.
pub const LOW_PRIORITY_YIELD_RETRIES: u32 = 3;

/// How long (ms) each yield iteration sleeps when a user is active.
pub const LOW_PRIORITY_YIELD_MS: u64 = 500;

/// Whether a task type is ephemeral (instance should be killed after completion).
pub fn is_ephemeral_task(task: &TaskType) -> bool {
    matches!(
        task,
        TaskType::Enrich | TaskType::Grade | TaskType::Hyde
    )
}
