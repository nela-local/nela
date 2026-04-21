//! Model management commands — list, start, stop, status.

use crate::process::ProcessManager;
use crate::commands::workspace::WorkspaceState;
use crate::registry::custom::{self, CustomModelEntry, CustomModelProfile};
use crate::registry::types::{
    BackendKind, ModelDef, ModelInfo, ModelKind, ModelStatus, TaskType,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Managed state wrapper for the ProcessManager.
pub struct ProcessManagerState(pub Arc<ProcessManager>);

// ── Legacy-compatible model listing ─────────────────────────────────────────
// These maintain backward compatibility with the existing frontend.

#[derive(serde::Serialize)]
pub struct ModelFile {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiscoveredModelUnit {
    pub key: String,
    pub category: String,
    pub repo_id: String,
    pub container_rel_path: String,
    pub llm_rel_path: String,
    pub llm_abs_path: String,
    pub llm_file_name: String,
    pub mmproj_rel_path: Option<String>,
    pub supports_vision: bool,
}

fn rel_to_unix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_gguf_file(path: &Path) -> bool {
    path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false)
}

fn collect_gguf_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gguf_files(&path, out);
        } else if is_gguf_file(&path) {
            out.push(path);
        }
    }
}

fn choose_primary_llm(ggufs: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates: Vec<(u64, &PathBuf)> = ggufs
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.to_ascii_lowercase().contains("mmproj"))
                .unwrap_or(false)
        })
        .map(|path| {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            (size, path)
        })
        .collect();

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.first().map(|(_, path)| (*path).clone())
}

fn choose_mmproj(ggufs: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates: Vec<(u64, &PathBuf)> = ggufs
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase().contains("mmproj"))
                .unwrap_or(false)
        })
        .map(|path| {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            (size, path)
        })
        .collect();

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.first().map(|(_, path)| (*path).clone())
}

fn inspect_container(
    models_dir: &Path,
    category: &str,
    container_path: &Path,
    repo_id: String,
) -> Option<DiscoveredModelUnit> {
    let mut ggufs = Vec::new();
    collect_gguf_files(container_path, &mut ggufs);
    if ggufs.is_empty() {
        return None;
    }

    let primary = choose_primary_llm(&ggufs)?;
    let mmproj = choose_mmproj(&ggufs);

    let container_rel = container_path.strip_prefix(models_dir).ok()?.to_path_buf();
    let llm_rel = primary.strip_prefix(models_dir).ok()?.to_path_buf();
    let llm_name = primary
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model.gguf")
        .to_string();
    let mmproj_rel = mmproj
        .as_ref()
        .and_then(|path| path.strip_prefix(models_dir).ok())
        .map(rel_to_unix);

    Some(DiscoveredModelUnit {
        key: format!("{}:{}", category, repo_id),
        category: category.to_string(),
        repo_id,
        container_rel_path: rel_to_unix(&container_rel),
        llm_rel_path: rel_to_unix(&llm_rel),
        llm_abs_path: primary.to_string_lossy().to_string(),
        llm_file_name: llm_name,
        mmproj_rel_path: mmproj_rel,
        supports_vision: mmproj.is_some(),
    })
}

