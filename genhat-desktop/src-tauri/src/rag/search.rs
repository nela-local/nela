//! Tantivy BM25 full-text search index.
//!
//! Maintains a tantivy index alongside the SQLite store.
//! Each chunk is indexed with its chunk_id, text, and doc_title for
//! keyword retrieval.  Uses stemmed + raw dual-field indexing for
//! proper acronym and technical term matching.
//!
//! For small indexes (<= 50 MB on disk), we mirror committed documents into a
//! RAM-backed read index for faster search, while keeping the writer on disk.

use std::path::Path;
use std::sync::{Arc, RwLock};
use tantivy::collector::TopDocs;
use tantivy::directory::error::LockError;
use tantivy::query::AllQuery;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyError};

/// Handle to the Tantivy BM25 search index.
#[derive(Clone)]
pub struct BM25Index {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    ram_mode_enabled: bool,
    ram_index: Arc<RwLock<Option<Index>>>,
    ram_reader: Arc<RwLock<Option<IndexReader>>>,
    _schema: Schema,
    // Field handles
    f_chunk_id: Field,
    f_text: Field,
    f_text_raw: Field,
    f_title: Field,
}

impl BM25Index {
    const RAM_READER_MAX_BYTES: u64 = 50 * 1024 * 1024;

    /// Open or create the tantivy index at the given directory.
    pub fn open(index_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(index_dir)
            .map_err(|e| format!("Failed to create index dir: {e}"))?;

        // Build schema with both stemmed (TEXT) and raw (STRING) fields
        let mut builder = Schema::builder();
        let f_chunk_id = builder.add_i64_field("chunk_id", INDEXED | STORED);
        let f_text = builder.add_text_field(
            "text",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("en_stem")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );
        let f_text_raw = builder.add_text_field("text_raw", STRING | STORED);
        let f_title = builder.add_text_field("title", TEXT | STORED);
        let schema = builder.build();

        // Open or create index — if schema changed, wipe and recreate
        let index = match Self::try_open_index(index_dir, schema.clone()) {
            Ok(idx) => idx,
            Err(_) => {
                log::warn!(
                    "BM25 index schema mismatch — deleting old index at {}",
                    index_dir.display()
                );
                // Remove old index files and recreate
                let _ = std::fs::remove_dir_all(index_dir);
                std::fs::create_dir_all(index_dir)
                    .map_err(|e| format!("Failed to recreate index dir: {e}"))?;
                Self::try_open_index(index_dir, schema.clone())
                    .map_err(|e| format!("Index recreate failed: {e}"))?
            }
        };

        // Register the en_stem tokenizer (tantivy includes it by default)
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| format!("Reader error: {e}"))?;

        // 50 MB heap for the writer
        let mut writer = match index.writer(50_000_000) {
            Ok(writer) => writer,
            Err(e) if Self::is_lock_busy(&e) => {
                log::warn!(
                    "BM25 writer lock busy at {}; attempting stale lock cleanup",
                    index_dir.display()
                );
                Self::cleanup_stale_writer_lock(index_dir)?;
                index
                    .writer(50_000_000)
                    .map_err(|e| format!("Writer error after stale lock cleanup: {e}"))?
            }
            Err(e) => return Err(format!("Writer error: {e}")),
        };

        // Initialize tantivy metadata files eagerly to avoid startup-time
        // watcher warnings on fresh/empty indexes.
        writer
            .commit()
            .map_err(|e| format!("Initial writer commit error: {e}"))?;

        let ram_mode_enabled = Self::should_enable_ram_reader(index_dir);
        let (ram_index, ram_reader) = if ram_mode_enabled {
            match Self::build_ram_reader_from_disk(&index, &reader, &schema) {
                Ok((ram_index, ram_reader)) => {
                    log::info!(
                        "BM25 RAM reader enabled for small index at {}",
                        index_dir.display()
                    );
                    (Some(ram_index), Some(ram_reader))
                }
                Err(e) => {
                    log::warn!(
                        "Failed to initialize BM25 RAM reader (falling back to disk reader): {e}"
                    );
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            index,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            ram_mode_enabled,
            ram_index: Arc::new(RwLock::new(ram_index)),
            ram_reader: Arc::new(RwLock::new(ram_reader)),
            _schema: schema,
            f_chunk_id,
            f_text,
            f_text_raw,
            f_title,
        })
    }

