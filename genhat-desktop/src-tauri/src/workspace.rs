use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

const REGISTRY_VERSION: u32 = 1;
const NELA_SCHEMA_VERSION: u32 = 1;
const RAG_MODEL_PREFS_FILE: &str = "rag_model_prefs.json";

/// User preferences for RAG pipeline model selection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RagModelPreferences {
    /// Preferred embedding model ID for vector similarity search.
    #[serde(default)]
    pub embed_model_id: Option<String>,
    /// Preferred LLM model ID for enrichment and chat tasks.
    #[serde(default)]
    pub llm_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub nela_path: Option<String>,
    pub cache_dir: String,
    pub created_at: u64,
    pub last_opened_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub schema_version: u32,
    pub created_at: u64,
    pub workspace_id: String,
    pub workspace_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceOpenResult {
    pub workspace: WorkspaceRecord,
    pub frontend_state_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceRegistry {
    pub version: u32,
    pub active_workspace_id: Option<String>,
    pub workspaces: Vec<WorkspaceRecord>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            active_workspace_id: None,
            workspaces: Vec::new(),
        }
    }
}

pub struct WorkspaceManager {
    registry_path: PathBuf,
    workspaces_root: PathBuf,
    inner: Mutex<WorkspaceRegistry>,
}

