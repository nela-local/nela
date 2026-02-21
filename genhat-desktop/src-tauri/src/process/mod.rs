//! Process Manager: central orchestrator for all model instances.
//!
//! Lazily spawns model instances, tracks their lifecycle, enforces concurrency
//! limits, reaps idle ephemeral instances, and handles graceful shutdown.

pub mod pool;
pub mod lifecycle;

use crate::backends::{self, ModelBackend};
use crate::registry::types::{
    ManagedInstance, ManagedModel, ModelDef, ModelHandle, ModelInfo,
    ModelStatus, TaskRequest, TaskResponse, TaskType,
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
    /// Backends keyed by model_id (wrapped for runtime registration)
    backends: Arc<RwLock<HashMap<String, Arc<dyn ModelBackend>>>>,
    /// Absolute path to the models directory
    models_dir: PathBuf,
    /// Global memory budget in MB (0 = unlimited)
    memory_budget_mb: u32,
    /// Currently-active LLM model id (for legacy commands)
    active_llm_id: Arc<RwLock<String>>,
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
            backends: Arc::new(RwLock::new(backend_map)),
            models_dir,
            memory_budget_mb: 0, // 0 = unlimited, set via config later
            active_llm_id: Arc::new(RwLock::new("lfm-1_2b".to_string())),
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

    /// Get the currently-active LLM model id.
    pub async fn active_llm_id(&self) -> String {
        self.active_llm_id.read().await.clone()
    }

    /// Set the currently-active LLM model id.
    pub async fn set_active_llm(&self, id: &str) {
        *self.active_llm_id.write().await = id.to_string();
    }

    /// Dynamically register a new model at runtime.
    /// If a model with the same ID already exists, it will be stopped and replaced.
    pub async fn register_model(&self, def: ModelDef) -> Result<(), String> {
        let model_id = def.id.clone();

        // Stop existing model if present
        if self.backends.read().await.contains_key(&model_id) {
            let _ = self.stop_model(&model_id).await;
        }

        // Create backend
        let backend: Arc<dyn ModelBackend> = Arc::from(backends::create_backend(&def));

        // Insert into backends map
        self.backends.write().await.insert(model_id.clone(), backend);

        // Insert into models map
        self.models.write().await.insert(
            model_id.clone(),
            ManagedModel {
                def,
                instances: Vec::new(),
            },
        );

        log::info!("Dynamically registered model '{model_id}'");
        Ok(())
    }

    /// Unregister a dynamically-added model, stopping all its instances first.
    pub async fn unregister_model(&self, model_id: &str) -> Result<(), String> {
        self.stop_model(model_id).await?;
        self.backends.write().await.remove(model_id);
        self.models.write().await.remove(model_id);
        log::info!("Unregistered model '{model_id}'");
        Ok(())
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
            .read()
            .await
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
            // All slots taken — pick the instance with the fewest in-flight requests
            // so the backend (e.g. llama-server HTTP API) can handle the concurrency.
            if let Some(inst) = managed
                .instances
                .iter()
                .filter(|i| i.status != ModelStatus::ShuttingDown)
                .min_by_key(|i| i.active_requests)
            {
                log::debug!(
                    "All instances occupied for '{model_id}', routing to least-loaded '{}' (active_requests={})",
                    &inst.instance_id[..8],
                    inst.active_requests,
                );
                return Ok(inst.instance_id.clone());
            }
            return Err(format!(
                "Model '{model_id}' at max instances ({max_instances}) and all are shutting down",
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
                        inst.handle = Some(Arc::new(handle));
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
            .read()
            .await
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


        // Extract the handle while briefly holding the read lock, then drop it
        // before the potentially long-running backend.execute() call so writers
        // (ensure_running, stop_model, reap_idle) are not starved.
        let handle = {
            let models = self.models.read().await;
            let managed = models.get(model_id).ok_or("Model not found")?;
            let inst = managed
                .instances
                .iter()
                .find(|i| i.instance_id == *instance_id)
                .ok_or("Instance not found")?;

            inst.handle.as_ref().cloned().ok_or("Instance handle not ready")?
        };

        let result = backend
            .execute(&handle, &enriched_request, &self.models_dir)
            .await;

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
        let backend = self.backends.read().await.get(model_id).cloned();
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
        let backends = self.backends.read().await;
        let mut models = self.models.write().await;
        for (model_id, managed) in models.iter_mut() {
            let backend = backends.get(model_id).cloned();
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

    /// Find a dynamically-registered model that supports a given task type.
    /// Returns the model_id of the highest-priority match, if any.
    pub async fn find_model_for_task(&self, task: &TaskType) -> Option<String> {
        let models = self.models.read().await;
        let mut best: Option<(&str, u32)> = None;
        for (id, managed) in models.iter() {
            if managed.def.tasks.contains(task) {
                match best {
                    Some((_, pri)) if managed.def.priority <= pri => {}
                    _ => best = Some((id.as_str(), managed.def.priority)),
                }
            }
        }
        best.map(|(id, _)| id.to_string())
    }

    /// Get a clone of the ModelDef for a specific model id.
    pub async fn get_model_def(&self, model_id: &str) -> Option<ModelDef> {
        let models = self.models.read().await;
        models.get(model_id).map(|m| m.def.clone())
    }

    /// Get the port of a running llama-server instance (for frontend streaming).
    pub async fn get_llama_port(&self, model_id: &str) -> Option<u16> {
        let models = self.models.read().await;
        models.get(model_id).and_then(|m| {
            m.instances.iter().find_map(|inst| {
                if let Some(handle) = &inst.handle {
                    if let ModelHandle::Process(ph) = handle.as_ref() {
                        return ph.port;
                    }
                }
                None
            })
        })
    }

    /// Get the port of a specific instance by model_id and instance_id.
    pub async fn get_instance_port(&self, model_id: &str, instance_id: &str) -> Option<u16> {
        let models = self.models.read().await;
        models.get(model_id).and_then(|m| {
            m.instances.iter().find_map(|inst| {
                if inst.instance_id == instance_id {
                    if let Some(handle) = &inst.handle {
                        if let ModelHandle::Process(ph) = handle.as_ref() {
                            return ph.port;
                        }
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

            let backend = self.backends.read().await.get(model_id).cloned();
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
