use crate::registry::types::{BackendKind, ModelDef, ModelKind, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomModelProfile {
    Llm,
    Vlm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomModelEntry {
    pub id: String,
    pub name: String,
    pub model_file: String,
    pub profile: CustomModelProfile,
    #[serde(default)]
    pub engine_adapter: String,
    #[serde(default)]
    pub max_instances: u32,
    #[serde(default)]
    pub idle_timeout_s: u64,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub memory_mb: u32,
    #[serde(default)]
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CustomRegistryFile {
    version: u32,
    models: Vec<CustomModelEntry>,
}

pub fn default_custom_registry_path(models_dir: &Path) -> PathBuf {
    models_dir.join(".nela").join("custom-models.json")
}

pub fn load_custom_models(models_dir: &Path) -> Result<Vec<ModelDef>, String> {
    let path = default_custom_registry_path(models_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(&path)
        .map_err(|e| format!("Failed to read custom registry '{}': {e}", path.display()))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let parsed: CustomRegistryFile = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse custom registry '{}': {e}", path.display()))?;

    if parsed.version != REGISTRY_VERSION {
        return Err(format!(
            "Unsupported custom registry version {} in '{}'",
            parsed.version,
            path.display()
        ));
    }

    Ok(parsed.models.into_iter().map(entry_to_model_def).collect())
}

pub fn upsert_custom_model(models_dir: &Path, entry: CustomModelEntry) -> Result<(), String> {
    let path = default_custom_registry_path(models_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create custom registry directory '{}': {e}",
                parent.display()
            )
        })?;
    }

    let mut file = if path.exists() {
        let bytes = fs::read(&path)
            .map_err(|e| format!("Failed to read custom registry '{}': {e}", path.display()))?;
        if bytes.is_empty() {
            CustomRegistryFile {
                version: REGISTRY_VERSION,
                models: Vec::new(),
            }
        } else {
            serde_json::from_slice::<CustomRegistryFile>(&bytes).map_err(|e| {
                format!("Failed to parse custom registry '{}': {e}", path.display())
            })?
        }
    } else {
        CustomRegistryFile {
            version: REGISTRY_VERSION,
            models: Vec::new(),
        }
    };

    if file.version != REGISTRY_VERSION {
        return Err(format!(
            "Unsupported custom registry version {} in '{}'",
            file.version,
            path.display()
        ));
    }

    if let Some(existing) = file.models.iter_mut().find(|m| m.id == entry.id) {
        *existing = entry;
    } else {
        file.models.push(entry);
    }

    let json = serde_json::to_vec_pretty(&file)
        .map_err(|e| format!("Failed to serialize custom registry: {e}"))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write custom registry '{}': {e}", path.display()))
}

pub fn remove_custom_model(models_dir: &Path, model_id: &str) -> Result<bool, String> {
    let path = default_custom_registry_path(models_dir);
    if !path.exists() {
        return Ok(false);
    }

    let bytes = fs::read(&path)
        .map_err(|e| format!("Failed to read custom registry '{}': {e}", path.display()))?;
    if bytes.is_empty() {
        return Ok(false);
    }

    let mut file: CustomRegistryFile = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse custom registry '{}': {e}", path.display()))?;

    if file.version != REGISTRY_VERSION {
        return Err(format!(
            "Unsupported custom registry version {} in '{}'",
            file.version,
            path.display()
        ));
    }

    let before = file.models.len();
    file.models.retain(|m| m.id != model_id);
    let removed = file.models.len() != before;
    if !removed {
        return Ok(false);
    }

    let json = serde_json::to_vec_pretty(&file)
        .map_err(|e| format!("Failed to serialize custom registry: {e}"))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write custom registry '{}': {e}", path.display()))?;
    Ok(true)
}

pub fn is_custom_model(models_dir: &Path, model_id: &str) -> Result<bool, String> {
    let path = default_custom_registry_path(models_dir);
    if !path.exists() {
        return Ok(false);
    }
    let bytes = fs::read(&path)
        .map_err(|e| format!("Failed to read custom registry '{}': {e}", path.display()))?;
    if bytes.is_empty() {
        return Ok(false);
    }
    let file: CustomRegistryFile = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse custom registry '{}': {e}", path.display()))?;
    Ok(file.models.iter().any(|m| m.id == model_id))
}

fn entry_to_model_def(entry: CustomModelEntry) -> ModelDef {
    let mut params = entry.params;
    params
        .entry("custom_profile".to_string())
        .or_insert_with(|| match entry.profile {
            CustomModelProfile::Llm => "llm".to_string(),
            CustomModelProfile::Vlm => "vlm".to_string(),
        });
    params
        .entry("engine_adapter".to_string())
        .or_insert_with(|| {
            if entry.engine_adapter.trim().is_empty() {
                "llama_cpp".to_string()
            } else {
                entry.engine_adapter.clone()
            }
        });

    ModelDef {
        id: entry.id,
        name: entry.name,
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: entry.model_file,
        tasks: match entry.profile {
            CustomModelProfile::Llm => vec![
                TaskType::Chat,
                TaskType::Summarize,
                TaskType::Mindmap,
                TaskType::Enrich,
                TaskType::Grade,
                TaskType::Hyde,
                TaskType::PodcastScript,
            ],
            CustomModelProfile::Vlm => vec![
                TaskType::Chat,
                TaskType::Summarize,
                TaskType::Mindmap,
                TaskType::Enrich,
                TaskType::Grade,
                TaskType::Hyde,
                TaskType::PodcastScript,
                TaskType::VisionChat,
            ],
        },
        auto_start: false,
        max_instances: if entry.max_instances == 0 { 2 } else { entry.max_instances },
        // Default idle timeout of 30s for custom models if not specified.
        // 0 means immediate reap, which is too aggressive for inference models.
        idle_timeout_s: if entry.idle_timeout_s == 0 { 30 } else { entry.idle_timeout_s },
        priority: if entry.priority == 0 { 12 } else { entry.priority },
        memory_mb: if entry.memory_mb == 0 { 1600 } else { entry.memory_mb },
        gdrive_id: None,
        is_zip: false,
        params,
        task_priorities: HashMap::new(),
    }
}