impl WorkspaceManager {
    pub fn new(app_data_dir: &Path) -> Result<Self, String> {
        let workspaces_root = app_data_dir.join("workspaces");
        std::fs::create_dir_all(&workspaces_root)
            .map_err(|e| format!("Failed to create workspaces root {}: {e}", workspaces_root.display()))?;

        let registry_path = workspaces_root.join("registry.json");
        let mut registry = Self::load_registry(&registry_path)?;

        if registry.workspaces.is_empty() {
            let now = now_unix_s();
            let default_id = "default".to_string();
            let default_cache_dir = workspaces_root.join(&default_id).join("cache");
            std::fs::create_dir_all(default_cache_dir.join("rag"))
                .map_err(|e| format!("Failed to create default workspace cache: {e}"))?;

            registry.workspaces.push(WorkspaceRecord {
                id: default_id.clone(),
                name: "Default Workspace".to_string(),
                nela_path: None,
                cache_dir: default_cache_dir.to_string_lossy().to_string(),
                created_at: now,
                last_opened_at: now,
            });
            registry.active_workspace_id = Some(default_id);
            Self::save_registry(&registry_path, &registry)?;
        } else {
            for ws in &registry.workspaces {
                let cache_dir = PathBuf::from(&ws.cache_dir);
                let _ = std::fs::create_dir_all(cache_dir.join("rag"));
            }

            if registry
                .active_workspace_id
                .as_ref()
                .is_none_or(|id| !registry.workspaces.iter().any(|w| &w.id == id))
            {
                registry.active_workspace_id = registry.workspaces.first().map(|w| w.id.clone());
                Self::save_registry(&registry_path, &registry)?;
            }
        }

        Ok(Self {
            registry_path,
            workspaces_root,
            inner: Mutex::new(registry),
        })
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceRecord>, String> {
        let registry = self.lock_registry()?;
        Ok(registry.workspaces.clone())
    }

    /// Returns the active workspace ID, if any.
    pub fn active_workspace_id(&self) -> Option<String> {
        self.lock_registry()
            .ok()
            .and_then(|r| r.active_workspace_id.clone())
    }

    pub fn get_active_workspace(&self) -> Result<WorkspaceRecord, String> {
        let registry = self.lock_registry()?;
        let active_id = registry
            .active_workspace_id
            .as_ref()
            .ok_or_else(|| "No active workspace".to_string())?;

        registry
            .workspaces
            .iter()
            .find(|ws| &ws.id == active_id)
            .cloned()
            .ok_or_else(|| format!("Active workspace '{active_id}' not found"))
    }

    pub fn create_workspace(&self, name: Option<String>) -> Result<WorkspaceRecord, String> {
        let mut registry = self.lock_registry()?;
        let now = now_unix_s();
        let id = format!("ws-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let cache_dir = self.workspaces_root.join(&id).join("cache");

        std::fs::create_dir_all(cache_dir.join("rag"))
            .map_err(|e| format!("Failed to create workspace cache dir: {e}"))?;

        let workspace = WorkspaceRecord {
            id: id.clone(),
            name: name
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Workspace {}", registry.workspaces.len() + 1)),
            nela_path: None,
            cache_dir: cache_dir.to_string_lossy().to_string(),
            created_at: now,
            last_opened_at: now,
        };

        registry.active_workspace_id = Some(id);
        registry.workspaces.push(workspace.clone());
        Self::save_registry(&self.registry_path, &registry)?;
        Ok(workspace)
    }

    pub fn delete_workspace(&self, workspace_id: &str) -> Result<WorkspaceRecord, String> {
        let mut registry = self.lock_registry()?;

        let index = registry
            .workspaces
            .iter()
            .position(|ws| ws.id == workspace_id)
            .ok_or_else(|| format!("Workspace '{workspace_id}' not found"))?;

        let removed = registry.workspaces.remove(index);

        // Remove local extracted cache for the deleted workspace.
        let removed_cache_dir = PathBuf::from(&removed.cache_dir);
        if removed_cache_dir.exists() {
            std::fs::remove_dir_all(&removed_cache_dir).map_err(|e| {
                format!(
                    "Failed to remove deleted workspace cache {}: {e}",
                    removed_cache_dir.display()
                )
            })?;
        }

        // Ensure at least one workspace always exists.
        if registry.workspaces.is_empty() {
            let now = now_unix_s();
            let default_id = "default".to_string();
            let default_cache_dir = self.workspaces_root.join(&default_id).join("cache");
            std::fs::create_dir_all(default_cache_dir.join("rag"))
                .map_err(|e| format!("Failed to create fallback default workspace cache: {e}"))?;

            registry.workspaces.push(WorkspaceRecord {
                id: default_id.clone(),
                name: "Default Workspace".to_string(),
                nela_path: None,
                cache_dir: default_cache_dir.to_string_lossy().to_string(),
                created_at: now,
                last_opened_at: now,
            });
            registry.active_workspace_id = Some(default_id);
        } else {
            let active_still_valid = registry
                .active_workspace_id
                .as_ref()
                .is_some_and(|id| registry.workspaces.iter().any(|ws| &ws.id == id));
            if !active_still_valid {
                registry.active_workspace_id = registry.workspaces.first().map(|ws| ws.id.clone());
            }
        }

        Self::save_registry(&self.registry_path, &registry)?;

        let active_id = registry
            .active_workspace_id
            .clone()
            .ok_or_else(|| "No active workspace after deletion".to_string())?;

        registry
            .workspaces
            .iter()
            .find(|ws| ws.id == active_id)
            .cloned()
            .ok_or_else(|| format!("Active workspace '{active_id}' not found after deletion"))
    }

    pub fn open_workspace(&self, workspace_id: &str) -> Result<WorkspaceRecord, String> {
        let mut registry = self.lock_registry()?;
        let now = now_unix_s();

        let ws_index = registry
            .workspaces
            .iter()
            .position(|ws| ws.id == workspace_id)
            .ok_or_else(|| format!("Workspace '{workspace_id}' not found"))?;

        registry.workspaces[ws_index].last_opened_at = now;
        registry.active_workspace_id = Some(workspace_id.to_string());

        let cache_dir = PathBuf::from(&registry.workspaces[ws_index].cache_dir);
        std::fs::create_dir_all(cache_dir.join("rag"))
            .map_err(|e| format!("Failed to prepare workspace cache dir: {e}"))?;

        let out = registry.workspaces[ws_index].clone();
        Self::save_registry(&self.registry_path, &registry)?;
        Ok(out)
    }

    pub fn clear_active_workspace(&self) -> Result<(), String> {
        let mut registry = self.lock_registry()?;
        registry.active_workspace_id = None;
        Self::save_registry(&self.registry_path, &registry)?;
        Ok(())
    }

    pub fn set_workspace_file(&self, workspace_id: &str, nela_path: &str) -> Result<WorkspaceRecord, String> {
        let mut registry = self.lock_registry()?;
        let ws = registry
            .workspaces
            .iter_mut()
            .find(|ws| ws.id == workspace_id)
            .ok_or_else(|| format!("Workspace '{workspace_id}' not found"))?;

        ws.nela_path = Some(nela_path.to_string());
        let out = ws.clone();
        Self::save_registry(&self.registry_path, &registry)?;
        Ok(out)
    }

    pub fn rename_workspace(
        &self,
        workspace_id: &str,
        new_name: &str,
    ) -> Result<WorkspaceRecord, String> {
        let mut registry = self.lock_registry()?;
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Workspace name cannot be empty".to_string());
        }

        let ws = registry
            .workspaces
            .iter_mut()
            .find(|ws| ws.id == workspace_id)
            .ok_or_else(|| format!("Workspace '{workspace_id}' not found"))?;

        ws.name = trimmed.to_string();
        let out = ws.clone();
        Self::save_registry(&self.registry_path, &registry)?;
        Ok(out)
    }

    pub fn get_workspace_scope(&self) -> Result<String, String> {
        let active = self.get_active_workspace()?;
        Ok(format!("workspace:{}", active.id))
    }

    pub fn get_active_frontend_state(&self) -> Result<Option<String>, String> {
        let active = self.get_active_workspace()?;
        let path = PathBuf::from(active.cache_dir).join("frontend_state.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read frontend state {}: {e}", path.display()))?;
        Ok(Some(text))
    }