fn discover_local_units(models_dir: &Path) -> Vec<DiscoveredModelUnit> {
    let mut units = Vec::new();

    for category in ["LLM", "LiquidAI-VLM"] {
        let category_root = models_dir.join(category);
        if !category_root.is_dir() {
            continue;
        }

        let owners = match std::fs::read_dir(&category_root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for owner in owners.flatten() {
            let owner_path = owner.path();
            if !owner_path.is_dir() {
                continue;
            }

            let owner_name = owner
                .file_name()
                .to_string_lossy()
                .to_string();

            let mut repo_dirs = Vec::new();
            if let Ok(children) = std::fs::read_dir(&owner_path) {
                for child in children.flatten() {
                    if child.path().is_dir() {
                        repo_dirs.push(child.path());
                    }
                }
            }

            // Preferred layout: <category>/<owner>/<repo>/...
            if !repo_dirs.is_empty() {
                for repo_path in repo_dirs {
                    let repo_name = repo_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string();
                    if repo_name.is_empty() {
                        continue;
                    }
                    let repo_id = format!("{owner_name}/{repo_name}");
                    if let Some(unit) = inspect_container(models_dir, category, &repo_path, repo_id)
                    {
                        units.push(unit);
                    }
                }
                continue;
            }

            // Backward-compatible one-level container fallback: <category>/<repo>/...
            if let Some(unit) = inspect_container(models_dir, category, &owner_path, owner_name) {
                units.push(unit);
            }
        }
    }

    units.sort_by(|a, b| {
        a.repo_id
            .cmp(&b.repo_id)
            .then_with(|| a.llm_file_name.cmp(&b.llm_file_name))
    });
    units
}

fn build_discovered_model_id(category: &str, repo_id: &str, llm_rel_path: &str) -> String {
    let normalized = format!("{}-{}", category, repo_id)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    category.hash(&mut hasher);
    repo_id.hash(&mut hasher);
    llm_rel_path.hash(&mut hasher);
    let suffix = hasher.finish();

    format!("disk-{}-{:08x}", normalized, suffix as u32)
}

fn default_chat_tasks() -> Vec<TaskType> {
    vec![
        TaskType::Chat,
        TaskType::Summarize,
        TaskType::Mindmap,
        TaskType::Enrich,
        TaskType::Grade,
        TaskType::Hyde,
        TaskType::PodcastScript,
    ]
}

fn model_def_from_discovered_unit(unit: &DiscoveredModelUnit) -> ModelDef {
    let mut tasks = default_chat_tasks();
    if unit.supports_vision {
        tasks.push(TaskType::VisionChat);
    }

    let mut params = HashMap::new();
    params.insert("engine_adapter".to_string(), "llama_cpp".to_string());
    params.insert("custom_profile".to_string(), if unit.supports_vision {
        "vlm".to_string()
    } else {
        "llm".to_string()
    });
    params.insert("discovery_source".to_string(), "disk_scan".to_string());
    params.insert("hf_repo_id".to_string(), unit.repo_id.clone());
    params.insert("container_path".to_string(), unit.container_rel_path.clone());
    if let Some(mmproj) = &unit.mmproj_rel_path {
        params.insert("mmproj_file".to_string(), mmproj.clone());
        params.insert("max_tokens".to_string(), "1024".to_string());
    }

    let id = build_discovered_model_id(&unit.category, &unit.repo_id, &unit.llm_rel_path);
    let file_stem = PathBuf::from(&unit.llm_file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&unit.llm_file_name)
        .to_string();
    let name = if unit.supports_vision {
        format!("{} ({file_stem}, VLM)", unit.repo_id)
    } else {
        format!("{} ({file_stem})", unit.repo_id)
    };

    ModelDef {
        id,
        name,
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: unit.llm_rel_path.clone(),
        tasks,
        auto_start: false,
        max_instances: 2,
        idle_timeout_s: 30,
        priority: 12,
        memory_mb: 1600,
        gdrive_id: None,
        is_zip: false,
        params,
        task_priorities: HashMap::new(),
    }
}

fn merge_discovered_params(
    existing: Option<&ModelInfo>,
    discovered: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut merged = existing.map(|model| model.params.clone()).unwrap_or_default();

    // If a previously vision-capable unit no longer has an mmproj companion,
    // clear the stale pointer while preserving all other user-tuned values.
    if !discovered.contains_key("mmproj_file") {
        merged.remove("mmproj_file");
    }

    for (key, value) in discovered {
        merged.insert(key.clone(), value.clone());
    }

    merged
}

async fn sync_discovered_models_internal(pm: &Arc<ProcessManager>) -> Result<(), String> {
    let models_dir = crate::paths::resolve_models_dir();
    let units = discover_local_units(&models_dir);
    let discovered_defs: Vec<ModelDef> = units
        .iter()
        .map(model_def_from_discovered_unit)
        .collect();

    let existing = pm.list_models().await;
    let mut existing_by_id: HashMap<String, ModelInfo> = HashMap::new();
    for model in existing {
        existing_by_id.insert(model.id.clone(), model);
    }

    let mut discovered_ids = std::collections::HashSet::new();
    for def in discovered_defs {
        discovered_ids.insert(def.id.clone());
        let desired_tasks: Vec<String> = def.tasks.iter().map(|t| t.to_string()).collect();
        let existing = existing_by_id.get(&def.id);

        let needs_update = match existing {
            None => true,
            Some(existing) => {
                let existing_mmproj = existing.params.get("mmproj_file");
                let desired_mmproj = def.params.get("mmproj_file");
                existing.model_file != def.model_file
                    || existing.tasks != desired_tasks
                    || existing_mmproj != desired_mmproj
            }
        };

        if needs_update {
            let mut next_def = def;
            next_def.params = merge_discovered_params(existing, &next_def.params);
            pm.register_model(next_def).await?;
        }
    }

    // Remove stale disk-scanned models that no longer exist physically.
    for (id, model) in existing_by_id {
        let is_disk_scan = model
            .params
            .get("discovery_source")
            .map(|v| v == "disk_scan")
            .unwrap_or(false);
        if is_disk_scan && !discovered_ids.contains(&id) {
            let _ = pm.unregister_model(&id).await;
        }
    }

    Ok(())
}

/// List available .gguf LLM model files from the LiquidAI-LLM subfolder.
/// Legacy-compatible with the original `list_models` command.
#[tauri::command]
pub fn list_models() -> Vec<ModelFile> {
    let models_dir = get_models_dir();
    discover_local_units(&models_dir)
        .into_iter()
        .filter(|unit| unit.category == "LLM")
        .map(|unit| ModelFile {
            name: unit.llm_file_name,
            path: unit.llm_abs_path,
        })
        .collect()
}

/// List available .gguf VLM model files from the LiquidAI-VLM subfolder.
/// Excludes mmproj files (those are companion projector weights, not selectable models).
#[tauri::command]
pub fn list_vision_models() -> Vec<ModelFile> {
    let models_dir = get_models_dir();
    discover_local_units(&models_dir)
        .into_iter()
        .filter(|unit| unit.supports_vision)
        .map(|unit| ModelFile {
            name: unit.llm_file_name,
            path: unit.llm_abs_path,
        })
        .collect()
}

#[tauri::command]
pub fn discover_local_model_units() -> Result<Vec<DiscoveredModelUnit>, String> {
    let models_dir = get_models_dir();
    Ok(discover_local_units(&models_dir))
}

#[tauri::command]
pub async fn sync_discovered_models(
    state: State<'_, ProcessManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    sync_discovered_models_internal(&state.0).await?;
    Ok(state.0.list_models().await)
}

// ── New unified model commands ──────────────────────────────────────────────

/// List all registered models with their current status.
#[tauri::command]
pub async fn list_registered_models(
    state: State<'_, ProcessManagerState>,
) -> Result<Vec<ModelInfo>, String> {
    sync_discovered_models_internal(&state.0).await?;
    Ok(state.0.list_models().await)
}

/// List models defined in models.toml, including those not yet downloaded.
#[tauri::command]
pub fn list_model_catalog() -> Result<Vec<ModelInfo>, String> {
    let defs = crate::config::load_model_definitions()?;
    let models_dir = crate::paths::resolve_models_dir();

    let mut catalog: Vec<ModelInfo> = defs
        .into_iter()
        .map(|def| {
            let is_downloaded = def.files_exist(&models_dir);
            let model_profile = def.params.get("custom_profile").cloned();
            let engine_adapter = def.params.get("engine_adapter").cloned();
            let model_source = if model_profile.is_some() {
                "custom".to_string()
            } else {
                "builtin".to_string()
            };

            ModelInfo {
                id: def.id,
                name: def.name,
                backend: format!("{:?}", def.backend),
                kind: format!("{:?}", def.kind),
                model_file: def.model_file,
                tasks: def.tasks.iter().map(|t| t.to_string()).collect(),
                status: ModelStatus::Unloaded,
                instance_count: 0,
                memory_mb: def.memory_mb,
                gdrive_id: def.gdrive_id,
                is_zip: def.is_zip,
                priority: def.priority,
                is_downloaded,
                model_source,
                model_profile,
                engine_adapter,
                params: def.params,
            }
        })
        .collect();

    catalog.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.name.cmp(&b.name)));
    Ok(catalog)
}

