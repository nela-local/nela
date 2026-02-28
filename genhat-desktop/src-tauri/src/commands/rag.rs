//! Tauri IPC commands for the RAG subsystem.

use crate::rag::pipeline::{IngestionStatus, RagPipeline, RagResult};
use crate::commands::models::ProcessManagerState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Tauri-managed state wrapper for the RAG pipeline.
pub struct RagPipelineState(pub Arc<RagPipeline>);

/// Response from the streaming RAG retrieve command.
/// Frontend uses sources immediately, then streams the answer from llama-server SSE.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RagStreamSetup {
    /// Source chunks retrieved for the query.
    pub sources: Vec<crate::rag::pipeline::SourceChunk>,
    /// The prompt to send to llama-server (augmented with context, or raw query).
    pub prompt: String,
    /// The llama-server port to stream from.
    pub llama_port: u16,
    /// Whether the classifier decided no retrieval was needed.
    pub no_retrieval: bool,
}

/// Ingest a single document into the RAG knowledge base.
#[tauri::command]
pub async fn ingest_document(
    path: String,
    state: State<'_, RagPipelineState>,
) -> Result<IngestionStatus, String> {
    let pipeline = state.0.clone();
    pipeline.ingest_document(&PathBuf::from(path)).await
}

/// Ingest all supported files in a directory.
#[tauri::command]
pub async fn ingest_folder(
    path: String,
    state: State<'_, RagPipelineState>,
) -> Result<Vec<IngestionStatus>, String> {
    let pipeline = state.0.clone();
    pipeline.ingest_folder(&PathBuf::from(path)).await
}

/// Query the RAG knowledge base.
#[tauri::command]
pub async fn query_rag(
    query: String,
    top_k: Option<usize>,
    state: State<'_, RagPipelineState>,
) -> Result<RagResult, String> {
    let pipeline = state.0.clone();
    let k = top_k.unwrap_or(5);
    pipeline.query(&query, k).await
}

/// List all ingested documents.
#[tauri::command]
pub async fn list_rag_documents(
    state: State<'_, RagPipelineState>,
) -> Result<Vec<IngestionStatus>, String> {
    let pipeline = state.0.clone();
    pipeline.list_documents()
}

/// Delete a document from the knowledge base.
#[tauri::command]
pub async fn delete_rag_document(
    doc_id: i64,
    state: State<'_, RagPipelineState>,
) -> Result<(), String> {
    let pipeline = state.0.clone();
    pipeline.delete_document(doc_id).await
}

