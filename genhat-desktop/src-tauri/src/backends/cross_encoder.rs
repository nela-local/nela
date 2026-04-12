//! In-process cross-encoder backend using ONNX Runtime for relevance scoring.
//!
//! Takes a (query, passage) pair and returns a relevance score (0-1).
//! Uses the ms-marco-MiniLM-L6-v2 cross-encoder model quantized to INT8.

use crate::registry::types::{
    InMemoryHandle, ModelDef, ModelHandle, TaskRequest, TaskResponse,
};
use async_trait::async_trait;
use ort::session::Session;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokenizers::Tokenizer;

// ─────────────────────────────────────────────────────────────────────────────
// Loaded model bundle
// ─────────────────────────────────────────────────────────────────────────────

struct LoadedCrossEncoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    max_length: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Backend implementation
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CrossEncoderBackend;

impl CrossEncoderBackend {
    pub fn new() -> Self {
        Self
    }
}

fn ort_thread_counts() -> (usize, usize) {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);

    let intra = logical.clamp(1, 8);
    let inter = if logical >= 8 { 2 } else { 1 };
    (intra, inter)
}

fn build_cross_encoder_session(model_path: &Path) -> Result<Session, String> {
    let (intra, inter) = ort_thread_counts();

    #[cfg(target_os = "macos")]
    {
        let coreml_attempt = Session::builder()
            .map_err(|e| format!("ORT session builder: {e}"))?
            .with_intra_threads(intra)
            .map_err(|e| format!("ORT intra threads: {e}"))?
            .with_inter_threads(inter)
            .map_err(|e| format!("ORT inter threads: {e}"))?
            .with_execution_providers([
                ort::ep::CoreML::default()
                    .with_subgraphs(true)
                    .with_compute_units(ort::ep::coreml::ComputeUnits::CPUAndNeuralEngine)
                    .build(),
            ]);

        match coreml_attempt {
            Ok(builder) => match builder.commit_from_file(model_path) {
                Ok(session) => {
                    log::info!(
                        "[CrossEncoder] ORT session using CoreML EP (intra={}, inter={})",
                        intra,
                        inter
                    );
                    return Ok(session);
                }
                Err(e) => {
                    log::warn!(
                        "[CrossEncoder] CoreML session commit failed, falling back to CPU: {e}"
                    );
                }
            },
            Err(e) => {
                log::warn!(
                    "[CrossEncoder] CoreML EP registration failed, falling back to CPU: {e}"
                );
            }
        }
    }

    Session::builder()
        .map_err(|e| format!("ORT session builder: {e}"))?
        .with_intra_threads(intra)
        .map_err(|e| format!("ORT intra threads: {e}"))?
        .with_inter_threads(inter)
        .map_err(|e| format!("ORT inter threads: {e}"))?
        .commit_from_file(model_path)
        .map_err(|e| format!("ORT load model: {e}"))
}

fn assert_not_lfs_pointer(model_path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(model_path)
        .map_err(|e| format!("Failed to read model metadata ({}): {e}", model_path.display()))?;

    // LFS pointer files are tiny text blobs (~100-200 bytes).
    if metadata.len() > 1024 {
        return Ok(());
    }

    let bytes = fs::read(model_path)
        .map_err(|e| format!("Failed to read model file ({}): {e}", model_path.display()))?;

    let pointer_prefix = b"version https://git-lfs.github.com/spec/v1";
    if bytes.starts_with(pointer_prefix) {
        return Err(format!(
            "Model file '{}' is a Git LFS pointer, not the real ONNX binary. \
Install git-lfs and run 'git lfs pull' in that model directory, or re-download the model artifact.",
            model_path.display()
        ));
    }

    Ok(())
}