    fn should_enable_ram_reader(index_dir: &Path) -> bool {
        match Self::dir_size_bytes(index_dir) {
            Ok(bytes) => bytes > 0 && bytes <= Self::RAM_READER_MAX_BYTES,
            Err(e) => {
                log::warn!(
                    "Could not determine BM25 index size at {}; RAM reader disabled: {e}",
                    index_dir.display()
                );
                false
            }
        }
    }

    fn dir_size_bytes(path: &Path) -> Result<u64, String> {
        let mut total = 0u64;
        let entries = std::fs::read_dir(path).map_err(|e| format!("read_dir failed: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("dir entry read failed: {e}"))?;
            let meta = entry
                .metadata()
                .map_err(|e| format!("metadata read failed: {e}"))?;
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                total = total.saturating_add(Self::dir_size_bytes(&entry.path())?);
            }
        }
        Ok(total)
    }

    fn build_ram_reader_from_disk(
        disk_index: &Index,
        disk_reader: &IndexReader,
        schema: &Schema,
    ) -> Result<(Index, IndexReader), String> {
        disk_reader
            .reload()
            .map_err(|e| format!("Disk reader reload error: {e}"))?;

        let searcher = disk_reader.searcher();
        let doc_count = searcher.num_docs() as usize;

        let mut ram_index = Index::create_in_ram(schema.clone());
        ram_index.set_tokenizers(disk_index.tokenizers().clone());
        ram_index.set_fast_field_tokenizers(disk_index.fast_field_tokenizer().clone());

        let mut ram_writer = ram_index
            .writer(20_000_000)
            .map_err(|e| format!("RAM writer error: {e}"))?;

        if doc_count > 0 {
            let top_docs = searcher
                .search(&AllQuery, &TopDocs::with_limit(doc_count))
                .map_err(|e| format!("All-docs scan error: {e}"))?;

            for (_, doc_address) in top_docs {
                let doc: tantivy::TantivyDocument = searcher
                    .doc(doc_address)
                    .map_err(|e| format!("Doc copy error: {e}"))?;
                ram_writer
                    .add_document(doc)
                    .map_err(|e| format!("RAM add doc error: {e}"))?;
            }
        }

        ram_writer
            .commit()
            .map_err(|e| format!("RAM commit error: {e}"))?;

        let ram_reader = ram_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| format!("RAM reader error: {e}"))?;

