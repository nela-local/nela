//! Tauri IPC commands for the RAG subsystem.

use crate::rag::pipeline::{IngestionStatus, RagPipeline, RagResult};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Tauri-managed state wrapper for the RAG pipeline.
pub struct RagPipelineState(pub Arc<RagPipeline>);

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

