//! SQLite vector store for RAG with r2d2 connection pooling.
//!
//! Schema:
//!   documents  — one row per ingested file
//!   chunks     — one row per text chunk, with embeddings stored as BLOB
//!
//! Uses an r2d2 connection pool (8 connections) for parallel read access
//! under WAL mode. Vector search is delegated to the in-memory VectorIndex
//! (see vecindex.rs) for sub-linear ANN search; the brute-force fallback
//! method is retained for testing.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::{Path, PathBuf};

// ── Types ──

/// A stored document record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentRecord {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub doc_type: String,
    pub chunk_count: i64,
    pub enriched_count: i64,
    pub created_at: String,
}

/// A stored chunk record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChunkRecord {
    pub id: i64,
    pub doc_id: i64,
    pub chunk_index: i32,
    pub text: String,
    pub enriched_text: Option<String>,
    pub metadata: String,
    pub confidence: Option<f64>,
}

/// Handle to the RAG database backed by an r2d2 connection pool.
#[derive(Clone)]
pub struct RagDb {
    pool: Pool<SqliteConnectionManager>,
    pub path: PathBuf,
}

// ── Helpers ──

/// Serialize f32 embedding to bytes (little-endian).
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &v in embedding {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Deserialize bytes back to f32 embedding.
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-12 {
        0.0
    } else {
        dot / denom
    }
}

// ── Connection Pool Initializer ──

/// Configures each pooled SQLite connection with WAL mode and synchronous=NORMAL.
#[derive(Debug)]
struct WalModeCustomizer;

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error> for WalModeCustomizer {
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(())
    }
}

// ── Implementation ──

