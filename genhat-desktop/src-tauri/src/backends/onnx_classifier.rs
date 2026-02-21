//! In-process DistilBERT sequence-classification backend using ONNX Runtime.
//!
//! Loads `model.onnx`, `config.json`, and `tokenizer.json` from the model
//! directory and runs inference entirely on the CPU — no Python sidecar,
//! no hand-rolled layers.  The ONNX graph is the ground truth exported from
//! PyTorch, so numerical behaviour is guaranteed identical.

use crate::registry::types::{
    InMemoryHandle, ModelDef, ModelHandle, TaskRequest, TaskResponse,
};
use async_trait::async_trait;
use ort::session::Session;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokenizers::Tokenizer;

// ─────────────────────────────────────────────────────────────────────────────
// Loaded model bundle (stored inside InMemoryHandle.model via Arc)
// ─────────────────────────────────────────────────────────────────────────────

struct LoadedClassifier {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    id2label: HashMap<u32, String>,
    max_length: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Config types (parsed from the HF config.json)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HfClassifierConfig {
    #[serde(default)]
    id2label: HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend implementation
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct OnnxClassifierBackend;

impl OnnxClassifierBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl super::ModelBackend for OnnxClassifierBackend {
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        let model_path = models_dir.join(&def.model_file);
        let model_dir = model_path.parent().unwrap_or(models_dir);

        let config_path = match def.params.get("config_file") {
            Some(rel) => models_dir.join(rel),
            None => model_dir.join("config.json"),
        };
        let tokenizer_path = match def.params.get("tokenizer_file") {
            Some(rel) => models_dir.join(rel),
            None => model_dir.join("tokenizer.json"),
        };

        log::info!(
            "[OnnxClassifier] Loading model from {}",
            model_path.display()
        );

        // Parse config.json for id2label
        let config_text = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.json: {e}"))?;
        let hf_config: HfClassifierConfig = serde_json::from_str(&config_text)
            .map_err(|e| format!("Failed to parse config.json: {e}"))?;

        let id2label: HashMap<u32, String> = hf_config
            .id2label
            .into_iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|idx| (idx, v)))
            .collect();

        if id2label.is_empty() {
            return Err("config.json has no id2label mapping".into());
        }
        let num_labels = id2label.len();

        // Load tokenizer
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to load tokenizer: {e}"))?;
        tokenizer.with_padding(None);
        tokenizer
            .with_truncation(None)
            .map_err(|e| format!("Failed to disable truncation: {e}"))?;

        // Create ONNX Runtime session
        let session = Session::builder()
            .map_err(|e| format!("ORT session builder: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT intra threads: {e}"))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("ORT load model: {e}"))?;

        let max_length: usize = def
            .params
            .get("max_length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(64);

        log::info!(
            "[OnnxClassifier] Model loaded ({num_labels} labels, max_len={max_length})"
        );

        let loaded = LoadedClassifier {
            session: Mutex::new(session),
            tokenizer,
            id2label,
            max_length,
        };

        Ok(ModelHandle::InMemory(InMemoryHandle {
            model: Arc::new(loaded),
            loaded_at: Instant::now(),
        }))
    }

    async fn is_healthy(&self, handle: &ModelHandle) -> bool {
        matches!(handle, ModelHandle::InMemory(_))
    }

    async fn execute(
        &self,
        handle: &ModelHandle,
        request: &TaskRequest,
        _models_dir: &Path,
    ) -> Result<TaskResponse, String> {
        let mem = match handle {
            ModelHandle::InMemory(h) => h,
            _ => return Err("OnnxClassifier requires InMemoryHandle".into()),
        };

        let loaded = mem
            .model
            .downcast_ref::<LoadedClassifier>()
            .ok_or("Failed to downcast to LoadedClassifier")?;

        classify(&request.input, loaded)
    }

    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        log::info!("[OnnxClassifier] Stopped (memory will free on drop)");
        Ok(())
    }

    fn estimated_memory_mb(&self, def: &ModelDef) -> u32 {
        if def.memory_mb > 0 {
            def.memory_mb
        } else {
            300
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference
// ─────────────────────────────────────────────────────────────────────────────

fn classify(text: &str, loaded: &LoadedClassifier) -> Result<TaskResponse, String> {
    // 1. Tokenize
    let encoding = loaded
        .tokenizer
        .encode(text, true)
        .map_err(|e| format!("Tokenization failed: {e}"))?;

    let mut ids = encoding.get_ids().to_vec();
    if ids.len() > loaded.max_length {
        ids.truncate(loaded.max_length);
    }
    let seq_len = ids.len();

    // 2. Build input tensors as (shape, data) tuples
    let input_ids: Vec<i64> = ids.iter().map(|&id| id as i64).collect();
    let attention_mask: Vec<i64> = vec![1i64; seq_len];

    // 3. Run ONNX inference
    let input_ids_value = ort::value::Value::from_array(([1usize, seq_len], input_ids))
        .map_err(|e| format!("ORT input_ids value: {e}"))?;
    let attention_mask_value = ort::value::Value::from_array(([1usize, seq_len], attention_mask))
        .map_err(|e| format!("ORT attention_mask value: {e}"))?;

    let mut session = loaded
        .session
        .lock()
        .map_err(|e| format!("Session lock poisoned: {e}"))?;

    let outputs = session
        .run(ort::inputs![
            "input_ids" => input_ids_value,
            "attention_mask" => attention_mask_value,
        ])
        .map_err(|e| format!("ORT run: {e}"))?;

    // 4. Extract logits tensor → [1, num_labels]
    let logits_value = outputs
        .get("logits")
        .ok_or("ONNX model output missing 'logits'")?;

    let logits_tensor = logits_value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Extract logits: {e}"))?;

    // Extract the raw f32 slice (second element of the returned tuple)
    let logits: Vec<f32> = logits_tensor.1.to_vec();

    // 5. Softmax
    let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|&l| (l - max_logit).exp()).collect();
    let sum_exp: f32 = exp.iter().sum();
    let probs: Vec<f32> = exp.iter().map(|&e| e / sum_exp).collect();

    log::info!(
        "[OnnxClassifier] logits={:?}  probs={:?}",
        logits, probs
    );

    // 6. Pick best label
    let (best_idx, &best_prob) = probs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .ok_or("Empty probability vector")?;

    let raw_label = loaded
        .id2label
        .get(&(best_idx as u32))
        .cloned()
        .unwrap_or_else(|| format!("class_{best_idx}"));

    let label = normalise_label(&raw_label);

    log::info!(
        "[OnnxClassifier] '{}' → {} ({:.1}%)",
        truncate_for_log(text, 60),
        label,
        best_prob * 100.0,
    );

    Ok(TaskResponse::Classification {
        label,
        confidence: best_prob,
    })
}

fn normalise_label(raw: &str) -> String {
    match raw {
        "NoRetrieval" => "no_retrieval".to_string(),
        "SimpleRAG" => "simple_rag".to_string(),
        "MultiDoc" => "multi_doc".to_string(),
        "Summarization" => "summarization".to_string(),
        other => other.to_lowercase(),
    }
}

fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
