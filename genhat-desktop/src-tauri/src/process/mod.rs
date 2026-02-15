//! Process Manager: central orchestrator for all model instances.
//!
//! Lazily spawns model instances, tracks their lifecycle, enforces concurrency
//! limits, reaps idle ephemeral instances, and handles graceful shutdown.

pub mod pool;
pub mod lifecycle;

use crate::backends::{self, ModelBackend};
use crate::registry::types::{
    ManagedInstance, ManagedModel, ModelHandle, ModelInfo, ModelStatus, TaskRequest,
    TaskResponse,
};
use crate::registry::ModelRegistry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Central process manager — holds all model instances and backends.
pub struct ProcessManager {
    /// model_id → managed model (instances + backend)
    models: Arc<RwLock<HashMap<String, ManagedModel>>>,
    /// Prebuilt backends keyed by model_id
    backends: HashMap<String, Arc<dyn ModelBackend>>,
    /// Absolute path to the models directory
    models_dir: PathBuf,
    /// Global memory budget in MB (0 = unlimited)
    memory_budget_mb: u32,
}

impl std::fmt::Debug for ProcessManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessManager")
            .field("models_dir", &self.models_dir)
            .field("memory_budget_mb", &self.memory_budget_mb)
            .finish()
    }
}

impl ProcessManager {
    /// Create a new ProcessManager from a registry and models directory.
    pub fn new(registry: &ModelRegistry, models_dir: PathBuf) -> Self {
        let mut managed = HashMap::new();
        let mut backend_map = HashMap::new();

        for def in registry.all() {
            let backend: Arc<dyn ModelBackend> = Arc::from(backends::create_backend(def));
            backend_map.insert(def.id.clone(), backend);
            managed.insert(
                def.id.clone(),
                ManagedModel {
                    def: def.clone(),
                    instances: Vec::new(),
                },
            );
        }

        Self {
            models: Arc::new(RwLock::new(managed)),
            backends: backend_map,
            models_dir,
            memory_budget_mb: 0, // 0 = unlimited, set via config later
        }
    }

    /// Set the global memory budget (MB). 0 = unlimited.
    pub fn set_memory_budget(&mut self, mb: u32) {
        self.memory_budget_mb = mb;
    }

    /// Get the models directory path.
    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    /// Ensure at least one instance of a model is running and available.
    /// If all existing instances are busy and `max_instances` allows, spawn a new one.
    /// The `ephemeral` flag marks the instance for auto-reap after its task completes.
    ///
    /// Returns the instance_id of the available instance.
    pub async fn ensure_running(
        &self,
        model_id: &str,
        ephemeral: bool,
    ) -> Result<String, String> {
        let backend = self
            .backends
            .get(model_id)
            .ok_or_else(|| format!("No backend registered for model '{model_id}'"))?
            .clone();

        let mut models = self.models.write().await;
        
        // Pre-check: extract needed info and compute memory usage before mutable borrow
        let (needed_mb, max_instances) = {
            let managed = models
                .get(model_id)
                .ok_or_else(|| format!("Model '{model_id}' not found in registry"))?;
            (managed.def.memory_mb, managed.def.max_instances)
        };

        // Memory budget check (before mutating)
        if self.memory_budget_mb > 0 {
            let current_usage = self.current_memory_usage_internal(&models);
            if current_usage + needed_mb > self.memory_budget_mb {
                return Err(format!(
                    "Memory budget exceeded: current={current_usage}MB, needed={needed_mb}MB, budget={}MB",
                    self.memory_budget_mb
                ));
            }
        }

        // Now get mutable reference
        let managed = models
            .get_mut(model_id)
            .ok_or_else(|| format!("Model '{model_id}' not found in registry"))?;

        // 1. Check for an existing Ready instance that is not busy
        for inst in &managed.instances {
            if inst.status == ModelStatus::Ready && inst.active_requests == 0 {
                log::debug!(
                    "Reusing existing instance '{}' for model '{model_id}'",
                    inst.instance_id
                );
                return Ok(inst.instance_id.clone());
            }
        }

        // 2. Check concurrency limit
        let active_count = managed
            .instances
            .iter()
            .filter(|i| i.status != ModelStatus::ShuttingDown)
            .count() as u32;

        if active_count >= max_instances {
            // Try to find an instance that is Ready (even if busy) — queue behind it
            for inst in &managed.instances {
                if inst.status == ModelStatus::Ready {
                    log::debug!(
                        "All instances busy for '{model_id}', queuing on '{}'",
                        inst.instance_id
                    );
                    return Ok(inst.instance_id.clone());
                }
            }
            return Err(format!(
                "Model '{model_id}' at max instances ({max_instances}) and all are busy/loading",
            ));
        }

        // 3. Spawn a new instance
        let instance_id = uuid::Uuid::new_v4().to_string();
        log::info!(
            "Spawning new instance '{}' for model '{model_id}' (ephemeral={ephemeral})",
            &instance_id[..8]
        );

        managed.instances.push(ManagedInstance {
            instance_id: instance_id.clone(),
            handle: None,
            status: ModelStatus::Loading,
            ephemeral,
            last_activity: std::time::Instant::now(),
            active_requests: 0,
        });

        let def = managed.def.clone();
        let models_dir = self.models_dir.clone();

        // Drop the write lock before the potentially slow start() call
        drop(models);

        // Actually start the model
        match backend.start(&def, &models_dir).await {
            Ok(handle) => {
                let mut models = self.models.write().await;
                if let Some(managed) = models.get_mut(model_id) {
                    if let Some(inst) = managed
                        .instances
                        .iter_mut()
                        .find(|i| i.instance_id == instance_id)
                    {
                        inst.handle = Some(handle);
                        inst.status = ModelStatus::Ready;
                        inst.last_activity = std::time::Instant::now();
                        log::info!(
                            "Instance '{}' for model '{model_id}' is ready",
                            &instance_id[..8]
                        );
                    }
                }
                Ok(instance_id)
            }
            Err(e) => {
                // Remove the failed instance
                let mut models = self.models.write().await;
                if let Some(managed) = models.get_mut(model_id) {
                    managed
                        .instances
                        .retain(|i| i.instance_id != instance_id);
                }
                Err(format!("Failed to start model '{model_id}': {e}"))
            }
        }
    }

