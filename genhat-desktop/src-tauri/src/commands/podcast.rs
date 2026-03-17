//! Tauri IPC command for podcast generation.

use crate::commands::inference::TaskRouterState;
use crate::commands::rag::RagPipelineState;
use crate::podcast::types::{PodcastRequest, PodcastResult};
use tauri::{AppHandle, State};

/// Generate a podcast from a RAG query.
///
/// Pipeline: RAG retrieval → LLM script generation → TTS per line → merge audio.
/// Emits `podcast-progress` events throughout.
#[tauri::command]
pub async fn generate_podcast(
    app: AppHandle,
    request: PodcastRequest,
    rag_state: State<'_, RagPipelineState>,
    router_state: State<'_, TaskRouterState>,
) -> Result<PodcastResult, String> {
    let rag_pipeline = rag_state.active_pipeline()?;
    let router = router_state.0.clone();

    crate::podcast::engine::generate_podcast(&app, request, rag_pipeline, router).await
}
