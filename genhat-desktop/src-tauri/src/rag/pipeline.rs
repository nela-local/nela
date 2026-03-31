//! RAG Pipeline — Progressive Ingestion + Hybrid Retrieval Orchestrator.
//!
//! Three-phase ingestion:
//!   Phase 1 (instant): parse → chunk → raw-embed → store + BM25 index
//!   Phase 2 (background): enrich chunks via LLM → re-embed enriched text
//!   Phase 3 (on-demand): RAPTOR tree building (future)
//!
//! Retrieval pipeline:
//!   query → optional HyDE → BM25 search + vector search → RRF fusion
//!   → fetch chunks → optional grading → build context

use crate::rag::chunker::{chunk_text_default, chunk_text_default_meta, Chunk};
use crate::rag::db::RagDb;
use crate::rag::fusion::{rrf_fuse, FusedResult};
use crate::rag::parsers;
use crate::rag::search::BM25Index;
use crate::rag::vecindex::VectorIndex;
use crate::registry::types::TaskResponse;
use crate::router::tasks;
use crate::router::TaskRouter;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex as TokioMutex;

// ═══════════════════════════════════════════════════════════════════════════════
// Prompt Template
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the augmented RAG prompt that wraps retrieved context + user question.
///
/// Designed to work well with small (1-3B) LLMs: uses explicit language so the
/// model knows the `Document content` section IS the user's uploaded material.
fn build_rag_prompt(context: &str, user_query: &str) -> String {
    format!(
        "Use ONLY the following reference text to answer the question.\n\
         If the reference text does not contain the answer, say you don't know.\n\
         Do NOT repeat or quote the reference text verbatim.\n\
         Do NOT include any source labels, tags, or brackets in your answer.\n\
         Write a clear answer in your own words.\n\
         If the user asks for a specific format (table, list, bullet points, etc.), \n\
         use that format in your answer.\n\n\
         Reference text:\n\
         {context}\n\n\
         Question: {user_query}\n\n\
         Answer:"
    )
}

/// Convert internal metadata tags like "page:3" or "slide:2" to a
/// human-readable label such as "Page 3" or "Slide 2".
/// Returns an empty string for unrecognized or empty metadata.
#[allow(dead_code)]
fn format_page_label(metadata: &str) -> String {
    if metadata.is_empty() {
        return String::new();
    }
    if let Some(num) = metadata.strip_prefix("page:") {
        // e.g. "page:3" → "Page 3"
        let num = num.split(':').next().unwrap_or(num);
        return format!("Page {num}");
    }
    if let Some(rest) = metadata.strip_prefix("slide:") {
        // e.g. "slide:2" or "slide:2:table:1"
        let num = rest.split(':').next().unwrap_or(rest);
        return format!("Slide {num}");
    }
    if let Some(num) = metadata.strip_prefix("paragraph:") {
        return format!("Paragraph {num}");
    }
    // Fallback: return as-is if it contains useful info
    metadata.to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a RAG query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagResult {
    /// The generated answer text.
    pub answer: String,
    /// Source chunks used to generate the answer.
    pub sources: Vec<SourceChunk>,
}

/// Result of the retrieval phase (sources + prompt, no LLM answer yet).
/// Used by streaming commands — frontend streams the answer directly from llama-server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    /// Source chunks retrieved for the query.
    pub sources: Vec<SourceChunk>,
    /// The augmented prompt to send to the LLM (includes context + question).
    /// Empty if no_retrieval — in that case send the raw user query.
    pub augmented_prompt: String,
    /// Whether the classifier decided no retrieval was needed.
    pub no_retrieval: bool,
}

/// A source chunk with provenance info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceChunk {
    pub chunk_id: i64,
    pub doc_title: String,
    pub text: String,
    pub score: f64,
    /// Page/slide/section provenance from the original document (e.g. "page:3", "slide:2").
    #[serde(default)]
    pub page_info: String,
}

/// Status of document ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionStatus {
    pub doc_id: i64,
    pub title: String,
    pub file_path: String,
    pub total_chunks: usize,
    pub embedded_chunks: usize,
    pub enriched_chunks: usize,
    pub phase: String,
}

/// The RAG pipeline engine.
pub struct RagPipeline {
    pub db: Arc<RagDb>,
    pub bm25: Arc<BM25Index>,
    pub vec_index: Arc<VectorIndex>,
    router: Arc<TaskRouter>,
    /// Lock to serialize ingestion (prevent concurrent writes to tantivy writer).
    ingest_lock: TokioMutex<()>,
    /// Cancellation flag for the enrichment worker. Set to true to stop the worker.
    enrichment_cancelled: Arc<AtomicBool>,
}

impl std::fmt::Debug for RagPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RagPipeline").finish()
    }
}

