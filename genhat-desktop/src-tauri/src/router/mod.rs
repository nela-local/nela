//! Task router: maps incoming TaskRequests to the correct model and backend.
//!
//! The router is the single entry point for all inference requests. It:
//! 1. Determines which model should handle a task (by task type + priority)
//! 2. Ensures the model is running (lazy spawn)
//! 3. Delegates execution to the process manager

pub mod tasks;

use crate::process::pool;
use crate::process::ProcessManager;
use crate::registry::types::{TaskRequest, TaskResponse, TaskType};
use crate::registry::types::ModelDef;
use crate::registry::ModelRegistry;
use std::sync::Arc;

/// The task router — stateless, uses registry for lookups and process manager for execution.
pub struct TaskRouter {
    registry: Arc<ModelRegistry>,
    pub process_manager: Arc<ProcessManager>,
}

impl std::fmt::Debug for TaskRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskRouter").finish()
    }
}

impl TaskRouter {
    pub fn new(registry: Arc<ModelRegistry>, process_manager: Arc<ProcessManager>) -> Self {
        Self {
            registry,
            process_manager,
        }
    }

    /// Route a task request to the appropriate model and execute it.
    ///
    /// This is the main entry point for all inference from the frontend and
    /// from internal RAG pipeline calls.
    pub fn route<'a>(&'a self, request: &'a TaskRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResponse, String>> + Send + 'a>> {
        Box::pin(async move {
        // Determine candidate models (ranked) and try them in order.
        let candidates = self.resolve_model_candidates(request).await?;
        let is_ephemeral = pool::is_ephemeral_task(&request.task_type);

        let mut errors = Vec::new();
        for model_id in candidates {
            log::info!(
                "Routing task '{}' (req={}) → model '{}' (ephemeral={is_ephemeral})",
                request.task_type,
                &request.request_id[..8.min(request.request_id.len())],
                model_id
            );

            match self
                .process_manager
                .ensure_running(&model_id, is_ephemeral)
                .await
            {
                Ok(instance_id) => {
                    match self
                        .process_manager
                        .execute(&model_id, &instance_id, request)
                        .await
                    {
                        Ok(resp) => return Ok(resp),
                        Err(e) => {
                            log::warn!(
                                "Task '{}' failed on model '{}': {}. Trying fallback...",
                                request.task_type,
                                model_id,
                                e
                            );
                            errors.push(format!("{model_id}: {e}"));
                        }
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Task '{}' could not start model '{}': {}. Trying fallback...",
                        request.task_type,
                        model_id,
                        e
                    );
                    errors.push(format!("{model_id}: {e}"));
                }
            }
        }

        Err(format!(
            "All candidate models failed for task '{}': {}",
            request.task_type,
            errors.join(" | ")
        ))
        }) // end Box::pin
    }

    /// Resolve which model should handle a request.
    /// Checks the static registry first, then falls back to dynamically
    /// registered models in the ProcessManager.
    async fn resolve_model_candidates(&self, request: &TaskRequest) -> Result<Vec<String>, String> {
        // If the user specified a model override, use it
        if let Some(ref override_id) = request.model_override {
            // Check static registry first
            if self.registry.get(override_id).is_some() {
                return Ok(vec![override_id.clone()]);
            }
            // Check dynamic models in ProcessManager
            if self.process_manager.model_status(override_id).await.is_some() {
                return Ok(vec![override_id.clone()]);
            }
            return Err(format!("Model override '{override_id}' not found"));
        }

        // Gather candidates by task priority from the static registry first,
        // then append any dynamic runtime-only candidates.
        let candidates = self.registry.find_for_task(&request.task_type);
        let mut resolved = Vec::new();
        for candidate in &candidates {
            // model_status returns None for models not in ProcessManager
            if self.process_manager.model_status(&candidate.id).await.is_some() {
                resolved.push(candidate.id.clone());
            }
        }

        // Include all runtime candidates and deduplicate while preserving order.
        for dynamic_id in self.process_manager.find_models_for_task(&request.task_type).await {
            if !resolved.contains(&dynamic_id) {
                resolved.push(dynamic_id);
            }
        }

        if resolved.is_empty() {
            Err(format!(
                "No model registered for task type '{}'",
                request.task_type
            ))
        } else {
            Ok(resolved)
        }
    }

    /// Get the port of the llama-server instance for a given model
    /// (used by frontend for direct SSE streaming).
    pub async fn get_llama_port(&self, model_id: &str) -> Option<u16> {
        self.process_manager.get_llama_port(model_id).await
    }

    /// Look up the ModelDef for a given task type.
    /// Checks the static registry first, then falls back to dynamically
    /// registered models in the ProcessManager.
    pub async fn get_model_def_for_task(&self, task: &TaskType) -> Option<ModelDef> {
        // Try static registry — only pick models actually registered in ProcessManager
        let candidates = self.registry.find_for_task(task);
        for def in &candidates {
            if self.process_manager.model_status(&def.id).await.is_some() {
                return Some((*def).clone());
            }
        }
        // Fall back to dynamic models
        let model_id = self.process_manager.find_model_for_task(task).await?;
        self.process_manager.get_model_def(&model_id).await
    }

    /// Look up a ModelDef by its id.
    /// Checks the static registry first, then falls back to dynamically
    /// registered models in the ProcessManager.
    pub async fn get_model_def_by_id(&self, id: &str) -> Option<ModelDef> {
        // Try static registry first
        if let Some(def) = self.registry.get(id) {
            return Some(def.clone());
        }
        // Fall back to dynamic models
        self.process_manager.get_model_def(id).await
    }
}