/// Manually trigger a round of background enrichment.
#[tauri::command]
pub async fn enrich_rag_documents(
    batch_size: Option<usize>,
    state: State<'_, RagPipelineState>,
) -> Result<usize, String> {
    let pipeline = state.0.clone();
    let size = batch_size.unwrap_or(10);
    pipeline.enrich_pending(size).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// RAPTOR Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a RAPTOR tree for a specific document (Phase 3).
#[tauri::command]
pub async fn build_raptor_tree(
    doc_id: i64,
    state: State<'_, RagPipelineState>,
) -> Result<crate::rag::raptor::RaptorTreeStatus, String> {
    let pipeline = state.0.clone();
    pipeline.build_raptor_tree(doc_id).await
}

/// Check if a document has a RAPTOR tree.
#[tauri::command]
pub async fn has_raptor_tree(
    doc_id: i64,
    state: State<'_, RagPipelineState>,
) -> Result<bool, String> {
    let pipeline = state.0.clone();
    pipeline.has_raptor_tree(doc_id)
}

/// Delete the RAPTOR tree for a document.
#[tauri::command]
pub async fn delete_raptor_tree(
    doc_id: i64,
    state: State<'_, RagPipelineState>,
) -> Result<(), String> {
    let pipeline = state.0.clone();
    pipeline.delete_raptor_tree(doc_id).await
}

/// Query using RAPTOR tree with confidence-aware traversal.
#[tauri::command]
pub async fn query_rag_with_raptor(
    doc_id: i64,
    query: String,
    top_k: Option<usize>,
    state: State<'_, RagPipelineState>,
) -> Result<RagResult, String> {
    let pipeline = state.0.clone();
    let k = top_k.unwrap_or(5);
    pipeline.query_with_raptor(doc_id, &query, k).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Streaming RAG Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// Retrieve RAG sources and return everything needed for frontend SSE streaming.
/// The frontend receives sources immediately and streams the LLM answer directly
/// from the llama-server SSE endpoint.
#[tauri::command]
pub async fn query_rag_stream(
    query: String,
    top_k: Option<usize>,
    rag_state: State<'_, RagPipelineState>,
    pm_state: State<'_, ProcessManagerState>,
) -> Result<RagStreamSetup, String> {
    let pipeline = rag_state.0.clone();
    let k = top_k.unwrap_or(5);

    // Phase 1: Retrieval (classify → HyDE → search → RRF → grade)
    let retrieval = pipeline.retrieve_for_query(&query, k).await?;

    // Phase 2: Get the llama-server port (ensures it's running)
    let active_id = pm_state.0.active_llm_id().await;
    let _ = pm_state.0.ensure_running(&active_id, false).await?;
    let llama_port = pm_state
        .0
        .get_llama_port(&active_id)
        .await
        .ok_or_else(|| "LLM not running or no port assigned".to_string())?;

    // Determine the prompt to stream — use raw query for no_retrieval, else augmented prompt
    let prompt = if retrieval.no_retrieval {
        query
    } else if retrieval.augmented_prompt.is_empty() {
        // No sources found — return empty result so frontend can show a message
        return Ok(RagStreamSetup {
            sources: vec![],
            prompt: String::new(),
            llama_port,
            no_retrieval: false,
        });
    } else {
        retrieval.augmented_prompt
    };

    Ok(RagStreamSetup {
        sources: retrieval.sources,
        prompt,
        llama_port,
        no_retrieval: retrieval.no_retrieval,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Media Retrieval Commands
// ═══════════════════════════════════════════════════════════════════════════════

/// Two-phase media retrieval: given the LLM's response text, find images/tables
/// whose captions are semantically similar to the response content.
/// Returns media assets that should be displayed alongside the chat answer.
#[tauri::command]
pub async fn retrieve_media_for_response(
    response_text: String,
    top_k: Option<usize>,
    threshold: Option<f32>,
    state: State<'_, RagPipelineState>,
) -> Result<Vec<crate::rag::db::MediaAssetRecord>, String> {
    let pipeline = state.0.clone();
    let k = top_k.unwrap_or(3);
    let sim_threshold = threshold.unwrap_or(0.55);
    Ok(pipeline
        .retrieve_media_for_response(&response_text, k, sim_threshold)
        .await)
}

/// Get all media assets for a specific document.
#[tauri::command]
pub async fn get_media_for_document(
    doc_id: i64,
    state: State<'_, RagPipelineState>,
) -> Result<Vec<crate::rag::db::MediaAssetRecord>, String> {
    let pipeline = state.0.clone();
    pipeline.db.get_media_for_doc(doc_id)
}

/// Streaming RAPTOR query — retrieve using RAPTOR tree and return setup for SSE streaming.
#[tauri::command]
pub async fn query_rag_with_raptor_stream(
    doc_id: i64,
    query: String,
    top_k: Option<usize>,
    rag_state: State<'_, RagPipelineState>,
    pm_state: State<'_, ProcessManagerState>,
) -> Result<RagStreamSetup, String> {
    let pipeline = rag_state.0.clone();
    let k = top_k.unwrap_or(5);

    // Phase 1: RAPTOR retrieval
    let retrieval = pipeline
        .retrieve_for_raptor_query(doc_id, &query, k)
        .await?;

    // Phase 2: Get the llama-server port
    let active_id = pm_state.0.active_llm_id().await;
    let _ = pm_state.0.ensure_running(&active_id, false).await?;
    let llama_port = pm_state
        .0
        .get_llama_port(&active_id)
        .await
        .ok_or_else(|| "LLM not running or no port assigned".to_string())?;

    let prompt = if retrieval.no_retrieval {
        query
    } else if retrieval.augmented_prompt.is_empty() {
        return Ok(RagStreamSetup {
            sources: vec![],
            prompt: String::new(),
            llama_port,
            no_retrieval: false,
        });
    } else {
        retrieval.augmented_prompt
    };

    Ok(RagStreamSetup {
        sources: retrieval.sources,
        prompt,
        llama_port,
        no_retrieval: retrieval.no_retrieval,
    })
}

/// Read a file (PDF or other) as base64 data URL for the frontend viewer.
#[tauri::command]
pub fn read_file_base64(path: String) -> Result<String, String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("File not found: {}", p.display()));
    }

    let mime = match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("md") => "text/markdown",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    };

    let data = std::fs::read(p).map_err(|e| format!("Failed to read file: {e}"))?;
    let b64 = STANDARD.encode(&data);
    Ok(format!("data:{};base64,{}", mime, b64))
}