    pub fn save_active_frontend_state(&self, frontend_state_json: &str) -> Result<(), String> {
        let active = self.get_active_workspace()?;
        let path = PathBuf::from(active.cache_dir).join("frontend_state.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create frontend state dir {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, frontend_state_json)
            .map_err(|e| format!("Failed to write frontend state {}: {e}", path.display()))
    }

    pub fn save_active_workspace_as_nela(
        &self,
        nela_path: &str,
        frontend_state_json: Option<&str>,
    ) -> Result<WorkspaceRecord, String> {
        if let Some(json) = frontend_state_json {
            self.save_active_frontend_state(json)?;
        }

        let active = self.get_active_workspace()?;
        let cache_dir = PathBuf::from(&active.cache_dir);
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to prepare workspace cache {}: {e}", cache_dir.display()))?;

        let nela_path_buf = PathBuf::from(nela_path);
        if let Some(parent) = nela_path_buf.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create target directory {}: {e}", parent.display()))?;
        }

        let manifest = WorkspaceManifest {
            schema_version: NELA_SCHEMA_VERSION,
            created_at: now_unix_s(),
            workspace_id: active.id.clone(),
            workspace_name: active.name.clone(),
        };

        let file = File::create(&nela_path_buf)
            .map_err(|e| format!("Failed to create .nela file {}: {e}", nela_path_buf.display()))?;

        let mut zip = zip::ZipWriter::new(BufWriter::new(file));
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(6));