/// Update a model's runtime parameters.
/// If the model is currently loaded, it will be restarted to apply startup-level settings.
#[tauri::command]
pub async fn update_model_params(
    model_id: String,
    params: HashMap<String, String>,
    state: State<'_, ProcessManagerState>,
) -> Result<ModelInfo, String> {
    state.0.update_model_params(&model_id, params).await?;
    state
        .0
        .list_models()
        .await
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Model '{model_id}' not found after update"))
}

/// Get the status of a specific model.
#[tauri::command]
pub async fn get_model_status(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<ModelStatus, String> {
    state
        .0
        .model_status(&model_id)
        .await
        .ok_or_else(|| format!("Model '{model_id}' not found"))
}

/// Manually start (pre-warm) a model.
#[tauri::command]
pub async fn start_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<String, String> {
    let instance_id = state.0.ensure_running(&model_id, false).await?;
    Ok(instance_id)
}

/// Stop a model (all instances).
#[tauri::command]
pub async fn stop_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<(), String> {
    state.0.stop_model(&model_id).await
}

/// Switch to a different LLM model by file path or registry ID.
/// If it's a registered ID, it uses the config from models.toml.
/// Otherwise, it falls back to dynamically registering the model from the file path.
#[tauri::command]
pub async fn switch_model(
    model_identifier: String,
    state: State<'_, ProcessManagerState>,
) -> Result<String, String> {
    // Keep the previous model warm instead of stopping on every switch.

    // First check if the identifier is a registered model ID
    if let Some(def) = state.0.get_model_def(&model_identifier).await {
        let instance_id = state.0.ensure_running(&def.id, false).await?;
        if let Some(evict) = state.0.rotate_active_llm_keep_previous(&def.id).await {
            let _ = state.0.stop_model(&evict).await;
        }
        return Ok(format!("server started (instance: {})", &instance_id[..8]));
    }

    // Fallback: assume it's a file path for dynamic registration
    let path = PathBuf::from(&model_identifier);
    if !path.exists() {
        return Err(format!("Model file or ID not found: {model_identifier}"));
    }

    // Derive a model ID from the filename (e.g. "MyModel-Q4_0.gguf" → "mymodel-q4_0")
    let file_name = path
        .file_stem()
        .ok_or("Invalid model path: no filename")?
        .to_string_lossy()
        .to_string();
    let model_id = file_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>();

    // Build a ModelDef for the new model.
    // Use the absolute path as model_file — Path::join with an absolute path
    // ignores the base, so the backend will use it directly.
    let def = ModelDef {
        id: model_id.clone(),
        name: file_name.clone(),
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: model_identifier.clone(),
        tasks: vec![
            TaskType::Chat,
            TaskType::Summarize,
            TaskType::Mindmap,
            TaskType::Enrich,
            TaskType::Grade,
            TaskType::Hyde,
            TaskType::PodcastScript,
        ],
        auto_start: false,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 10,
        memory_mb: 1400,
        params: HashMap::new(),
        task_priorities: HashMap::new(),
        gdrive_id: None,
        is_zip: false,
    };

    // Register and start the new model
    state.0.register_model(def).await?;
    let instance_id = state.0.ensure_running(&model_id, false).await?;
    if let Some(evict) = state.0.rotate_active_llm_keep_previous(&model_id).await {
        let _ = state.0.stop_model(&evict).await;
    }

    Ok(format!("server started (instance: {})", &instance_id[..8]))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportModelProfile {
    Llm,
    Vlm,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ImportDownloadedModelRequest {
    pub folder: String,
    pub filename: String,
    pub profile: ImportModelProfile,
    pub display_name: Option<String>,
    pub mmproj_file: Option<String>,
    pub engine_adapter: Option<String>,
}

/// Import a downloaded GGUF model into the runtime and persist it in custom registry.
///
/// This enables downloaded Hugging Face models to be recognized by NELA
/// immediately without requiring app restart.
#[tauri::command]
pub async fn import_downloaded_model(
    req: ImportDownloadedModelRequest,
    state: State<'_, ProcessManagerState>,
) -> Result<ModelInfo, String> {
    let models_dir = crate::paths::resolve_models_dir();
    let model_rel = PathBuf::from(&req.folder).join(&req.filename);
    let model_path = models_dir.join(&model_rel);

    if !model_path.exists() {
        return Err(format!(
            "Model file not found: {}",
            model_path.display()
        ));
    }
    if model_path.extension().and_then(|s| s.to_str()) != Some("gguf") {
        return Err(format!(
            "Only .gguf models can be imported with this command: {}",
            model_path.display()
        ));
    }

    let model_id = build_custom_model_id(&req.folder, &req.filename);
    let display_name = req
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(&req.filename)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| req.filename.clone())
        });

    let mut params = HashMap::new();
    let engine_adapter = req
        .engine_adapter
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "llama_cpp".to_string());

    if engine_adapter != "llama_cpp" {
        return Err(format!(
            "Unsupported engine adapter '{}'. Currently supported: llama_cpp",
            engine_adapter
        ));
    }

    params.insert("engine_adapter".to_string(), engine_adapter.clone());

    match req.profile {
        ImportModelProfile::Llm => {
            params.insert("custom_profile".to_string(), "llm".to_string());
        }
        ImportModelProfile::Vlm => {
            params.insert("custom_profile".to_string(), "vlm".to_string());
        }
    }

    let has_mmproj = if let Some(mmproj) = req
        .mmproj_file
        .clone()
        .filter(|s| !s.trim().is_empty())
    {
        let mmproj_path = std::path::Path::new(&mmproj);
        let mmproj_exists = if mmproj_path.is_absolute() {
            mmproj_path.exists()
        } else {
            models_dir.join(&mmproj).exists()
        };
        if !mmproj_exists {
            return Err(format!("VLM companion file not found: {}", mmproj));
        }
        params.insert("mmproj_file".to_string(), mmproj);
        params.insert("max_tokens".to_string(), "1024".to_string());
        true
    } else {
        false
    };

    if matches!(req.profile, ImportModelProfile::Vlm) && !has_mmproj {
        return Err("VLM imports require an mmproj companion file. Provide mmproj_file.".to_string());
    }

    let mut tasks = default_chat_tasks();
    if has_mmproj {
        tasks.push(TaskType::VisionChat);
    }

    let def = ModelDef {
        id: model_id.clone(),
        name: display_name.clone(),
        backend: BackendKind::LlamaServer,
        kind: ModelKind::ChildProcess,
        model_file: model_rel.to_string_lossy().to_string(),
        tasks,
        auto_start: false,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 12,
        memory_mb: 1600,
        params: params.clone(),
        task_priorities: HashMap::new(),
        gdrive_id: None,
        is_zip: false,
    };

    state.0.register_model(def).await?;

    let entry = CustomModelEntry {
        id: model_id.clone(),
        name: display_name,
        model_file: model_rel.to_string_lossy().to_string(),
        profile: match req.profile {
            ImportModelProfile::Llm => CustomModelProfile::Llm,
            ImportModelProfile::Vlm => CustomModelProfile::Vlm,
        },
        engine_adapter,
        max_instances: 2,
        idle_timeout_s: 0,
        priority: 12,
        memory_mb: 1600,
        params,
    };

    custom::upsert_custom_model(&models_dir, entry)?;

    let imported = state
        .0
        .list_models()
        .await
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| "Imported model was not found in runtime list".to_string())?;

    Ok(imported)
}