impl RagDb {
    /// Get a connection from the pool.
    pub(crate) fn conn(
        &self,
    ) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        self.pool.get().map_err(|e| format!("DB pool error: {e}"))
    }

    /// Open (or create) the RAG database at the given path.
    pub fn open(db_path: &Path) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create DB directory: {e}"))?;
        }

        let manager = SqliteConnectionManager::file(db_path);
        let pool = Pool::builder()
            .max_size(8) // 8 connections for parallel read access
            .connection_customizer(Box::new(WalModeCustomizer))
            .build(manager)
            .map_err(|e| format!("Failed to create DB pool: {e}"))?;

        let db = Self {
            pool,
            path: db_path.to_path_buf(),
        };

        db.create_tables()?;
        db.create_raptor_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS documents (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                path         TEXT NOT NULL UNIQUE,
                title        TEXT NOT NULL,
                doc_type     TEXT NOT NULL,
                chunk_count  INTEGER NOT NULL DEFAULT 0,
                enriched_count INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id       INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                chunk_index  INTEGER NOT NULL,
                text         TEXT NOT NULL,
                enriched_text TEXT,
                metadata     TEXT NOT NULL DEFAULT '',
                embedding    BLOB,
                enriched_embedding BLOB,
                confidence   REAL,
                UNIQUE(doc_id, chunk_index)
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_doc ON chunks(doc_id);
            ",
        )
        .map_err(|e| format!("Failed to create tables: {e}"))
    }

    // ── Document CRUD ──

    /// Insert a new document. Returns the document ID.
    pub fn insert_document(
        &self,
        path: &str,
        title: &str,
        doc_type: &str,
        chunk_count: i64,
    ) -> Result<i64, String> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO documents (path, title, doc_type, chunk_count) VALUES (?1, ?2, ?3, ?4)",
            params![path, title, doc_type, chunk_count],
        )
        .map_err(|e| format!("Insert doc error: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all documents.
    pub fn list_documents(&self) -> Result<Vec<DocumentRecord>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, path, title, doc_type, chunk_count, enriched_count, created_at
                 FROM documents ORDER BY created_at DESC",
            )
            .map_err(|e| format!("Query error: {e}"))?;

        let docs = stmt
            .query_map([], |row| {
                Ok(DocumentRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    doc_type: row.get(3)?,
                    chunk_count: row.get(4)?,
                    enriched_count: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(docs)
    }

    /// Delete a document and all its chunks.
    pub fn delete_document(&self, doc_id: i64) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM chunks WHERE doc_id = ?1", params![doc_id])
            .map_err(|e| format!("Delete chunks error: {e}"))?;
        conn.execute("DELETE FROM documents WHERE id = ?1", params![doc_id])
            .map_err(|e| format!("Delete doc error: {e}"))?;
        Ok(())
    }

    /// Check if a document (by path) is already ingested.
    pub fn document_exists(&self, path: &str) -> Result<bool, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM documents WHERE path = ?1")
            .map_err(|e| format!("Query error: {e}"))?;
        let count: i64 = stmt
            .query_row(params![path], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?;
        Ok(count > 0)
    }

    // ── Chunk CRUD ──

    /// Insert a batch of chunks for a document. Returns the chunk IDs.
    pub fn insert_chunks(
        &self,
        doc_id: i64,
        chunks: &[(usize, String)], // (chunk_index, text)
    ) -> Result<Vec<i64>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "INSERT INTO chunks (doc_id, chunk_index, text, metadata) VALUES (?1, ?2, ?3, '')",
            )
            .map_err(|e| format!("Prepare error: {e}"))?;

        let mut ids = Vec::with_capacity(chunks.len());
        for (idx, text) in chunks {
            stmt.execute(params![doc_id, *idx as i32, text])
                .map_err(|e| format!("Insert chunk error: {e}"))?;
            ids.push(conn.last_insert_rowid());
        }

        Ok(ids)
    }

    /// Store an embedding for a chunk, with optional confidence score.
    pub fn set_chunk_embedding(
        &self,
        chunk_id: i64,
        embedding: &[f32],
        confidence: Option<f64>,
    ) -> Result<(), String> {
        let conn = self.conn()?;
        let bytes = embedding_to_bytes(embedding);
        conn.execute(
            "UPDATE chunks SET embedding = ?1, confidence = ?2 WHERE id = ?3",
            params![bytes, confidence, chunk_id],
        )
        .map_err(|e| format!("Set embedding error: {e}"))
        .map(|_| ())
    }

    /// Store enriched text and optionally its embedding for a chunk.
    pub fn set_chunk_enrichment(
        &self,
        chunk_id: i64,
        enriched_text: &str,
        enriched_embedding: Option<&Vec<f32>>,
    ) -> Result<(), String> {
        let conn = self.conn()?;
        let bytes = enriched_embedding.map(|e| embedding_to_bytes(e));
        conn.execute(
            "UPDATE chunks SET enriched_text = ?1, enriched_embedding = ?2 WHERE id = ?3",
            params![enriched_text, bytes, chunk_id],
        )
        .map_err(|e| format!("Set enrichment error: {e}"))
        .map(|_| ())?;

        // Increment enriched_count on the parent document
        conn.execute(
            "UPDATE documents SET enriched_count = enriched_count + 1 WHERE id = (SELECT doc_id FROM chunks WHERE id = ?1)",
            params![chunk_id],
        )
        .map_err(|e| format!("Update enriched count error: {e}"))
        .map(|_| ())
    }

    /// Get chunk IDs that haven't been enriched yet (across all documents).
    /// Returns up to `limit` IDs.
    pub fn unenriched_chunk_ids(&self, limit: usize) -> Result<Vec<i64>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id FROM chunks WHERE enriched_text IS NULL AND embedding IS NOT NULL ORDER BY id LIMIT ?1",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let ids = stmt
            .query_map(params![limit as i64], |row| row.get::<_, i64>(0))
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// Get all chunk IDs belonging to a document.
    pub fn get_chunk_ids_for_doc(&self, doc_id: i64) -> Result<Vec<i64>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id FROM chunks WHERE doc_id = ?1 ORDER BY chunk_index")
            .map_err(|e| format!("Query error: {e}"))?;
        let ids = stmt
            .query_map(params![doc_id], |row| row.get::<_, i64>(0))
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    /// Get a chunk by ID.
    pub fn get_chunk(&self, chunk_id: i64) -> Result<ChunkRecord, String> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, doc_id, chunk_index, text, enriched_text, metadata, confidence
             FROM chunks WHERE id = ?1",
            params![chunk_id],
            |row| {
                Ok(ChunkRecord {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    chunk_index: row.get(2)?,
                    text: row.get(3)?,
                    enriched_text: row.get(4)?,
                    metadata: row.get(5)?,
                    confidence: row.get(6)?,
                })
            },
        )
        .map_err(|e| format!("Get chunk error: {e}"))
    }

    // ── Vector Search ──

    /// Brute-force KNN search over all chunks.
    /// Returns (chunk_id, similarity) pairs, sorted descending by similarity.
    /// If `use_enriched` is true, prefer enriched embeddings.
    pub fn vector_search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        use_enriched: bool,
    ) -> Result<Vec<(i64, f32)>, String> {
        let conn = self.conn()?;
        let sql = if use_enriched {
            "SELECT id, COALESCE(enriched_embedding, embedding)
             FROM chunks WHERE COALESCE(enriched_embedding, embedding) IS NOT NULL"
        } else {
            "SELECT id, embedding
             FROM chunks WHERE embedding IS NOT NULL"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("Query error: {e}"))?;

        let mut scored: Vec<(i64, f32)> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, blob))
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .map(|(id, blob)| {
                let emb = bytes_to_embedding(&blob);
                let sim = cosine_similarity(query_embedding, &emb);
                (id, sim)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Get full chunk records by IDs (preserving order).
    pub fn get_chunks_by_ids(&self, ids: &[i64]) -> Result<Vec<ChunkRecord>, String> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn()?;
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT id, doc_id, chunk_index, text, enriched_text, metadata, confidence
             FROM chunks WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| format!("Prepare error: {e}"))?;

        let params: Vec<Box<dyn rusqlite::types::ToSql>> =
            ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>).collect();

        let rows: Vec<ChunkRecord> = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok(ChunkRecord {
                    id: row.get(0)?,
                    doc_id: row.get(1)?,
                    chunk_index: row.get(2)?,
                    text: row.get(3)?,
                    enriched_text: row.get(4)?,
                    metadata: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();

        // Re-order to match input ID order
        let mut ordered = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(rec) = rows.iter().find(|r| r.id == *id) {
                ordered.push(rec.clone());
            }
        }
        Ok(ordered)
    }

    /// Get the document title for a chunk.
    pub fn doc_title_for_chunk(&self, chunk_id: i64) -> Result<String, String> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT d.title FROM documents d
             JOIN chunks c ON c.doc_id = d.id
             WHERE c.id = ?1",
            params![chunk_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|e| format!("Query error: {e}"))
    }

    // ── Bulk Embedding Access (for VectorIndex) ──

    /// Get all chunk embeddings (prefer enriched). For loading VectorIndex on startup.
    pub fn get_all_embeddings(&self) -> Result<Vec<(i64, Vec<f32>)>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, COALESCE(enriched_embedding, embedding) FROM chunks
                 WHERE COALESCE(enriched_embedding, embedding) IS NOT NULL",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let results = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, bytes_to_embedding(&blob)))
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }

    /// Get embeddings for all chunks of a specific document (prefer enriched).
    pub fn get_chunk_embeddings_for_doc(&self, doc_id: i64) -> Result<Vec<(i64, Vec<f32>)>, String> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, COALESCE(enriched_embedding, embedding) FROM chunks
                 WHERE doc_id = ?1 AND COALESCE(enriched_embedding, embedding) IS NOT NULL",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let results = stmt
            .query_map(params![doc_id], |row| {
                let id: i64 = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, bytes_to_embedding(&blob)))
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(results)
    }
}
