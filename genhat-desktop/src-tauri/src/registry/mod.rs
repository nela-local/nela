//! Model registry: loads model definitions from config and provides lookups.

pub mod types;

use crate::config;
use types::{ModelDef, TaskType};

/// The model registry holds all model definitions loaded from `models.toml`.
/// It is immutable after initialization — models are not added at runtime.
#[derive(Debug)]
pub struct ModelRegistry {
    models: Vec<ModelDef>,
}

impl ModelRegistry {
    /// Load all model definitions from the embedded `models.toml`.
    pub fn load() -> Result<Self, String> {
        let models = config::load_model_definitions()?;
        log::info!(
            "ModelRegistry: loaded {} model definition(s): [{}]",
            models.len(),
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
        );
        Ok(Self { models })
    }

    /// Get all registered model definitions.
    pub fn all(&self) -> &[ModelDef] {
        &self.models
    }

    /// Find a model definition by its id.
    pub fn get(&self, id: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Find all models that can handle a given task type, sorted by priority (highest first).
    /// Uses per-task priority overrides when available, otherwise falls back
    /// to the model's default priority.
    pub fn find_for_task(&self, task: &TaskType) -> Vec<&ModelDef> {
        let mut matches: Vec<&ModelDef> = self
            .models
            .iter()
            .filter(|m| m.supports_task(task))
            .collect();
        matches.sort_by(|a, b| b.priority_for_task(task).cmp(&a.priority_for_task(task)));
        matches
    }

    /// Get models that should auto-start.
    pub fn auto_start_models(&self) -> Vec<&ModelDef> {
        self.models.iter().filter(|m| m.auto_start).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_load() {
        let registry = ModelRegistry::load().expect("Should load registry");
        assert!(!registry.all().is_empty());
    }

    #[test]
    fn test_find_for_task() {
        let registry = ModelRegistry::load().expect("Should load registry");
        let chat_models = registry.find_for_task(&TaskType::Chat);
        assert!(!chat_models.is_empty(), "Should find models for Chat task");
    }
}
