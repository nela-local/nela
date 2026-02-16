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

use crate::rag::chunker::{chunk_text_default, Chunk};
use crate::rag::db::RagDb;
use crate::rag::fusion::{rrf_fuse, FusedResult};
use crate::rag::parsers;
use crate::rag::search::BM25Index;
use crate::registry::types::TaskResponse;
use crate::router::tasks;
use crate::router::TaskRouter;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

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

/// A source chunk with provenance info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceChunk {
    pub chunk_id: i64,
    pub doc_title: String,
    pub text: String,
    pub score: f64,
}

/// Status of document ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionStatus {
    pub doc_id: i64,
    pub title: String,
    pub total_chunks: usize,
    pub embedded_chunks: usize,
    pub enriched_chunks: usize,
    pub phase: String,
}

/// The RAG pipeline engine.
pub struct RagPipeline {
    pub db: Arc<RagDb>,
    pub bm25: Arc<BM25Index>,
    router: Arc<TaskRouter>,
    /// Lock to serialize ingestion (prevent concurrent writes to tantivy writer).
    ingest_lock: TokioMutex<()>,
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

        Ok(Self {
            db,
            bm25,
            router,
            ingest_lock: TokioMutex::new(()),
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Phase 1: Instant Ingestion
    // ═══════════════════════════════════════════════════════════════════════

    /// Ingest a single document: parse → chunk → embed → store + index.
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

        // 1. Parse document
        let parsed = parsers::parse_document(file_path)?;
        let title = parsed.title.clone();
        let doc_type = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        // 2. Chunk the sections
        let mut all_chunks: Vec<Chunk> = Vec::new();
        for section in &parsed.sections {
            let section_chunks = chunk_text_default(&section.text);
            all_chunks.extend(section_chunks);
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

        // 3. Insert document record
        let doc_id = self.db.insert_document(
            &path_str,
            &title,
            &doc_type,
            chunk_count as i64,
        )?;

        // 4. Insert chunk records (text only, no embeddings yet)
        let chunk_texts: Vec<(usize, String)> = all_chunks
            .iter()
            .map(|c| (c.index, c.text.clone()))
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

        Ok(IngestionStatus {
            doc_id,
            title,
            total_chunks: chunk_count,
            embedded_chunks: embedded_count,
            enriched_chunks: 0,
            phase: "phase1_complete".to_string(),
        })
    }

    /// Embed a batch of chunks and store embeddings in the DB.
    /// Returns the number of successfully embedded chunks.
    async fn embed_chunks(&self, chunk_ids: &[i64], chunks: &[Chunk]) -> usize {
        // Batch embeddings (submit all texts at once)
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();

        let request = tasks::embed_request(texts);
        match self.router.route(&request).await {
            Ok(TaskResponse::Embeddings(vectors)) => {
                let mut count = 0;
                for (i, embedding) in vectors.iter().enumerate() {
                    if i < chunk_ids.len() {
                        if let Ok(()) =
                            self.db.set_chunk_embedding(chunk_ids[i], embedding, None)
                        {
                            count += 1;
                        }
                    }
                }
                count
            }
            Ok(other) => {
                log::warn!("Embed task returned unexpected response: {:?}", other);
                0
            }
            Err(e) => {
                log::warn!("Embedding failed (model may not be loaded): {e}");
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
                    }
                }
                Ok(_) => {
                    log::warn!("Enrich returned non-text for chunk {}", chunk.id);
                }
                Err(e) => {
                    log::warn!("Enrichment failed for chunk {}: {e}", chunk.id);
                    break; // Model probably not available
                }
            }
        }

        Ok(enriched)
    }

    /// Spawn a background enrichment worker that processes chunks gradually.
    pub fn start_enrichment_worker(pipeline: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            log::info!("RAG enrichment worker started");
            loop {
                match pipeline.enrich_pending(5).await {
                    Ok(0) => {
                        // Nothing to enrich — wait longer
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                    Ok(n) => {
                        log::info!("Enriched {n} chunks, continuing...");
                        // Small delay to avoid hogging the model
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        log::warn!("Enrichment error: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                }
            }
        });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Retrieval Pipeline
    // ═══════════════════════════════════════════════════════════════════════

    /// Full RAG query: classify → HyDE → hybrid search → RRF → context build → generate.
    pub async fn query(&self, user_query: &str, top_k: usize) -> Result<RagResult, String> {
        // 1. Optional HyDE: generate a hypothetical answer to improve retrieval
        let search_query = match self.try_hyde(user_query).await {
            Some(hyde_text) => hyde_text,
            None => user_query.to_string(),
        };

        // 2. BM25 keyword search
        let bm25_results = self.bm25.search(user_query, top_k)?;

        // 3. Vector similarity search (using the potentially HyDE-enhanced query)
        let vector_results = self.vector_search(&search_query, top_k).await?;

        // 4. RRF fusion
        let fused = rrf_fuse(&[bm25_results, vector_results]);

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

                graded_sources.push((
                    SourceChunk {
                        chunk_id: chunk.id,
                        doc_title,
                        text: chunk.text.clone(),
                        score: fused_result.rrf_score,
                    },
                    grade,
                ));
            }
        }

        // Filter: keep only chunks with grade >= 3, then take top_k
        graded_sources.retain(|(_, grade)| *grade >= 3);
        graded_sources.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.score.partial_cmp(&a.0.score).unwrap_or(std::cmp::Ordering::Equal)));
        graded_sources.truncate(top_k);

        let sources: Vec<SourceChunk> = graded_sources.into_iter().map(|(s, _)| s).collect();

        if sources.is_empty() {
            return Ok(RagResult {
                answer: "Retrieved documents were not relevant to your question. Try rephrasing or ingesting more documents."
                    .to_string(),
                sources: vec![],
            });
        }

        // 8. Build augmented prompt
        let context = sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "[Source {} — {}]\n{}",
                    i + 1,
                    s.doc_title,
                    s.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let augmented_prompt = format!(
            "Use the following context to answer the question. \
             Cite sources using [Source N] when referencing specific information.\n\n\
             Context:\n{context}\n\n\
             Question: {user_query}\n\n\
             Answer:"
        );

        // 9. Generate answer via LLM
        let chat_request = tasks::chat_request(&augmented_prompt);
        let answer = match self.router.route(&chat_request).await {
            Ok(TaskResponse::Text(text)) => text,
            Ok(_) => "Failed to generate answer: unexpected response type".to_string(),
            Err(e) => format!("Failed to generate answer: {e}"),
        };

        Ok(RagResult { answer, sources })
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
                });
            }
        }

        Ok(sources)
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

    /// Grade a chunk's relevance to the query (1-5 scale).
    /// Returns 3 as default if grading model is unavailable.
    async fn grade_chunk(&self, query: &str, chunk_text: &str) -> u8 {
        let request = tasks::grade_request(query, chunk_text);
        match self.router.route(&request).await {
            Ok(TaskResponse::Text(response)) => {
                // Parse the grade (LLM returns a single digit 1-5)
                response
                    .trim()
                    .chars()
                    .find(|c| c.is_ascii_digit())
                    .and_then(|c| c.to_digit(10))
                    .map(|d| d.clamp(1, 5) as u8)
                    .unwrap_or(3)
            }
            _ => {
                log::debug!("Grading unavailable, defaulting to 3");
                3
            }
        }
    }

    /// Vector similarity search using the embedding model.
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
                    self.db.vector_search(&query_vec, top_k, false)
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
}