impl RagPipeline {
    /// Open the RAG pipeline, creating storage if needed.
    pub fn open(data_dir: &Path, router: Arc<TaskRouter>) -> Result<Self, String> {
        let db_path = data_dir.join("rag.db");
        let index_dir = data_dir.join("bm25_index");

        let db = Arc::new(RagDb::open(&db_path)?);
        let bm25 = Arc::new(BM25Index::open(&index_dir)?);

        // Load all embeddings into in-memory IVF vector index
        let vec_index = Arc::new(VectorIndex::load_from_db(&db)?);
        log::info!(
            "VectorIndex loaded: {} vectors in memory (~{:.2} MB quantized)",
            vec_index.len(),
            vec_index.estimated_memory_bytes() as f64 / (1024.0 * 1024.0)
        );

        Ok(Self {
            db,
            bm25,
            vec_index,
            router,
            ingest_lock: TokioMutex::new(()),
            enrichment_cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Phase 1: Instant Ingestion
    // ═══════════════════════════════════════════════════════════════════════

    /// Ingest a single document: parse → chunk → embed → store + index.
    /// Also extracts and stores media assets (images, tables) as PNG files.
    pub async fn ingest_document(&self, file_path: &Path) -> Result<IngestionStatus, String> {
        let _lock = self.ingest_lock.lock().await;

        let path_str = file_path
            .to_str()
            .ok_or("Invalid file path encoding")?
            .to_string();

        // Check for duplicates
        if self.db.document_exists(&path_str)? {
            return Err(format!("Document already ingested: {}", path_str));
        }

        // Prepare media output directory inside the data dir
        // (next to rag.db, under a "media/" subfolder)
        let media_dir = self.db.path.parent()
            .map(|p| p.join("media"))
            .unwrap_or_else(|| std::path::PathBuf::from("media"));

        // 1. Parse document (with media extraction)
        let mut parsed = parsers::parse_document_with_media(file_path, Some(&media_dir))?;
        let title = parsed.title.clone();
        
        log::info!(
            "Parsed document '{}' into {} sections + {} media elements",
            title,
            parsed.sections.len(),
            parsed.media_elements().len()
        );
        for (i, sec) in parsed.sections.iter().take(3).enumerate() {
            log::debug!("  Section {}: {} chars", i + 1, sec.text.len());
        }
        
        let doc_type = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        // 1b. If audio file with pending transcription, run Whisper STT
        let has_pending_audio = parsed.sections.iter().any(|s| s.metadata == "audio:pending");
        if has_pending_audio {
            log::info!("Audio file detected, attempting transcription: {}", path_str);
            let transcribe_req = tasks::transcribe_request(&path_str);
            match self.router.route(&transcribe_req).await {
                Ok(TaskResponse::Transcription { segments }) => {
                    log::info!("Transcribed {} segments from audio", segments.len());
                    let segment_tuples: Vec<(String, u64, u64)> = segments
                        .iter()
                        .map(|s| (s.text.clone(), s.start_ms, s.end_ms))
                        .collect();
                    parsed = crate::rag::parsers::audio::from_transcription(
                        &title,
                        &segment_tuples,
                    );
                }
                Ok(TaskResponse::Text(text)) => {
                    log::info!("Got text transcription for audio");
                    parsed = crate::rag::parsers::audio::from_transcription(
                        &title,
                        &[(text, 0, 0)],
                    );
                }
                Ok(_) => {
                    log::warn!("Transcription returned unexpected response type, keeping placeholder");
                }
                Err(e) => {
                    log::warn!("Audio transcription failed: {e}. Ingesting with placeholder text.");
                }
            }
        }

        // 2. Chunk the sections (carry metadata like "page:3" or "slide:2")
        let mut all_chunks: Vec<Chunk> = Vec::new();
        for section in &parsed.sections {
            if section.metadata.is_empty() {
                let section_chunks = chunk_text_default(&section.text);
                all_chunks.extend(section_chunks);
            } else {
                let section_chunks = chunk_text_default_meta(&section.text, &section.metadata);
                all_chunks.extend(section_chunks);
            }
        }

        // Re-index chunks sequentially
        let all_chunks: Vec<Chunk> = all_chunks
            .into_iter()
            .enumerate()
            .map(|(i, mut c)| {
                c.index = i;
                c
            })
            .collect();

        if all_chunks.is_empty() {
            return Err("Document produced no chunks after parsing".into());
        }

        let chunk_count = all_chunks.len();
        log::info!("Created {} chunks from document '{}'", chunk_count, title);
        for (i, chunk) in all_chunks.iter().take(3).enumerate() {
            log::debug!("  Chunk {}: {} chars, preview: {}", 
                i + 1, 
                chunk.text.len(),
                chunk.text.chars().take(80).collect::<String>()
            );
        }

        // 3. Insert document record
        let doc_id = self.db.insert_document(
            &path_str,
            &title,
            &doc_type,
            chunk_count as i64,
        )?;

        // 4. Insert chunk records (text + metadata, no embeddings yet)
        let chunk_texts: Vec<(usize, String, String)> = all_chunks
            .iter()
            .map(|c| (c.index, c.text.clone(), c.metadata.clone()))
            .collect();
        let chunk_ids = self.db.insert_chunks(doc_id, &chunk_texts)?;

        // 5. Index chunks in BM25
        let bm25_batch: Vec<(i64, String, String)> = chunk_ids
            .iter()
            .zip(all_chunks.iter())
            .map(|(&id, chunk)| (id, chunk.text.clone(), title.clone()))
            .collect();
        self.bm25.add_chunks_batch(&bm25_batch)?;

        // 6. Embed chunks via TaskRouter
        let embedded_count = self.embed_chunks(&chunk_ids, &all_chunks).await;
        log::info!(
            "Ingestion summary for '{}': {} chunks, {} embedded, vec_index now has {} vectors",
            title, chunk_count, embedded_count, self.vec_index.len()
        );

        // 7. Store media assets and embed their captions
        let media_count = self.store_media_assets(doc_id, &parsed).await;
        if media_count > 0 {
            log::info!("Stored {media_count} media assets for document '{title}'");
        }

        Ok(IngestionStatus {
            doc_id,
            title,
            file_path: path_str,
            total_chunks: chunk_count,
            embedded_chunks: embedded_count,
            enriched_chunks: 0,
            phase: "phase1_complete".to_string(),
        })
    }

    /// Store extracted media elements (images/tables) in the DB and embed their captions.
    async fn store_media_assets(
        &self,
        doc_id: i64,
        parsed: &parsers::ParsedDocument,
    ) -> usize {
        let media_elements = parsed.media_elements();
        if media_elements.is_empty() {
            return 0;
        }

        let mut stored = 0usize;
        let mut captions_to_embed: Vec<(i64, String)> = Vec::new();

        for elem in &media_elements {
            let asset_type = match elem.kind {
                parsers::ElementKind::Image => "image",
                parsers::ElementKind::Table => "table",
                _ => continue,
            };

            let file_path = match &elem.media_path {
                Some(p) => p.to_string_lossy().to_string(),
                None => continue,
            };

            match self.db.insert_media_asset(
                doc_id,
                asset_type,
                &file_path,
                &elem.text,
                &elem.metadata,
            ) {
                Ok(asset_id) => {
                    stored += 1;
                    // Queue caption for embedding
                    if !elem.text.is_empty() {
                        captions_to_embed.push((asset_id, elem.text.clone()));
                    }
                }
                Err(e) => {
                    log::warn!("Failed to store media asset: {e}");
                }
            }
        }

        // Batch embed all captions — retry up to 3 times to handle
        // the embedding model not being loaded yet at ingestion time.
        if !captions_to_embed.is_empty() {
            let texts: Vec<String> = captions_to_embed.iter().map(|(_, t)| t.clone()).collect();
            let n_captions = texts.len();
            let mut embedded_count = 0usize;

            for attempt in 1..=3 {
                let request = tasks::embed_request(texts.clone());
                match self.router.route(&request).await {
                    Ok(TaskResponse::Embeddings(vectors)) => {
                        for (i, embedding) in vectors.iter().enumerate() {
                            if i < captions_to_embed.len() {
                                let asset_id = captions_to_embed[i].0;
                                if let Ok(()) = self.db.set_media_embedding(asset_id, embedding) {
                                    self.vec_index.insert(-asset_id, embedding.clone());
                                    embedded_count += 1;
                                }
                            }
                        }
                        log::info!(
                            "Embedded {embedded_count}/{n_captions} media captions (attempt {attempt})"
                        );
                        break; // success
                    }
                    Ok(_) => {
                        log::warn!("Caption embedding returned unexpected response type (attempt {attempt})");
                        break; // non-retryable
                    }
                    Err(e) => {
                        log::warn!(
                            "Caption embedding failed (attempt {attempt}/3): {e}"
                        );
                        if attempt < 3 {
                            // Wait before retry — gives the embed model time to load
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        } else {
                            log::error!(
                                "❌ Caption embedding failed after 3 attempts — \
                                 {n_captions} media assets stored WITHOUT embeddings. \
                                 They will be invisible to media retrieval. \
                                 Use re_embed_unembedded_media to fix later."
                            );
                        }
                    }
                }
            }
        }

        stored
    }

    /// Embed a batch of chunks and store embeddings in the DB.
    /// Returns the number of successfully embedded chunks.
    async fn embed_chunks(&self, chunk_ids: &[i64], chunks: &[Chunk]) -> usize {
        // Batch embeddings (submit all texts at once)
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        log::info!("Embedding {} chunks (vec_index has {} vectors before)",
                   texts.len(), self.vec_index.len());

        let request = tasks::embed_request(texts);
        match self.router.route(&request).await {
            Ok(TaskResponse::Embeddings(vectors)) => {
                let mut count = 0;
                for (i, embedding) in vectors.iter().enumerate() {
                    if i < chunk_ids.len() {
                        if let Ok(()) =
                            self.db.set_chunk_embedding(chunk_ids[i], embedding, None)
                        {
                            // Also add to in-memory vector index
                            self.vec_index.insert(chunk_ids[i], embedding.clone());
                            count += 1;
                        }
                    }
                }
                log::info!("Successfully embedded {}/{} chunks (vec_index now has {} vectors)",
                           count, chunk_ids.len(), self.vec_index.len());
                count
            }
            Ok(other) => {
                log::warn!("Embed task returned unexpected response: {:?}", other);
                0
            }
            Err(e) => {
                log::warn!("⚠ Embedding FAILED (model may not be loaded): {e} — \
                            retrieval will fall back to BM25 only");
                0
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Phase 2: Background Enrichment
    // ═══════════════════════════════════════════════════════════════════════

    /// Enrich unenriched chunks via LLM (background task).
    /// Returns the number of chunks enriched this round.
    pub async fn enrich_pending(&self, batch_size: usize) -> Result<usize, String> {
        let unenriched = self.db.unenriched_chunk_ids(batch_size)?;
        if unenriched.is_empty() {
            return Ok(0);
        }

        log::debug!("Enrichment: Processing {} unenriched chunks", unenriched.len());
        let chunk_records = self.db.get_chunks_by_ids(&unenriched)?;
        let mut enriched = 0;

        for chunk in &chunk_records {
            // Ask LLM to generate contextual summary
            let request = tasks::enrich_request(&chunk.text);
            match self.router.route(&request).await {
                Ok(TaskResponse::Text(enriched_text)) => {
                    // Re-embed the enriched text
                    let embed_req =
                        tasks::embed_request(vec![enriched_text.clone()]);
                    let enriched_emb = match self.router.route(&embed_req).await {
                        Ok(TaskResponse::Embeddings(vecs)) => vecs.into_iter().next(),
                        _ => None,
                    };

                    if let Err(e) = self.db.set_chunk_enrichment(
                        chunk.id,
                        &enriched_text,
                        enriched_emb.as_ref(),
                    ) {
                        log::warn!("Failed to store enrichment for chunk {}: {e}", chunk.id);
                    } else {
                        enriched += 1;
                        // Update in-memory vector index with enriched embedding
                        if let Some(ref emb) = enriched_emb {
                            self.vec_index.insert(chunk.id, emb.clone());
                        }
                    }
                }
                Ok(_) => {
                    log::warn!("Enrich returned non-text for chunk {}", chunk.id);
                }
                Err(e) => {
                    log::warn!("Enrichment failed for chunk {}: {e}", chunk.id);
                    log::info!("Enrichment model unavailable, skipping remaining chunks");
                    break; // Model probably not available
                }
            }
        }

        log::info!("Enrichment round complete: {}/{} chunks enriched", enriched, chunk_records.len());
        Ok(enriched)
    }

    /// Spawn a background enrichment worker that processes chunks gradually.
    /// Emits `rag:enrichment_progress` events to the frontend.
    /// When enrichment is complete, automatically builds RAPTOR trees for
    /// fully-enriched documents (lazy RAPTOR) and rebuilds the vector index.
    /// 
    /// The worker will stop when `stop_enrichment()` is called on this pipeline.
    pub fn start_enrichment_worker(pipeline: Arc<Self>, app_handle: tauri::AppHandle) {
        let cancel_flag = pipeline.enrichment_cancelled.clone();
        tauri::async_runtime::spawn(async move {
            log::info!("RAG enrichment worker started");
            let mut idle_cycles: u32 = 0;
            let mut failed_cycles: u32 = 0;
            loop {
                // Check cancellation before each cycle
                if cancel_flag.load(Ordering::Relaxed) {
                    log::info!("RAG enrichment worker cancelled, exiting");
                    break;
                }

                match pipeline.enrich_pending(5).await {
                    Ok(0) => {
                        idle_cycles += 1;
                        log::info!("RAG enrichment idle (cycle {})", idle_cycles);

                        // After 2 idle cycles (~60s), try building RAPTOR trees
                        if idle_cycles == 2 {
                            log::info!("RAG enrichment complete, triggering RAPTOR auto-build");
                            pipeline.auto_build_raptor_trees(&app_handle).await;
                            // Reset after building to avoid re-triggering on next cycle
                            idle_cycles = 0;
                        }

                        // Periodically rebuild vector index partitions during idle time
                        pipeline.vec_index.rebuild_if_needed();

                        // Nothing to enrich — wait longer, but check cancellation periodically
                        for _ in 0..6 {
                            if cancel_flag.load(Ordering::Relaxed) {
                                log::info!("RAG enrichment worker cancelled during idle, exiting");
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                    Ok(n) => {
                        idle_cycles = 0;
                        failed_cycles = 0;
                        log::info!("Enriched {n} chunks, continuing...");
                        // Emit progress event to frontend
                        let _ = app_handle.emit("rag:enrichment_progress", serde_json::json!({
                            "enriched_this_round": n,
                            "status": "in_progress"
                        }));
                        // Small delay to avoid hogging the model
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        idle_cycles = 0;
                        failed_cycles += 1;
                        log::warn!("Enrichment error (cycle {}): {}", failed_cycles, e);
                        
                        // After 3 consecutive failures (~3 mins), give up on enrichment
                        // and try to build RAPTOR trees anyway from existing chunks
                        if failed_cycles >= 3 {
                            log::warn!("Enrichment persistently failing, triggering RAPTOR anyway");
                            pipeline.auto_build_raptor_trees(&app_handle).await;
                            failed_cycles = 0; // Reset to avoid re-triggering
                        }
                        
                        let _ = app_handle.emit("rag:enrichment_progress", serde_json::json!({
                            "enriched_this_round": 0,
                            "status": "error",
                            "error": e.to_string()
                        }));
                        
                        // Wait but check cancellation periodically
                        for _ in 0..12 {
                            if cancel_flag.load(Ordering::Relaxed) {
                                log::info!("RAG enrichment worker cancelled during error backoff, exiting");
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        });
    }

    /// Signal the enrichment worker to stop. The worker will exit on its next cycle.
    pub fn stop_enrichment(&self) {
        log::info!("Stopping RAG enrichment worker");
        self.enrichment_cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if the enrichment worker has been cancelled.
    pub fn is_enrichment_cancelled(&self) -> bool {
        self.enrichment_cancelled.load(Ordering::Relaxed)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Context Window Expansion
    // ═══════════════════════════════════════════════════════════════════════

    /// Build an augmented context string for the LLM by expanding each selected
    /// source chunk with its immediate predecessor and successor in the document.
    ///
    /// This dramatically improves answer quality for questions like
    /// "What are the objectives?" where the heading lands in one chunk
    /// and the body text in the next.
    ///
    /// `fetched_chunks` must be the full `Vec<ChunkRecord>` already fetched
    /// during the retrieval step (contains doc_id + chunk_index for each chunk).
    fn build_expanded_context(
        &self,
        sources: &[SourceChunk],
        fetched_chunks: &[crate::rag::db::ChunkRecord],
    ) -> String {
        // Collect (doc_id, chunk_index) for every selected source
        let refs: Vec<(i64, i32)> = sources
            .iter()
            .filter_map(|s| {
                fetched_chunks
                    .iter()
                    .find(|c| c.id == s.chunk_id)
                    .map(|c| (c.doc_id, c.chunk_index))
            })
            .collect();

        // Fetch prev/next neighbors in one DB call
        let neighbors = self.db.get_adjacent_chunks(&refs).unwrap_or_default();

        // IDs of chunks that are already selected sources (don't double-print)
        let selected_ids: std::collections::HashSet<i64> =
            sources.iter().map(|s| s.chunk_id).collect();

        sources
            .iter()
            .map(|s| {
                // Find the meta for this source
                let meta = fetched_chunks.iter().find(|c| c.id == s.chunk_id);

                let body = if let Some(c) = meta {
                    let mut parts: Vec<&str> = Vec::new();

                    // Previous chunk (if not already a selected source)
                    if let Some(prev) = neighbors
                        .iter()
                        .find(|n| n.doc_id == c.doc_id && n.chunk_index == c.chunk_index - 1)
                    {
                        if !selected_ids.contains(&prev.id) {
                            parts.push(prev.text.as_str());
                        }
                    }

                    parts.push(s.text.as_str());

                    // Next chunk (if not already a selected source)
                    if let Some(next) = neighbors
                        .iter()
                        .find(|n| n.doc_id == c.doc_id && n.chunk_index == c.chunk_index + 1)
                    {
                        if !selected_ids.contains(&next.id) {
                            parts.push(next.text.as_str());
                        }
                    }

                    parts.join("\n")
                } else {
                    s.text.clone()
                };

                body
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Retrieval Pipeline
    // ═══════════════════════════════════════════════════════════════════════

    /// Full RAG query: classify → HyDE → hybrid search → RRF → context build → generate.
    pub async fn query(&self, user_query: &str, top_k: usize) -> Result<RagResult, String> {
        let doc_count = self.db.document_count().unwrap_or(0);

        // If no documents are ingested, skip classifier entirely — always no_retrieval
        let skip_retrieval = if doc_count == 0 {
            log::info!("No documents in DB — skipping classifier, using no_retrieval");
            true
        } else {
            // Classify and skip retrieval only if no_retrieval confidence > 90%
            let (query_class, confidence) = self.classify_query(user_query).await;
            log::info!(
                "Query classified as: {} ({:.1}%) (docs in DB: {}, vec_index: {} vectors)",
                query_class, confidence * 100.0, doc_count, self.vec_index.len()
            );
            query_class == "no_retrieval" && confidence > 0.9
        };

        if skip_retrieval {
            let chat_request = tasks::chat_request(user_query);
            let answer = match self.router.route(&chat_request).await {
                Ok(TaskResponse::Text(text)) => text,
                Ok(_) => "Failed to generate answer: unexpected response type".to_string(),
                Err(e) => format!("Failed to generate answer: {e}"),
            };
            return Ok(RagResult {
                answer,
                sources: vec![],
            });
        }

        // 1. Optional HyDE: generate a hypothetical answer to improve retrieval
        let search_query = match self.try_hyde(user_query).await {
            Some(hyde_text) => hyde_text,
            None => user_query.to_string(),
        };

        // 2. BM25 keyword search
        let bm25_results = self.bm25.search(user_query, top_k)?;
        log::info!("BM25 returned {} results", bm25_results.len());
        for (id, score) in bm25_results.iter().take(3) {
            log::debug!("  BM25 chunk_id={} score={:.4}", id, score);
        }

        // 3. Vector similarity search (using the potentially HyDE-enhanced query)
        let vector_results = self.vector_search(&search_query, top_k).await?;
        log::info!("Vector search returned {} results", vector_results.len());
        for (id, score) in vector_results.iter().take(3) {
            log::debug!("  Vector chunk_id={} score={:.4}", id, score);
        }

        // 4. RRF fusion
        let fused = rrf_fuse(&[bm25_results, vector_results]);
        log::info!("RRF fused into {} results", fused.len());

        // 5. Take top-k fused results (fetch extra for grading headroom)
        let grading_pool = top_k * 2; // fetch 2x candidates, grade down to top_k
        let top_fused: Vec<&FusedResult> = fused.iter().take(grading_pool).collect();
        let chunk_ids: Vec<i64> = top_fused.iter().map(|r| r.chunk_id).collect();

        if chunk_ids.is_empty() {
            return Ok(RagResult {
                answer: "No relevant documents found. Please ingest some documents first."
                    .to_string(),
                sources: vec![],
            });
        }

        // 6. Fetch chunk texts
        let chunks = self.db.get_chunks_by_ids(&chunk_ids)?;

        // 7. Grade chunks for relevance (filter out irrelevant ones)
        let mut graded_sources: Vec<(SourceChunk, u8)> = Vec::new();
        for fused_result in &top_fused {
            if let Some(chunk) = chunks.iter().find(|c| c.id == fused_result.chunk_id) {
                let doc_title = self
                    .db
                    .doc_title_for_chunk(chunk.id)
                    .unwrap_or_else(|_| "Unknown".to_string());

                // Ask LLM to grade relevance (1-5). Falls back to 3 if grading unavailable.
                let grade = self.grade_chunk(user_query, &chunk.text).await;
                log::info!(
                    "  Graded chunk_id={} '{}' → grade={} (rrf_score={:.4})",
                    chunk.id,
                    chunk.text.chars().take(60).collect::<String>(),
                    grade,
                    fused_result.rrf_score
                );

                graded_sources.push((
                    SourceChunk {
                        chunk_id: chunk.id,
                        doc_title,
                        text: chunk.text.clone(),
                        score: fused_result.rrf_score,
                        page_info: chunk.metadata.clone(),
                    },
                    grade,
                ));
            }
        }

        let before_filter = graded_sources.len();
        // Filter: keep only chunks with grade >= 3, then take top_k
        graded_sources.retain(|(_, grade)| *grade >= 3);
        log::info!(
            "Grading kept {}/{} chunks (grade >= 3)",
            graded_sources.len(),
            before_filter
        );
        graded_sources.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.score.partial_cmp(&a.0.score).unwrap_or(std::cmp::Ordering::Equal)));
        graded_sources.truncate(top_k);

        let sources: Vec<SourceChunk> = graded_sources.into_iter().map(|(s, _)| s).collect();

        // RAG Fusion: if all chunks failed grading, rephrase and retry once
        if sources.is_empty() {
            log::info!("All chunks failed grading — attempting RAG Fusion rephrase");
            let rephrased = self.generate_rephrasings(user_query).await;

            let mut retry_sources = Vec::new();
            let mut retry_chunks: Vec<crate::rag::db::ChunkRecord> = Vec::new();
            for variant in &rephrased {
                let bm25_r = self.bm25.search(variant, top_k).unwrap_or_default();
                let vec_r = self.vector_search(variant, top_k).await.unwrap_or_default();
                let fused_r = rrf_fuse(&[bm25_r, vec_r]);

                let ids: Vec<i64> = fused_r.iter().take(top_k).map(|r| r.chunk_id).collect();
                if let Ok(fetched) = self.db.get_chunks_by_ids(&ids) {
                    for (fused_result, chunk) in fused_r.iter().zip(fetched.iter()) {
                        let doc_title = self
                            .db
                            .doc_title_for_chunk(chunk.id)
                            .unwrap_or_else(|_| "Unknown".to_string());
                        retry_sources.push(SourceChunk {
                            chunk_id: chunk.id,
                            doc_title,
                            text: chunk.text.clone(),
                            score: fused_result.rrf_score,
                            page_info: chunk.metadata.clone(),
                        });
                    }
                    retry_chunks.extend(fetched);
                }
            }

            // Deduplicate by chunk_id, keep highest score
            retry_sources.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            retry_sources.dedup_by_key(|s| s.chunk_id);
            retry_sources.truncate(top_k);

            if retry_sources.is_empty() {
                return Ok(RagResult {
                    answer: "Retrieved documents were not relevant to your question. Try rephrasing or ingesting more documents."
                        .to_string(),
                    sources: vec![],
                });
            }

            // Use retry_sources for answer generation (with context expansion)
            let context = self.build_expanded_context(&retry_sources, &retry_chunks);
            let max_context_chars: usize = 65536;
            let context = if context.len() > max_context_chars {
                log::warn!("RAG retry context truncated from {} to {} chars", context.len(), max_context_chars);
                context[..max_context_chars].to_string()
            } else {
                context
            };

            let augmented_prompt = build_rag_prompt(&context, user_query);

            let chat_request = tasks::chat_request(&augmented_prompt);
            let answer = match self.router.route(&chat_request).await {
                Ok(TaskResponse::Text(text)) => text,
                Ok(_) => "Failed to generate answer: unexpected response type".to_string(),
                Err(e) => format!("Failed to generate answer: {e}"),
            };

            return Ok(RagResult {
                answer,
                sources: retry_sources,
            });
        }

        // 8. Build augmented prompt with context window expansion
        let context = self.build_expanded_context(&sources, &chunks);
        log::info!(
            "RAG context: {} chars, {} sources (first 200: '{}')",
            context.len(),
            sources.len(),
            context.chars().take(200).collect::<String>()
        );
        let max_context_chars: usize = 65536;
        let context = if context.len() > max_context_chars {
            log::warn!("RAG context truncated from {} to {} chars", context.len(), max_context_chars);
            context[..max_context_chars].to_string()
        } else {
            context
        };

        let augmented_prompt = build_rag_prompt(&context, user_query);

        // 9. Generate answer via LLM
        let chat_request = tasks::chat_request(&augmented_prompt);
        let answer = match self.router.route(&chat_request).await {
            Ok(TaskResponse::Text(text)) => text,
            Ok(_) => "Failed to generate answer: unexpected response type".to_string(),
            Err(e) => format!("Failed to generate answer: {e}"),
        };

        Ok(RagResult { answer, sources })
    }

    /// Perform the full retrieval pipeline without generating an answer.
    /// Returns sources and the augmented prompt for streaming by the frontend.
    pub async fn retrieve_for_query(
        &self,
        user_query: &str,
        top_k: usize,
    ) -> Result<RetrievalResult, String> {
        let doc_count = self.db.document_count().unwrap_or(0);

        // If no documents are ingested, skip classifier entirely — always no_retrieval
        let skip_retrieval = if doc_count == 0 {
            log::info!("[stream] No documents in DB — skipping classifier, using no_retrieval");
            true
        } else {
            let (query_class, confidence) = self.classify_query(user_query).await;
            log::info!(
                "[stream] Query classified as: {} ({:.1}%) (docs={}, vec={})",
                query_class, confidence * 100.0, doc_count, self.vec_index.len()
            );
            query_class == "no_retrieval" && confidence > 0.9
        };

        if skip_retrieval {
            return Ok(RetrievalResult {
                sources: vec![],
                augmented_prompt: String::new(),
                no_retrieval: true,
            });
        }

        // 1. Optional HyDE
        let search_query = match self.try_hyde(user_query).await {
            Some(hyde_text) => hyde_text,
            None => user_query.to_string(),
        };

        // 2. BM25 keyword search
        let bm25_results = self.bm25.search(user_query, top_k)?;
        log::info!("[stream] BM25 returned {} results", bm25_results.len());

        // 3. Vector similarity search
        let vector_results = self.vector_search(&search_query, top_k).await?;
        log::info!("[stream] Vector search returned {} results", vector_results.len());

        // 4. RRF fusion
        let fused = rrf_fuse(&[bm25_results, vector_results]);
        log::info!("[stream] RRF fused into {} results", fused.len());

        // 5. Take top-k fused results (fetch extra for grading headroom)
        let grading_pool = top_k * 2;
        let top_fused: Vec<&FusedResult> = fused.iter().take(grading_pool).collect();
        let chunk_ids: Vec<i64> = top_fused.iter().map(|r| r.chunk_id).collect();

        if chunk_ids.is_empty() {
            return Ok(RetrievalResult {
                sources: vec![],
                augmented_prompt: String::new(),
                no_retrieval: false,
            });
        }

        // 6. Fetch chunk texts
        let mut chunks = self.db.get_chunks_by_ids(&chunk_ids)?;

        // 7. Grade chunks for relevance
        let mut graded_sources: Vec<(SourceChunk, u8)> = Vec::new();
        for fused_result in &top_fused {
            if let Some(chunk) = chunks.iter().find(|c| c.id == fused_result.chunk_id) {
                let doc_title = self
                    .db
                    .doc_title_for_chunk(chunk.id)
                    .unwrap_or_else(|_| "Unknown".to_string());
                let grade = self.grade_chunk(user_query, &chunk.text).await;
                log::info!(
                    "[stream] Graded chunk_id={} grade={} (rrf={:.4})",
                    chunk.id, grade, fused_result.rrf_score
                );
                graded_sources.push((
                    SourceChunk {
                        chunk_id: chunk.id,
                        doc_title,
                        text: chunk.text.clone(),
                        score: fused_result.rrf_score,
                        page_info: chunk.metadata.clone(),
                    },
                    grade,
                ));
            }
        }

        let before_filter = graded_sources.len();
        graded_sources.retain(|(_, grade)| *grade >= 3);
        log::info!(
            "[stream] Grading kept {}/{} chunks",
            graded_sources.len(), before_filter
        );
        graded_sources.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(b.0.score.partial_cmp(&a.0.score).unwrap_or(std::cmp::Ordering::Equal))
        });
        graded_sources.truncate(top_k);

        let mut sources: Vec<SourceChunk> = graded_sources.into_iter().map(|(s, _)| s).collect();

        // RAG Fusion: if all chunks failed grading, rephrase and retry once
        if sources.is_empty() {
            log::info!("All chunks failed grading — attempting RAG Fusion rephrase (streaming)");
            let rephrased = self.generate_rephrasings(user_query).await;
            let mut retry_sources = Vec::new();
            let mut retry_chunks: Vec<crate::rag::db::ChunkRecord> = Vec::new();
            for variant in &rephrased {
                let bm25_r = self.bm25.search(variant, top_k).unwrap_or_default();
                let vec_r = self.vector_search(variant, top_k).await.unwrap_or_default();
                let fused_r = rrf_fuse(&[bm25_r, vec_r]);
                let ids: Vec<i64> = fused_r.iter().take(top_k).map(|r| r.chunk_id).collect();
                if let Ok(fetched) = self.db.get_chunks_by_ids(&ids) {
                    for (fused_result, chunk) in fused_r.iter().zip(fetched.iter()) {
                        let doc_title = self
                            .db
                            .doc_title_for_chunk(chunk.id)
                            .unwrap_or_else(|_| "Unknown".to_string());
                        retry_sources.push(SourceChunk {
                            chunk_id: chunk.id,
                            doc_title,
                            text: chunk.text.clone(),
                            score: fused_result.rrf_score,
                            page_info: chunk.metadata.clone(),
                        });
                    }
                    retry_chunks.extend(fetched);
                }
            }
            retry_sources.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            retry_sources.dedup_by_key(|s| s.chunk_id);
            retry_sources.truncate(top_k);
            // Swap chunk metadata so context expansion works on retry sources
            chunks = retry_chunks;
            sources = retry_sources;
        }

        if sources.is_empty() {
            return Ok(RetrievalResult {
                sources: vec![],
                augmented_prompt: String::new(),
                no_retrieval: false,
            });
        }

        // Build augmented prompt with context window expansion
        let context = self.build_expanded_context(&sources, &chunks);
        log::info!(
            "[stream] RAG context: {} chars, {} sources (first 200: '{}')",
            context.len(),
            sources.len(),
            context.chars().take(200).collect::<String>()
        );

        // Truncate context to ~65536 chars (~16384 tokens) to stay within
        // the model's context window after adding system/user framing and
        // leaving room for the max_tokens output.
        let max_context_chars: usize = 65536;
        let context = if context.len() > max_context_chars {
            log::warn!(
                "RAG context truncated from {} to {} chars to fit context window",
                context.len(),
                max_context_chars
            );
            context[..max_context_chars].to_string()
        } else {
            context
        };

        let augmented_prompt = build_rag_prompt(&context, user_query);

        Ok(RetrievalResult {
            sources,
            augmented_prompt,
            no_retrieval: false,
        })
    }

    /// Perform RAPTOR-based retrieval without generating an answer.
    /// Returns sources and the augmented prompt for streaming by the frontend.
    pub async fn retrieve_for_raptor_query(
        &self,
        doc_id: i64,
        user_query: &str,
        top_k: usize,
    ) -> Result<RetrievalResult, String> {
        use crate::rag::raptor;

        // Fall back to standard retrieval if no RAPTOR tree
        if !self.has_raptor_tree(doc_id)? {
            log::info!(
                "No RAPTOR tree for doc {}, falling back to standard retrieval (streaming)",
                doc_id
            );
            return self.retrieve_for_query(user_query, top_k).await;
        }

        let raptor_results = raptor::raptor_retrieve(
            self.db.clone(),
            self.router.clone(),
            doc_id,
            user_query,
            top_k,
            None,
        )
        .await?;

        if raptor_results.is_empty() {
            return Ok(RetrievalResult {
                sources: vec![],
                augmented_prompt: String::new(),
                no_retrieval: false,
            });
        }

        let sources: Vec<SourceChunk> = raptor_results
            .into_iter()
            .map(|(chunk_id, score, text)| {
                let doc_title = self
                    .db
                    .doc_title_for_chunk(chunk_id)
                    .unwrap_or_else(|_| "RAPTOR Summary".to_string());
                SourceChunk {
                    chunk_id,
                    doc_title,
                    text,
                    score,
                    page_info: String::new(),
                }
            })
            .collect();

        let context = sources
            .iter()
            .enumerate()
            .map(|(_, s)| {
                s.text.clone()
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Truncate context to fit within context window
        let max_context_chars: usize = 65536;
        let context = if context.len() > max_context_chars {
            log::warn!(
                "RAPTOR context truncated from {} to {} chars to fit context window",
                context.len(),
                max_context_chars
            );
            context[..max_context_chars].to_string()
        } else {
            context
        };

        let augmented_prompt = build_rag_prompt(&context, user_query);

        Ok(RetrievalResult {
            sources,
            augmented_prompt,
            no_retrieval: false,
        })
    }

    /// Retrieve context chunks without generating an answer.
    /// Useful for inspection or custom prompt building.
    pub async fn retrieve(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<SourceChunk>, String> {
        let bm25_results = self.bm25.search(query, top_k)?;
        let vector_results = self.vector_search(query, top_k).await?;
        let fused = rrf_fuse(&[bm25_results, vector_results]);

        let chunk_ids: Vec<i64> = fused.iter().take(top_k).map(|r| r.chunk_id).collect();
        let chunks = self.db.get_chunks_by_ids(&chunk_ids)?;

        let mut sources = Vec::new();
        for fused_result in fused.iter().take(top_k) {
            if let Some(chunk) = chunks.iter().find(|c| c.id == fused_result.chunk_id) {
                let doc_title = self
                    .db
                    .doc_title_for_chunk(chunk.id)
                    .unwrap_or_else(|_| "Unknown".to_string());
                sources.push(SourceChunk {
                    chunk_id: chunk.id,
                    doc_title,
                    text: chunk.text.clone(),
                    score: fused_result.rrf_score,
                    page_info: chunk.metadata.clone(),
                });
            }
        }

        Ok(sources)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Media Embedding Recovery
    // ═══════════════════════════════════════════════════════════════════════

    /// Re-embed media assets whose caption embedding failed during ingestion.
    /// This recovers from the common case where the embedding model wasn't
    /// loaded when the document was first ingested.
    ///
    /// Returns the number of successfully (re-)embedded media assets.
    pub async fn re_embed_unembedded_media(&self) -> usize {
        let unembedded = match self.db.get_unembedded_media() {
            Ok(u) => u,
            Err(e) => {
                log::warn!("Failed to query unembedded media: {e}");
                return 0;
            }
        };

        if unembedded.is_empty() {
            log::debug!("No unembedded media assets found — nothing to fix");
            return 0;
        }

        log::info!(
            "Found {} media assets without embeddings — attempting re-embed",
            unembedded.len()
        );

        let texts: Vec<String> = unembedded.iter().map(|(_, t)| t.clone()).collect();
        let request = tasks::embed_request(texts);

        match self.router.route(&request).await {
            Ok(TaskResponse::Embeddings(vectors)) => {
                let mut count = 0usize;
                for (i, embedding) in vectors.iter().enumerate() {
                    if i < unembedded.len() {
                        let asset_id = unembedded[i].0;
                        if let Ok(()) = self.db.set_media_embedding(asset_id, embedding) {
                            self.vec_index.insert(-asset_id, embedding.clone());
                            count += 1;
                        }
                    }
                }
                log::info!("Re-embedded {count}/{} media captions", unembedded.len());
                count
            }
            Ok(_) => {
                log::warn!("Re-embed returned unexpected response type");
                0
            }
            Err(e) => {
                log::warn!("Re-embed failed: {e}");
                0
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Two-Phase Media Retrieval
    // ═══════════════════════════════════════════════════════════════════════

    /// Two-phase media retrieval: given the LLM's response text, find media
    /// assets whose captions are semantically similar to parts of the response.
    ///
    /// This is the key insight from the TDS article: matching the *response*
    /// to image captions (not the user *query*) dramatically improves recall
    /// because the LLM's answer contains the specific terminology and context
    /// that was near the image in the original document.
    ///
    /// Returns a list of `MediaAssetRecord`s that are relevant to the answer.
    pub async fn retrieve_media_for_response(
        &self,
        response_text: &str,
        top_k: usize,
        similarity_threshold: f32,
    ) -> Vec<crate::rag::db::MediaAssetRecord> {
        let trimmed = response_text.trim();
        let word_count = trimmed.split_whitespace().count();
        // Skip media retrieval for empty or very short responses
        if trimmed.is_empty() || word_count < 5 {
            log::debug!("Skipping media retrieval: response too short ({word_count} words)");
            return vec![];
        }

        // Embed the response text
        let request = tasks::embed_request(vec![response_text.to_string()]);
        let response_embedding = match self.router.route(&request).await {
            Ok(TaskResponse::Embeddings(mut vecs)) => {
                if vecs.is_empty() {
                    log::warn!("Media retrieval: embed returned empty vector list");
                    return vec![];
                }
                vecs.remove(0)
            }
            Ok(_) => {
                log::warn!("Media retrieval: embed returned unexpected response type");
                return vec![];
            }
            Err(e) => {
                log::warn!("Media retrieval: embed failed: {e}");
                return vec![];
            }
        };

        // Search against media caption embeddings
        let media_results = self
            .db
            .media_vector_search(&response_embedding, top_k)
            .unwrap_or_default();

        log::info!(
            "Media vector search returned {} candidates (threshold={:.2})",
            media_results.len(),
            similarity_threshold
        );
        for (asset_id, sim) in &media_results {
            log::debug!("  candidate asset_id={asset_id} sim={sim:.3}");
        }

        // Filter by similarity threshold and fetch full records
        let mut assets = Vec::new();
        for (asset_id, sim) in &media_results {
            if *sim >= similarity_threshold {
                if let Ok(asset) = self.db.get_media_asset(*asset_id) {
                    // Verify the file still exists on disk
                    if std::path::Path::new(&asset.file_path).exists() {
                        log::info!(
                            "Media match: {} (sim={:.3}, type={})",
                            asset.file_path,
                            sim,
                            asset.asset_type
                        );
                        assets.push(asset);
                    } else {
                        log::warn!(
                            "Media asset file missing: {} (asset_id={asset_id})",
                            asset.file_path
                        );
                    }
                }
            }
        }

        if assets.is_empty() && !media_results.is_empty() {
            log::info!(
                "No media assets passed threshold {:.2} — best similarity was {:.3}",
                similarity_threshold,
                media_results.first().map(|(_, s)| *s).unwrap_or(0.0)
            );
        }

        assets
    }

    /// Attempt HyDE: generate a hypothetical answer, use it for embedding search.
    async fn try_hyde(&self, query: &str) -> Option<String> {
        let request = tasks::hyde_request(query);
        match self.router.route(&request).await {
            Ok(TaskResponse::Text(hyde_text)) => {
                log::debug!("HyDE generated hypothetical for query");
                Some(hyde_text)
            }
            _ => {
                log::debug!("HyDE unavailable, using raw query");
                None
            }
        }
    }

    /// Classify a query to determine the retrieval strategy.
    /// Returns: "no_retrieval", "simple_rag", "multi_doc", or "summarization".
    /// Falls back to "simple_rag" if the classifier is unavailable.
    /// Classify a query, returning `(label, confidence)`.
    async fn classify_query(&self, query: &str) -> (String, f32) {
        let request = tasks::classify_request(query);
        match self.router.route(&request).await {
            Ok(TaskResponse::Classification { label, confidence }) => {
                log::debug!("Query classified as '{}' (confidence: {:.2})", label, confidence);
                if confidence < 0.5 {
                    ("simple_rag".to_string(), confidence)
                } else {
                    (label, confidence)
                }
            }
            Ok(TaskResponse::Text(text)) => {
                // Fallback: parse text response as label
                let lower = text.trim().to_lowercase();
                if lower.contains("no_retrieval") {
                    ("no_retrieval".to_string(), 1.0)
                } else if lower.contains("multi_doc") {
                    ("multi_doc".to_string(), 1.0)
                } else if lower.contains("summarization") {
                    ("summarization".to_string(), 1.0)
                } else {
                    ("simple_rag".to_string(), 1.0)
                }
            }
            Ok(_) => {
                log::warn!("Query classification returned unexpected response type");
                ("simple_rag".to_string(), 0.0)
            }
            Err(e) => {
                log::warn!("Query classification failed: {e}");
                ("simple_rag".to_string(), 0.0)
            }
        }
    }

    /// Generate 3 query variations for RAG Fusion retry.
    async fn generate_rephrasings(&self, query: &str) -> Vec<String> {
        let prompt = format!(
            "Generate exactly 3 different rephrasings of this search query. \
             Each rephrasing should approach the topic from a different angle. \
             Output each on a new line, numbered 1-3.\n\n\
             Original query: {query}\n\n\
             Rephrasings:"
        );

        let request = tasks::chat_request(&prompt);
        match self.router.route(&request).await {
            Ok(TaskResponse::Text(text)) => {
                text.lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| {
                        // Strip leading numbers like "1.", "1)", "1:"
                        let trimmed = l.trim();
                        if trimmed.len() > 2 && trimmed.as_bytes()[0].is_ascii_digit() {
                            let skip = trimmed
                                .find(|c: char| c.is_alphabetic())
                                .unwrap_or(0);
                            trimmed[skip..].trim().to_string()
                        } else {
                            trimmed.to_string()
                        }
                    })
                    .take(3)
                    .collect()
            }
            _ => {
                log::debug!("Query rephrasing unavailable");
                vec![query.to_string()] // Fallback: just retry with original
            }
        }
    }

    /// Grade a chunk's relevance to the query (1-5 scale) using cross-encoder.
    /// Returns 3 as default if grading model is unavailable.
    async fn grade_chunk(&self, query: &str, chunk_text: &str) -> u8 {
        let request = tasks::grade_request(query, chunk_text);
        match self.router.route(&request).await {
            Ok(TaskResponse::Score(score)) => {
                // Convert 0-1 score to 1-5 scale
                // 0.0-0.2 → 1, 0.2-0.4 → 2, 0.4-0.6 → 3, 0.6-0.8 → 4, 0.8-1.0 → 5
                let grade = ((score * 5.0).ceil() as u8).clamp(1, 5);
                grade
            }
            _ => {
                log::debug!("Grading unavailable, defaulting to 3");
                3
            }
        }
    }

    /// Batch grade multiple chunks at once for efficiency.
    /// Returns grades in the same order as input chunks.
    async fn batch_grade_chunks(&self, query: &str, chunk_texts: &[&str]) -> Vec<u8> {
        let mut grades = Vec::with_capacity(chunk_texts.len());
        
        // Process each chunk (cross-encoder is fast, no need for true batching yet)
        for chunk_text in chunk_texts {
            grades.push(self.grade_chunk(query, chunk_text).await);
        }
        
        grades
    }

    /// Vector similarity search using the in-memory VectorIndex (IVF-accelerated).
    async fn vector_search(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<(i64, f32)>, String> {
        // Embed the query
        let request = tasks::embed_request(vec![query.to_string()]);
        match self.router.route(&request).await {
            Ok(TaskResponse::Embeddings(vecs)) => {
                if let Some(query_vec) = vecs.into_iter().next() {
                    Ok(self.vec_index.search(&query_vec, top_k))
                } else {
                    Ok(vec![])
                }
            }
            Ok(_) => {
                log::warn!("Embed returned unexpected type for query");
                Ok(vec![])
            }
            Err(e) => {
                log::warn!("Query embedding failed: {e}, falling back to BM25 only");
                Ok(vec![])
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Document Management
    // ═══════════════════════════════════════════════════════════════════════

    /// Delete a document and all its chunks from DB + BM25 index.
    pub async fn delete_document(&self, doc_id: i64) -> Result<(), String> {
        let _lock = self.ingest_lock.lock().await;

        // Get chunk IDs before deleting
        let chunks = self.db.get_chunks_by_ids(
            &self.db.get_chunk_ids_for_doc(doc_id)?,
        )?;
        let chunk_ids: Vec<i64> = chunks.iter().map(|c| c.id).collect();

        // Remove from BM25
        if !chunk_ids.is_empty() {
            self.bm25.delete_chunks(&chunk_ids)?;
            // Remove from in-memory vector index
            for &id in &chunk_ids {
                self.vec_index.remove(id);
            }
        }

        // Remove from DB
        self.db.delete_document(doc_id)?;

        Ok(())
    }

    /// List all ingested documents with their status.
    pub fn list_documents(&self) -> Result<Vec<IngestionStatus>, String> {
        let docs = self.db.list_documents()?;
        Ok(docs
            .into_iter()
            .map(|d| IngestionStatus {
                doc_id: d.id,
                title: d.title,
                file_path: d.path,
                total_chunks: d.chunk_count as usize,
                embedded_chunks: d.chunk_count as usize, // Phase 1 embeds all
                enriched_chunks: d.enriched_count as usize,
                phase: if d.enriched_count >= d.chunk_count {
                    "phase2_complete".to_string()
                } else if d.enriched_count > 0 {
                    "phase2_in_progress".to_string()
                } else {
                    "phase1_complete".to_string()
                },
            })
            .collect())
    }

    /// Ingest all supported files in a directory.
    pub async fn ingest_folder(&self, dir_path: &Path) -> Result<Vec<IngestionStatus>, String> {
        let supported_exts = [
            "pdf", "docx", "pptx", "txt", "md", "rs", "py", "js", "ts",
            "java", "c", "cpp", "h", "hpp", "go", "rb", "sh", "toml",
            "yaml", "yml", "json", "xml", "csv",
        ];

        let mut results = Vec::new();

        let entries = std::fs::read_dir(dir_path)
            .map_err(|e| format!("Failed to read directory: {e}"))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !supported_exts.contains(&ext.as_str()) {
                continue;
            }

            match self.ingest_document(&path).await {
                Ok(status) => results.push(status),
                Err(e) => {
                    log::warn!("Skipping {:?}: {e}", path.file_name());
                }
            }
        }

        Ok(results)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Phase 3: RAPTOR Tree Building (On-Demand)
    // ═══════════════════════════════════════════════════════════════════════

    /// Build a RAPTOR tree for a specific document.
    /// This is an on-demand operation, typically triggered manually or when
    /// retrieval confidence is low.
    pub async fn build_raptor_tree(
        &self,
        doc_id: i64,
    ) -> Result<crate::rag::raptor::RaptorTreeStatus, String> {
        use crate::rag::raptor;
        raptor::build_raptor_tree(self.db.clone(), self.router.clone(), doc_id).await
    }

    /// Check if a document has a RAPTOR tree.
    pub fn has_raptor_tree(&self, doc_id: i64) -> Result<bool, String> {
        self.db.has_raptor_tree(doc_id)
    }

    /// Delete the RAPTOR tree for a document.
    pub async fn delete_raptor_tree(&self, doc_id: i64) -> Result<(), String> {
        self.db.delete_raptor_nodes(doc_id)
    }

    /// Retrieve using RAPTOR tree with confidence-aware traversal.
    /// Falls back to standard retrieval if no RAPTOR tree exists.
    pub async fn query_with_raptor(
        &self,
        doc_id: i64,
        user_query: &str,
        top_k: usize,
    ) -> Result<RagResult, String> {
        use crate::rag::raptor;

        // Check if RAPTOR tree exists
        if !self.has_raptor_tree(doc_id)? {
            log::info!("No RAPTOR tree for doc {}, using standard retrieval", doc_id);
            return self.query(user_query, top_k).await;
        }

        // Retrieve using RAPTOR with confidence-aware traversal
        let raptor_results = raptor::raptor_retrieve(
            self.db.clone(),
            self.router.clone(),
            doc_id,
            user_query,
            top_k,
            None, // Use default confidence threshold
        )
        .await?;

        if raptor_results.is_empty() {
            return Ok(RagResult {
                answer: "No relevant information found in the RAPTOR tree.".to_string(),
                sources: vec![],
            });
        }

        // Convert RAPTOR results to SourceChunk format
        let mut sources: Vec<SourceChunk> = Vec::new();
        for (chunk_id, score, text) in raptor_results {
            let doc_title = self
                .db
                .doc_title_for_chunk(chunk_id)
                .unwrap_or_else(|_| "RAPTOR Summary".to_string());

            sources.push(SourceChunk {
                chunk_id,
                doc_title,
                text,
                score,
                page_info: String::new(),
            });
        }

        // Build augmented prompt
        let context = sources
            .iter()
            .enumerate()
            .map(|(_, s)| {
                s.text.clone()
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let augmented_prompt = build_rag_prompt(&context, user_query);

        // Generate answer via LLM
        let chat_request = tasks::chat_request(&augmented_prompt);
        let answer = match self.router.route(&chat_request).await {
            Ok(TaskResponse::Text(text)) => text,
            Ok(_) => "Failed to generate answer: unexpected response type".to_string(),
            Err(e) => format!("Failed to generate answer: {e}"),
        };

        Ok(RagResult { answer, sources })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Lazy RAPTOR — Automatic Tree Building During Idle Time
    // ═══════════════════════════════════════════════════════════════════════

    /// Automatically build RAPTOR trees for fully-enriched documents
    /// that don't have one yet. Called during enrichment worker idle cycles.
    async fn auto_build_raptor_trees(&self, app_handle: &tauri::AppHandle) {
        log::info!("RAPTOR auto-build: Checking documents for tree building");
        let docs = match self.db.list_documents() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Lazy RAPTOR: failed to list documents: {e}");
                return;
            }
        };

        log::info!("RAPTOR auto-build: Found {} documents to check", docs.len());
        for doc in docs {
            // Only auto-build for fully enriched docs without a RAPTOR tree
            log::debug!(
                "RAPTOR check doc {}: enriched={}/{}, chunks={}",
                doc.id, doc.enriched_count, doc.chunk_count, doc.chunk_count
            );
            
            if doc.enriched_count >= doc.chunk_count && doc.chunk_count > 0 {
                let has_tree = self.has_raptor_tree(doc.id).unwrap_or(true);
                log::debug!("  Doc {} fully enriched, has_tree={}", doc.id, has_tree);
                
                if !has_tree {
                    log::info!(
                        "Lazy RAPTOR: building tree for doc {} ({})",
                        doc.id,
                        doc.title
                    );
                    let _ = app_handle.emit(
                        "rag:enrichment_progress",
                        serde_json::json!({
                            "enriched_this_round": 0,
                            "status": "building_raptor",
                            "doc_title": &doc.title
                        }),
                    );

                    match self.build_raptor_tree(doc.id).await {
                        Ok(status) => {
                            log::info!(
                                "Lazy RAPTOR: tree built for doc {} — {} nodes, {} levels",
                                doc.id,
                                status.nodes_created,
                                status.levels
                            );
                            let _ = app_handle.emit(
                                "rag:enrichment_progress",
                                serde_json::json!({
                                    "enriched_this_round": 0,
                                    "status": "raptor_complete",
                                    "doc_id": doc.id,
                                    "nodes_created": status.nodes_created
                                }),
                            );
                        }
                        Err(e) => {
                            log::warn!(
                                "Lazy RAPTOR: failed for doc {} ({}): {e}",
                                doc.id,
                                doc.title
                            );
                        }
                    }
                } else {
                    log::debug!(
                        "  Doc {} not ready: enriched {}/{} chunks",
                        doc.id, doc.enriched_count, doc.chunk_count
                    );
                }
            }
        }
    }
}