/// Unregister a previously imported custom model.
#[tauri::command]
pub async fn unregister_custom_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
) -> Result<(), String> {
    let models_dir = crate::paths::resolve_models_dir();
    if !custom::is_custom_model(&models_dir, &model_id)? {
        return Err(format!("Model '{}' is not a custom imported model", model_id));
    }

    state.0.unregister_model(&model_id).await?;
    let _ = custom::remove_custom_model(&models_dir, &model_id)?;
    Ok(())
}

/// Stop the LLM server. Legacy-compatible.
#[tauri::command]
pub async fn stop_llama(state: State<'_, ProcessManagerState>) -> Result<(), String> {
    let active_id = state.0.active_llm_id().await;
    let prev_id = state.0.previous_llm_id().await;
    state.0.stop_model(&active_id).await?;
    if let Some(prev) = prev_id {
        if prev != active_id {
            let _ = state.0.stop_model(&prev).await;
        }
    }
    Ok(())
}

/// Get the port of the running llama-server (for frontend SSE streaming).
/// Emits `model-loading` events during model initialization.
#[tauri::command]
pub async fn get_llama_port(
    app: AppHandle,
    state: State<'_, ProcessManagerState>,
) -> Result<u16, String> {
    let active_id = state.0.active_llm_id().await;
    let mut candidates = Vec::new();
    if !active_id.is_empty() {
        candidates.push(active_id);
    }

    for id in state.0.find_models_for_task(&TaskType::Chat).await {
        if !candidates.contains(&id) {
            candidates.push(id);
        }
    }

    if candidates.is_empty() {
        return Err("No chat-capable model is registered".to_string());
    }

    let mut errors = Vec::new();
    for id in candidates {
        if let Some(port) = state.0.get_llama_port(&id).await {
            return Ok(port);
        }

        // Emit loading event before starting model
        let _ = app.emit("model-loading", serde_json::json!({
            "model_id": &id,
            "status": "starting",
            "message": format!("Loading model {}...", &id)
        }));
        log::info!("Starting model {} for chat...", &id);

        // Increased timeout to 150s to account for large model loading
        let start = std::time::Instant::now();
        match tokio::time::timeout(
            std::time::Duration::from_secs(150),
            state.0.ensure_running(&id, false),
        )
        .await
        {
            Ok(Ok(_)) => {
                let elapsed = start.elapsed().as_secs();
                let _ = app.emit("model-loading", serde_json::json!({
                    "model_id": &id,
                    "status": "ready",
                    "message": format!("Model {} ready ({}s)", &id, elapsed)
                }));
                if let Some(port) = state.0.get_llama_port(&id).await {
                    return Ok(port);
                }
                errors.push(format!("{id}: started but no port assigned"));
            }
            Ok(Err(e)) => {
                let _ = app.emit("model-loading", serde_json::json!({
                    "model_id": &id,
                    "status": "error",
                    "message": format!("Failed to start {}: {}", &id, &e)
                }));
                errors.push(format!("{id}: {e}"));
            }
            Err(_) => {
                let _ = app.emit("model-loading", serde_json::json!({
                    "model_id": &id,
                    "status": "timeout",
                    "message": format!("{} timed out after 150s", &id)
                }));
                errors.push(format!("{id}: timed out while starting"));
            }
        }
    }

    Err(format!(
        "No runnable chat model available: {}",
        errors.join(" | ")
    ))
}

