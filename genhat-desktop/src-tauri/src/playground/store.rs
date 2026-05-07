//! Playground pipeline store — persist/load pipelines as JSON files.

use super::types::Pipeline;
use std::path::PathBuf;

pub struct PipelineStore {
    dir: PathBuf,
}

impl PipelineStore {
    pub fn new(app_data_dir: &PathBuf) -> Result<Self, String> {
        let dir = app_data_dir.join("pipelines");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create pipelines dir: {}", e))?;
        Ok(Self { dir })
    }

    fn path_for(&self, id: &str) -> PathBuf {
        // Sanitize to avoid path traversal — only alphanumeric and hyphens/underscores
        let safe_id: String = id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe_id}.json"))
    }

    pub fn list(&self) -> Result<Vec<Pipeline>, String> {
        let mut pipelines = Vec::new();
        for entry in std::fs::read_dir(&self.dir)
            .map_err(|e| format!("Failed to read pipelines dir: {}", e))?
        {
            let entry = entry.map_err(|e| format!("Dir entry error: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match std::fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<Pipeline>(&json) {
                        Ok(p) => pipelines.push(p),
                        Err(e) => log::warn!("Skipping malformed pipeline {:?}: {e}", path),
                    },
                    Err(e) => log::warn!("Failed to read pipeline file {:?}: {e}", path),
                }
            }
        }
        pipelines.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(pipelines)
    }

    pub fn load(&self, id: &str) -> Result<Pipeline, String> {
        let path = self.path_for(id);
        let json = std::fs::read_to_string(&path)
            .map_err(|e| format!("Pipeline not found '{id}': {e}"))?;
        serde_json::from_str(&json)
            .map_err(|e| format!("Malformed pipeline JSON '{id}': {e}"))
    }

    pub fn save(&self, pipeline: &Pipeline) -> Result<(), String> {
        let path = self.path_for(&pipeline.id);
        let json = serde_json::to_string_pretty(pipeline)
            .map_err(|e| format!("Failed to serialize pipeline: {e}"))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write pipeline '{}': {}", path.display(), e))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path_for(id);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete pipeline '{id}': {e}"))?;
        }
        Ok(())
    }

    pub fn list_auto_resume(&self) -> Result<Vec<Pipeline>, String> {
        Ok(self.list()?.into_iter().filter(|p| p.auto_resume).collect())
    }
}
