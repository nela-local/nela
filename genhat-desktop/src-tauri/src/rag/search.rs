//! Tantivy BM25 full-text search index (MmapDirectory).
//!
//! Maintains a tantivy index alongside the SQLite store.
//! Each chunk is indexed with its chunk_id, text, and doc_title for
//! keyword retrieval.  Uses stemmed + raw dual-field indexing for
//! proper acronym and technical term matching.

use std::path::Path;
use std::sync::{Arc, RwLock};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy};

/// Handle to the Tantivy BM25 search index.
#[derive(Clone)]
pub struct BM25Index {
    index: Index,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    _schema: Schema,
    // Field handles
    f_chunk_id: Field,
    f_text: Field,
    f_text_raw: Field,
    f_title: Field,
}

impl BM25Index {
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
        let writer = index
            .writer(50_000_000)
            .map_err(|e| format!("Writer error: {e}"))?;

        Ok(Self {
            index,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            _schema: schema,
            f_chunk_id,
            f_text,
            f_text_raw,
            f_title,
        })
    }

    /// Attempt to open or create a tantivy index with the given schema.
    fn try_open_index(index_dir: &Path, schema: Schema) -> Result<Index, String> {
        let mmap_dir = tantivy::directory::MmapDirectory::open(index_dir)
            .map_err(|e| format!("MmapDirectory error: {e}"))?;
        Index::open_or_create(mmap_dir, schema)
            .map_err(|e| format!("Index open error: {e}"))
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
        Ok(())
    }

    /// BM25 search. Returns (chunk_id, score) pairs sorted by relevance.
    pub fn search(&self, query_str: &str, top_k: usize) -> Result<Vec<(i64, f32)>, String> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.f_text, self.f_text_raw, self.f_title]);

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
            if let Some(chunk_id_val) = retrieved.get_first(self.f_chunk_id) {
                if let tantivy::schema::OwnedValue::I64(id) = chunk_id_val {
                    results.push((*id, score));
                }
            }
        }

        Ok(results)
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
        Ok(())
    }
}