#[async_trait]
impl super::ModelBackend for CrossEncoderBackend {
    async fn start(&self, def: &ModelDef, models_dir: &Path) -> Result<ModelHandle, String> {
        let model_path = models_dir.join(&def.model_file);
        assert_not_lfs_pointer(&model_path)?;
        let model_dir = model_path.parent().unwrap_or(models_dir);

        let tokenizer_path = match def.params.get("tokenizer_file") {
            Some(rel) => models_dir.join(rel),
            None => model_dir.join("tokenizer.json"),
        };

        log::info!(
            "[CrossEncoder] Loading model from {}",
            model_path.display()
        );

        // Load tokenizer
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to load tokenizer: {e}"))?;
        tokenizer.with_padding(None);
        tokenizer
            .with_truncation(None)
            .map_err(|e| format!("Failed to disable truncation: {e}"))?;

        // Create ONNX Runtime session with adaptive thread policy.
        let session = build_cross_encoder_session(&model_path)?;

        let max_length: usize = def
            .params
            .get("max_length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(256);

        log::info!("[CrossEncoder] Model loaded (max_len={max_length})");

        let loaded = LoadedCrossEncoder {
            session: Mutex::new(session),
            tokenizer,
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
            _ => return Err("CrossEncoder requires InMemoryHandle".into()),
        };

        let loaded = mem
            .model
            .downcast_ref::<LoadedCrossEncoder>()
            .ok_or("Failed to downcast to LoadedCrossEncoder")?;

        // Extract query and passage from request.
        // Preferred shape:
        //   extra.query = user query
        //   input       = passage/chunk text
        // Legacy fallback shape (for backward compatibility):
        //   input = "Query: ...\n\nContext: ..."
        let (query, passage) = if let Some(query) = request.extra.get("query") {
            (query.to_string(), request.input.clone())
        } else if let Some((legacy_query, legacy_passage)) = parse_legacy_grade_payload(&request.input) {
            (legacy_query, legacy_passage)
        } else {
            return Err("CrossEncoder requires 'query' in extra params".to_string());
        };

        score_pair(&query, &passage, loaded)
    }

    async fn stop(&self, _handle: &ModelHandle) -> Result<(), String> {
        log::info!("[CrossEncoder] Stopped (memory will free on drop)");
        Ok(())
    }

    fn estimated_memory_mb(&self, def: &ModelDef) -> u32 {
        if def.memory_mb > 0 {
            def.memory_mb
        } else {
            100 // INT8 quantized ms-marco-MiniLM is ~80-100MB
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference
// ─────────────────────────────────────────────────────────────────────────────

fn score_pair(query: &str, passage: &str, loaded: &LoadedCrossEncoder) -> Result<TaskResponse, String> {
    // 1. Concatenate query + [SEP] + passage
    let pair_text = format!("{} [SEP] {}", query, passage);

    // 2. Tokenize
    let encoding = loaded
        .tokenizer
        .encode(pair_text.as_str(), true)
        .map_err(|e| format!("Tokenization failed: {e}"))?;

    let mut ids = encoding.get_ids().to_vec();
    if ids.len() > loaded.max_length {
        ids.truncate(loaded.max_length);
    }
    let seq_len = ids.len();

    // 3. Build input tensors
    let input_ids: Vec<i64> = ids.iter().map(|&id| id as i64).collect();
    let attention_mask: Vec<i64> = vec![1i64; seq_len];
    let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();

    // 4. Run ONNX inference
    let input_ids_value = ort::value::Value::from_array(([1usize, seq_len], input_ids))
        .map_err(|e| format!("ORT input_ids value: {e}"))?;
    let attention_mask_value = ort::value::Value::from_array(([1usize, seq_len], attention_mask))
        .map_err(|e| format!("ORT attention_mask value: {e}"))?;
    let token_type_ids_value = ort::value::Value::from_array(([1usize, seq_len], token_type_ids))
        .map_err(|e| format!("ORT token_type_ids value: {e}"))?;

    let mut session = loaded
        .session
        .lock()
        .map_err(|e| format!("Session lock poisoned: {e}"))?;

    let outputs = session
        .run(ort::inputs![
            "input_ids" => input_ids_value,
            "attention_mask" => attention_mask_value,
            "token_type_ids" => token_type_ids_value,
        ])
        .map_err(|e| format!("ORT run: {e}"))?;

    // 5. Extract logits tensor → [1, 1] (single regression output)
    let logits_value = outputs
        .get("logits")
        .ok_or("ONNX model output missing 'logits'")?;

    let logits_tensor = logits_value
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Extract logits: {e}"))?;

    let logits: Vec<f32> = logits_tensor.1.to_vec();
    let raw_score = logits.first().copied().unwrap_or(0.0);

    // 6. Apply sigmoid to normalize to [0, 1]
    let score = 1.0 / (1.0 + (-raw_score).exp());

    log::debug!(
        "[CrossEncoder] '{}' relevance={:.3}",
        truncate_for_log(passage, 60),
        score
    );

    Ok(TaskResponse::Score(score))
}

fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn parse_legacy_grade_payload(input: &str) -> Option<(String, String)> {
    // Expected legacy format:
    // Query: <query text>
    //
    // Context: <passage text>
    const QUERY_PREFIX: &str = "Query:";
    const CONTEXT_PREFIX: &str = "Context:";

    let trimmed = input.trim();
    if !trimmed.starts_with(QUERY_PREFIX) {
        return None;
    }

    let context_idx = trimmed.find(CONTEXT_PREFIX)?;
    if context_idx <= QUERY_PREFIX.len() {
        return None;
    }

    let query_part = trimmed[QUERY_PREFIX.len()..context_idx].trim();
    let context_part = trimmed[context_idx + CONTEXT_PREFIX.len()..].trim();
    if query_part.is_empty() || context_part.is_empty() {
        return None;
    }

    Some((query_part.to_string(), context_part.to_string()))
}