        Ok((ram_index, ram_reader))
    }

    fn refresh_ram_reader_if_enabled(&self) {
        if !self.ram_mode_enabled {
            return;
        }

        match Self::build_ram_reader_from_disk(&self.index, &self.reader, &self._schema) {
            Ok((ram_index, ram_reader)) => {
                if let Ok(mut idx_guard) = self.ram_index.write() {
                    *idx_guard = Some(ram_index);
                }
                if let Ok(mut rdr_guard) = self.ram_reader.write() {
                    *rdr_guard = Some(ram_reader);
                }
            }
            Err(e) => {
                log::warn!("Failed to refresh BM25 RAM reader; using disk reader: {e}");
            }
        }
    }

    fn search_with(
        index: &Index,
        reader: &IndexReader,
        query_str: &str,
        top_k: usize,
        f_chunk_id: Field,
        f_text: Field,
        f_text_raw: Field,
        f_title: Field,
    ) -> Result<Vec<(i64, f32)>, String> {
        let searcher = reader.searcher();
        let query_parser = QueryParser::for_index(index, vec![f_text, f_text_raw, f_title]);

        let query = query_parser
            .parse_query(query_str)
            .map_err(|e| format!("Query parse error: {e}"))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(top_k))
            .map_err(|e| format!("Search error: {e}"))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved: tantivy::TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| format!("Doc fetch error: {e}"))?;
            if let Some(chunk_id_val) = retrieved.get_first(f_chunk_id) {
                if let tantivy::schema::OwnedValue::I64(id) = chunk_id_val {
                    results.push((*id, score));
                }
            }
        }

        Ok(results)
    }

    /// Attempt to open or create a tantivy index with the given schema.
    fn try_open_index(index_dir: &Path, schema: Schema) -> Result<Index, String> {
        let mmap_dir = tantivy::directory::MmapDirectory::open(index_dir)
            .map_err(|e| format!("MmapDirectory error: {e}"))?;
        Index::open_or_create(mmap_dir, schema)
            .map_err(|e| format!("Index open error: {e}"))
    }

    fn is_lock_busy(error: &TantivyError) -> bool {
        matches!(error, TantivyError::LockFailure(LockError::LockBusy, _))
    }

    fn cleanup_stale_writer_lock(index_dir: &Path) -> Result<(), String> {
        let lock_path = index_dir.join(".tantivy-writer.lock");
        if lock_path.exists() {
            std::fs::remove_file(&lock_path).map_err(|e| {
                format!(
                    "Failed to remove stale Tantivy writer lock file {}: {e}",
                    lock_path.display()
                )
            })?;
            log::warn!(
                "Removed stale Tantivy writer lock file {}",
                lock_path.display()
            );
        }
        Ok(())
    }

    /// Index a single chunk.
    pub fn add_chunk(&self, chunk_id: i64, text: &str, doc_title: &str) -> Result<(), String> {
        let writer = self.writer.write().unwrap();
        writer
            .add_document(doc!(
                self.f_chunk_id => chunk_id,
                self.f_text => text,
                self.f_text_raw => text,
                self.f_title => doc_title,
            ))
            .map_err(|e| format!("Add doc error: {e}"))?;
        Ok(())
    }

    /// Index a batch of chunks and commit.
    pub fn add_chunks_batch(
        &self,
        chunks: &[(i64, String, String)], // (chunk_id, text, doc_title)
    ) -> Result<(), String> {
        {
            let writer = self.writer.write().unwrap();
            for (id, text, title) in chunks {
                writer
                    .add_document(doc!(
                        self.f_chunk_id => *id,
                        self.f_text => text.as_str(),
                        self.f_text_raw => text.as_str(),
                        self.f_title => title.as_str(),
                    ))
                    .map_err(|e| format!("Add doc error: {e}"))?;
            }
        }
        self.commit()?;
        Ok(())
    }

    /// Commit pending writes.
    pub fn commit(&self) -> Result<(), String> {
        let mut writer = self.writer.write().unwrap();
        writer
            .commit()
            .map_err(|e| format!("Commit error: {e}"))?;
        drop(writer);
        self.refresh_ram_reader_if_enabled();
        Ok(())
    }

    /// Delete all indexed chunks for a given set of chunk IDs.
    pub fn delete_chunks(&self, chunk_ids: &[i64]) -> Result<(), String> {
        let mut writer = self.writer.write().unwrap();
        for &id in chunk_ids {
            let term = tantivy::Term::from_field_i64(self.f_chunk_id, id);
            writer.delete_term(term);
        }
        writer
            .commit()
            .map_err(|e| format!("Commit error: {e}"))?;
        drop(writer);
        self.refresh_ram_reader_if_enabled();
        Ok(())
    }

    /// BM25 search. Returns (chunk_id, score) pairs sorted by relevance.
    pub fn search(&self, query_str: &str, top_k: usize) -> Result<Vec<(i64, f32)>, String> {
        if self.ram_mode_enabled {
            if let (Ok(ram_index_guard), Ok(ram_reader_guard)) =
                (self.ram_index.read(), self.ram_reader.read())
            {
                if let (Some(ram_index), Some(ram_reader)) =
                    (ram_index_guard.as_ref(), ram_reader_guard.as_ref())
                {
                    return Self::search_with(
                        ram_index,
                        ram_reader,
                        query_str,
                        top_k,
                        self.f_chunk_id,
                        self.f_text,
                        self.f_text_raw,
                        self.f_title,
                    );
                }
            }
        }

        Self::search_with(
            &self.index,
            &self.reader,
            query_str,
            top_k,
            self.f_chunk_id,
            self.f_text,
            self.f_text_raw,
            self.f_title,
        )
    }

    /// Delete the entire index contents and rebuild.
    pub fn clear(&self) -> Result<(), String> {
        let mut writer = self.writer.write().unwrap();
        writer
            .delete_all_documents()
            .map_err(|e| format!("Clear error: {e}"))?;
        writer
            .commit()
            .map_err(|e| format!("Commit error: {e}"))?;
        drop(writer);
        self.refresh_ram_reader_if_enabled();
        Ok(())
    }
}