/// Get estimated total memory usage of all loaded models (MB).
#[tauri::command]
pub async fn get_memory_usage(
    state: State<'_, ProcessManagerState>,
) -> Result<u32, String> {
    Ok(state.0.memory_usage().await)
}

/// Return the current working directory so frontend state can be scoped per workspace.
#[tauri::command]
pub fn get_workspace_scope(workspace: State<'_, WorkspaceState>) -> Result<String, String> {
    workspace.0.get_workspace_scope()
}

// ── Helper ──────────────────────────────────────────────────────────────────

/// Resolve the models directory.
///
/// Resolution order:
///   1. `GENHAT_MODEL_PATH` environment variable (absolute path to dir or any file inside it)
///   2. `models/` folder next to the running executable (production install location)
///   3. Tauri resource directory (Linux: `/usr/lib/GenHat/models/`)
///   4. `../../models` relative to the crate root at compile time (dev / cargo run fallback)
pub fn get_models_dir() -> PathBuf {
    crate::paths::resolve_models_dir()
}

fn build_custom_model_id(folder: &str, filename: &str) -> String {
    let stem = PathBuf::from(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.to_string());

    let normalized = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    folder.hash(&mut hasher);
    filename.hash(&mut hasher);
    let suffix = hasher.finish();

    format!("custom-{}-{:08x}", normalized, suffix as u32)
}

// ── File utilities ──────────────────────────────────────────────────────────

use base64::{engine::general_purpose::STANDARD, Engine};

/// Read an image file and return it as a base64-encoded data URL.
/// Used for image preview in the frontend.
#[tauri::command]
pub fn read_image_base64(path: String) -> Result<String, String> {
    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    // Determine MIME type from extension
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };

    let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {e}"))?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("data:{};base64,{}", mime, b64))
}