        // Core metadata files
        zip.start_file("manifest.json", options)
            .map_err(|e| format!("Failed to start manifest entry: {e}"))?;
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("Failed to serialize manifest: {e}"))?;
        zip.write_all(manifest_json.as_bytes())
            .map_err(|e| format!("Failed to write manifest content: {e}"))?;

        zip.start_file("workspace.json", options)
            .map_err(|e| format!("Failed to start workspace metadata entry: {e}"))?;
        let workspace_json = serde_json::to_string_pretty(&active)
            .map_err(|e| format!("Failed to serialize workspace metadata: {e}"))?;
        zip.write_all(workspace_json.as_bytes())
            .map_err(|e| format!("Failed to write workspace metadata: {e}"))?;

        let frontend_path = cache_dir.join("frontend_state.json");
        if frontend_path.exists() {
            add_file_to_zip(&mut zip, &frontend_path, "frontend_state.json", options)?;
        }

        // Include workspace-scoped assets if present
        for sub in ["rag", "audio", "podcasts", "docs"] {
            let subdir = cache_dir.join(sub);
            if subdir.exists() {
                add_dir_to_zip(&mut zip, &subdir, sub, options)?;
            }
        }

        zip.finish()
            .map_err(|e| format!("Failed to finalize .nela archive: {e}"))?;

        let saved = self.set_workspace_file(&active.id, &nela_path_buf.to_string_lossy())?;
        Ok(saved)
    }

    pub fn save_active_workspace_nela(
        &self,
        frontend_state_json: Option<&str>,
    ) -> Result<WorkspaceRecord, String> {
        let active = self.get_active_workspace()?;
        let nela = active
            .nela_path
            .clone()
            .ok_or_else(|| "Active workspace has no .nela path yet. Use Save As first.".to_string())?;
        self.save_active_workspace_as_nela(&nela, frontend_state_json)
    }

    pub fn open_workspace_nela(
        &self,
        nela_path: &str,
        name: Option<String>,
    ) -> Result<WorkspaceOpenResult, String> {
        let nela_abs = PathBuf::from(nela_path);
        if !nela_abs.exists() {
            return Err(format!(".nela file not found: {}", nela_abs.display()));
        }

        // Find or create workspace record first.
        let workspace = {
            let mut registry = self.lock_registry()?;
            let now = now_unix_s();
            let nela_abs_str = nela_abs.to_string_lossy().to_string();

            if let Some(index) = registry
                .workspaces
                .iter()
                .position(|ws| ws.nela_path.as_deref() == Some(nela_abs_str.as_str()))
            {
                registry.workspaces[index].last_opened_at = now;
                let active_id = registry.workspaces[index].id.clone();
                registry.active_workspace_id = Some(active_id);
                let out = registry.workspaces[index].clone();
                Self::save_registry(&self.registry_path, &registry)?;
                out
            } else {
                let id = format!("ws-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let cache_dir = self.workspaces_root.join(&id).join("cache");
                let workspace = WorkspaceRecord {
                    id: id.clone(),
                    name: name
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| {
                            nela_abs
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Imported Workspace")
                                .to_string()
                        }),
                    nela_path: Some(nela_abs_str),
                    cache_dir: cache_dir.to_string_lossy().to_string(),
                    created_at: now,
                    last_opened_at: now,
                };

                registry.active_workspace_id = Some(id);
                registry.workspaces.push(workspace.clone());
                Self::save_registry(&self.registry_path, &registry)?;
                workspace
            }
        };

        // Extract archive into workspace cache.
        let cache_dir = PathBuf::from(&workspace.cache_dir);
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)
                .map_err(|e| format!("Failed to clear workspace cache {}: {e}", cache_dir.display()))?;
        }
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create workspace cache {}: {e}", cache_dir.display()))?;

        extract_nela_archive(&nela_abs, &cache_dir)?;
        std::fs::create_dir_all(cache_dir.join("rag"))
            .map_err(|e| format!("Failed to ensure rag cache dir: {e}"))?;

        let frontend_state_path = cache_dir.join("frontend_state.json");
        let frontend_state_json = if frontend_state_path.exists() {
            Some(
                std::fs::read_to_string(&frontend_state_path)
                    .map_err(|e| format!("Failed to read extracted frontend state: {e}"))?,
            )
        } else {
            None
        };

        // Ensure active selection and timestamps are persisted.
        let reopened = self.open_workspace(&workspace.id)?;

        Ok(WorkspaceOpenResult {
            workspace: reopened,
            frontend_state_json,
        })
    }

    pub fn active_rag_dir(&self) -> Result<PathBuf, String> {
        let active = self.get_active_workspace()?;
        Ok(PathBuf::from(active.cache_dir).join("rag"))
    }

    /// Get the cache directory for a specific workspace by ID.
    pub fn get_workspace_cache_dir(&self, workspace_id: &str) -> Result<PathBuf, String> {
        let registry = self.lock_registry()?;
        let ws = registry
            .workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)
            .ok_or_else(|| format!("Workspace '{workspace_id}' not found"))?;
        Ok(PathBuf::from(&ws.cache_dir))
    }

    /// Load RAG model preferences for a workspace.
    pub fn get_rag_model_preferences(&self, workspace_id: &str) -> Result<RagModelPreferences, String> {
        let cache_dir = self.get_workspace_cache_dir(workspace_id)?;
        let prefs_path = cache_dir.join(RAG_MODEL_PREFS_FILE);
        
        if !prefs_path.exists() {
            return Ok(RagModelPreferences::default());
        }
        
        let text = std::fs::read_to_string(&prefs_path)
            .map_err(|e| format!("Failed to read RAG model preferences {}: {e}", prefs_path.display()))?;
        
        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse RAG model preferences: {e}"))
    }

    /// Save RAG model preferences for a workspace.
    pub fn save_rag_model_preferences(
        &self,
        workspace_id: &str,
        prefs: &RagModelPreferences,
    ) -> Result<(), String> {
        let cache_dir = self.get_workspace_cache_dir(workspace_id)?;
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create workspace cache dir: {e}"))?;
        
        let prefs_path = cache_dir.join(RAG_MODEL_PREFS_FILE);
        let json = serde_json::to_string_pretty(prefs)
            .map_err(|e| format!("Failed to serialize RAG model preferences: {e}"))?;
        
        std::fs::write(&prefs_path, json)
            .map_err(|e| format!("Failed to write RAG model preferences {}: {e}", prefs_path.display()))
    }

    fn load_registry(path: &Path) -> Result<WorkspaceRegistry, String> {
        if !path.exists() {
            return Ok(WorkspaceRegistry::default());
        }

        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read workspace registry {}: {e}", path.display()))?;

        let mut registry: WorkspaceRegistry =
            serde_json::from_str(&text).map_err(|e| format!("Failed to parse workspace registry JSON: {e}"))?;

        if registry.version == 0 {
            registry.version = REGISTRY_VERSION;
        }

        Ok(registry)
    }

    fn save_registry(path: &Path, registry: &WorkspaceRegistry) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("Invalid workspace registry path: {}", path.display()))?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create workspace registry dir {}: {e}", parent.display()))?;

        let json = serde_json::to_string_pretty(registry)
            .map_err(|e| format!("Failed to serialize workspace registry: {e}"))?;

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .map_err(|e| format!("Failed to write workspace registry temp file {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("Failed to replace workspace registry {}: {e}", path.display()))?;

        Ok(())
    }

    fn lock_registry(&self) -> Result<std::sync::MutexGuard<'_, WorkspaceRegistry>, String> {
        self.inner
            .lock()
            .map_err(|_| "Workspace registry lock poisoned".to_string())
    }
}