    /// Execute a task on a specific model instance.
    pub async fn execute(
        &self,
        model_id: &str,
        instance_id: &str,
        request: &TaskRequest,
    ) -> Result<TaskResponse, String> {
        let backend = self
            .backends
            .get(model_id)
            .ok_or_else(|| format!("No backend for '{model_id}'"))?
            .clone();

        // Inject model_file into the request extra params for backends that need it
        let mut enriched_request = request.clone();
        {
            let models = self.models.read().await;
            if let Some(managed) = models.get(model_id) {
                enriched_request.extra.insert("model_file".to_string(), managed.def.model_file.clone());
                // Also inject any model params into extra
                for (k, v) in &managed.def.params {
                    if !enriched_request.extra.contains_key(k) {
                        enriched_request.extra.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        // Mark instance as busy
        {
            let mut models = self.models.write().await;
            if let Some(managed) = models.get_mut(model_id) {
                if let Some(inst) = managed.instances.iter_mut().find(|i| i.instance_id == *instance_id) {
                    inst.active_requests += 1;
                    inst.status = ModelStatus::Busy;
                }
            }
        }


        // Execute on the backend.
        // We hold the read lock only during the execute() call.
        let result = {
            let models = self.models.read().await;
            let managed = models.get(model_id).ok_or("Model not found")?;
            let inst = managed
                .instances
                .iter()
                .find(|i| i.instance_id == *instance_id)
                .ok_or("Instance not found")?;

            let handle = inst.handle.as_ref().ok_or("Instance handle not ready")?;

            backend
                .execute(handle, &enriched_request, &self.models_dir)
                .await
        };

        // Mark instance as ready again, update last_activity
        {
            let mut models = self.models.write().await;
            if let Some(managed) = models.get_mut(model_id) {
                if let Some(inst) = managed.instances.iter_mut().find(|i| i.instance_id == *instance_id) {
                    inst.active_requests = inst.active_requests.saturating_sub(1);
                    if inst.active_requests == 0 {
                        inst.status = ModelStatus::Ready;
                    }
                    inst.last_activity = std::time::Instant::now();
                }
            }
        }

        result
    }

    /// Stop a specific model (all instances).
    pub async fn stop_model(&self, model_id: &str) -> Result<(), String> {
        let backend = self.backends.get(model_id).cloned();
        let mut models = self.models.write().await;

        if let Some(managed) = models.get_mut(model_id) {
            for inst in managed.instances.drain(..) {
                inst_stop(&backend, inst).await;
            }
            log::info!("All instances of '{model_id}' stopped");
        }
        Ok(())
    }

    /// Stop all models — called on app exit.
    pub async fn stop_all(&self) {
        log::info!("ProcessManager: stopping all models...");
        let mut models = self.models.write().await;
        for (model_id, managed) in models.iter_mut() {
            let backend = self.backends.get(model_id).cloned();
            for inst in managed.instances.drain(..) {
                inst_stop(&backend, inst).await;
            }
        }
        log::info!("ProcessManager: all models stopped");
    }

    /// Get info about all models (for frontend display).
    pub async fn list_models(&self) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        models
            .values()
            .map(|m| {
                let status = if m.instances.is_empty() {
                    ModelStatus::Unloaded
                } else if m.instances.iter().any(|i| i.status == ModelStatus::Ready) {
                    ModelStatus::Ready
                } else if m.instances.iter().any(|i| i.status == ModelStatus::Loading) {
                    ModelStatus::Loading
                } else if m.instances.iter().any(|i| i.status == ModelStatus::Busy) {
                    ModelStatus::Busy
                } else {
                    ModelStatus::Unloaded
                };

                ModelInfo {
                    id: m.def.id.clone(),
                    name: m.def.name.clone(),
                    backend: format!("{:?}", m.def.backend),
                    kind: format!("{:?}", m.def.kind),
                    model_file: m.def.model_file.clone(),
                    tasks: m.def.tasks.iter().map(|t| t.to_string()).collect(),
                    status,
                    instance_count: m.instances.len() as u32,
                    memory_mb: m.def.memory_mb,
                }
            })
            .collect()
    }

    /// Get the status of a specific model.
    pub async fn model_status(&self, model_id: &str) -> Option<ModelStatus> {
        let models = self.models.read().await;
        models.get(model_id).map(|m| {
            if m.instances.is_empty() {
                ModelStatus::Unloaded
            } else if m.instances.iter().any(|i| i.status == ModelStatus::Ready) {
                ModelStatus::Ready
            } else {
                ModelStatus::Loading
            }
        })
    }

    /// Get the port of a running llama-server instance (for frontend streaming).
    pub async fn get_llama_port(&self, model_id: &str) -> Option<u16> {
        let models = self.models.read().await;
        models.get(model_id).and_then(|m| {
            m.instances.iter().find_map(|inst| {
                if let Some(ModelHandle::Process(ph)) = &inst.handle {
                    ph.port
                } else {
                    None
                }
            })
        })
    }

    /// Get the port of a specific instance by model_id and instance_id.
    pub async fn get_instance_port(&self, model_id: &str, instance_id: &str) -> Option<u16> {
        let models = self.models.read().await;
        models.get(model_id).and_then(|m| {
            m.instances.iter().find_map(|inst| {
                if inst.instance_id == instance_id {
                    if let Some(ModelHandle::Process(ph)) = &inst.handle {
                        return ph.port;
                    }
                }
                None
            })
        })
    }

    /// Current total memory usage of all loaded models, in MB.
    pub async fn memory_usage(&self) -> u32 {
        let models = self.models.read().await;
        self.current_memory_usage_internal(&models)
    }

    fn current_memory_usage_internal(&self, models: &HashMap<String, ManagedModel>) -> u32 {
        models
            .values()
            .map(|m| {
                let instance_count = m
                    .instances
                    .iter()
                    .filter(|i| i.status != ModelStatus::ShuttingDown)
                    .count() as u32;
                m.def.memory_mb * instance_count
            })
            .sum()
    }

    /// Reap idle ephemeral instances that have exceeded their idle timeout.
    pub async fn reap_idle(&self) {
        let mut models = self.models.write().await;
        for (model_id, managed) in models.iter_mut() {
            let timeout_s = managed.def.idle_timeout_s;
            if timeout_s == 0 {
                continue; // 0 means fire-and-forget backends handle their own cleanup
            }

            let backend = self.backends.get(model_id).cloned();
            let mut to_remove = Vec::new();

            for (idx, inst) in managed.instances.iter().enumerate() {
                if inst.ephemeral
                    && inst.active_requests == 0
                    && inst.last_activity.elapsed().as_secs() > timeout_s
                {
                    log::info!(
                        "Reaping idle ephemeral instance '{}' of '{model_id}'",
                        &inst.instance_id[..8]
                    );
                    to_remove.push(idx);
                }
            }

            // Remove in reverse order to preserve indices
            for idx in to_remove.into_iter().rev() {
                let inst = managed.instances.remove(idx);
                inst_stop(&backend, inst).await;
            }
        }
    }
}

/// Helper: stop a single instance via its backend.
async fn inst_stop(backend: &Option<Arc<dyn ModelBackend>>, inst: ManagedInstance) {
    if let Some(b) = backend {
        if let Some(handle) = &inst.handle {
            if let Err(e) = b.stop(handle).await {
                log::warn!("Error stopping instance '{}': {e}", &inst.instance_id[..8]);
            }
        } else {
            log::debug!(
                "Instance '{}' has no handle yet, skipping stop call",
                &inst.instance_id[..8]
            );
        }
    }
}