fn now_unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn add_file_to_zip(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    file_path: &Path,
    archive_path: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open file {} for zipping: {e}", file_path.display()))?;
    let mut reader = BufReader::new(file);

    zip.start_file(archive_path.replace('\\', "/"), options)
        .map_err(|e| format!("Failed to start archive file {archive_path}: {e}"))?;
    std::io::copy(&mut reader, zip)
        .map_err(|e| format!("Failed to copy file {} into archive: {e}", file_path.display()))?;

    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    dir_path: &Path,
    archive_prefix: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir_path)
        .map_err(|e| format!("Failed to read directory {}: {e}", dir_path.display()))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let archive_path = format!("{}/{}", archive_prefix.trim_end_matches('/'), name);

        if path.is_dir() {
            add_dir_to_zip(zip, &path, &archive_path, options)?;
        } else if path.is_file() {
            add_file_to_zip(zip, &path, &archive_path, options)?;
        }
    }
    Ok(())
}

fn extract_nela_archive(archive_path: &Path, target_dir: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed to open .nela archive {}: {e}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to parse .nela archive: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read .nela entry #{i}: {e}"))?;

        let name = entry.name().replace('\\', "/");
        if name.starts_with('/') || name.split('/').any(|segment| segment == "..") {
            return Err(format!("Unsafe path inside .nela archive: {name}"));
        }

        let out_path = target_dir.join(&name);

        if entry.is_dir() || name.ends_with('/') {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed to create extracted directory {}: {e}", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create extracted parent {}: {e}", parent.display()))?;
        }

        let mut out_file = File::create(&out_path)
            .map_err(|e| format!("Failed to create extracted file {}: {e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|e| format!("Failed to extract entry {name}: {e}"))?;
    }

    // Validate manifest if present.
    let manifest_path = target_dir.join("manifest.json");
    if manifest_path.exists() {
        let mut raw = String::new();
        File::open(&manifest_path)
            .map_err(|e| format!("Failed to open extracted manifest {}: {e}", manifest_path.display()))?
            .read_to_string(&mut raw)
            .map_err(|e| format!("Failed to read extracted manifest {}: {e}", manifest_path.display()))?;

        let manifest: WorkspaceManifest = serde_json::from_str(&raw)
            .map_err(|e| format!("Invalid .nela manifest format: {e}"))?;

        if manifest.schema_version != NELA_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported .nela schema version {} (expected {})",
                manifest.schema_version, NELA_SCHEMA_VERSION
            ));
        }
    }

    Ok(())
}
