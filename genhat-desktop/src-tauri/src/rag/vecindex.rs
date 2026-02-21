//! In-memory IVF (Inverted File) vector index for fast approximate nearest neighbor search.
//!
//! Replaces brute-force O(N) cosine scan in SQLite with:
//!   - In-memory pre-loaded embeddings (no SQL/blob overhead per search)
//!   - IVF partitioning for sub-linear search when >128 vectors
//!   - Automatic index rebuild after threshold insertions
//!
//! For small corpora (<128 vectors), falls back to exact brute-force in-memory
//! search which is still much faster than the SQL-based scan.

use crate::rag::db::{cosine_similarity, RagDb};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

// ═══════════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════════

/// Number of IVF partitions (Voronoi cells).
const IVF_NUM_CELLS: usize = 32;

/// Number of partitions to probe during search (higher = better recall, slower).
const IVF_NUM_PROBES: usize = 4;

/// Minimum vector count before IVF partitioning is used.
const MIN_VECTORS_FOR_IVF: usize = 128;

/// Number of new insertions before automatic IVF rebuild.
const REBUILD_THRESHOLD: usize = 100;

/// Maximum k-means iterations when building IVF centroids.
const KMEANS_MAX_ITER: usize = 20;

// ═══════════════════════════════════════════════════════════════════════════════
// VectorIndex
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory vector index with optional IVF partitioning.
///
/// For corpora under [`MIN_VECTORS_FOR_IVF`] vectors, performs exact brute-force
/// search in memory (already much faster than SQL blob deserialization).
/// Above that threshold, k-means IVF partitioning gives sub-linear search.
pub struct VectorIndex {
    /// All vectors, keyed by chunk_id.
    vectors: RwLock<HashMap<i64, Vec<f32>>>,
    /// IVF centroids (one per partition).
    centroids: RwLock<Vec<Vec<f32>>>,
    /// Partition assignments: centroids[i] owns partition_members[i].
    partition_members: RwLock<Vec<Vec<i64>>>,
    /// Insertions since last IVF rebuild.
    pending_inserts: AtomicUsize,
}

impl VectorIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self {
            vectors: RwLock::new(HashMap::new()),
            centroids: RwLock::new(Vec::new()),
            partition_members: RwLock::new(Vec::new()),
            pending_inserts: AtomicUsize::new(0),
        }
    }

    /// Load all existing embeddings from the database into memory.
    pub fn load_from_db(db: &RagDb) -> Result<Self, String> {
        let all_embeddings = db.get_all_embeddings()?;
        let mut map = HashMap::with_capacity(all_embeddings.len());
        for (id, emb) in all_embeddings {
            map.insert(id, emb);
        }

        let index = Self {
            vectors: RwLock::new(map),
            centroids: RwLock::new(Vec::new()),
            partition_members: RwLock::new(Vec::new()),
            pending_inserts: AtomicUsize::new(0),
        };

        // Build IVF partitions if enough vectors
        index.rebuild_ivf();
        Ok(index)
    }

    /// Insert or update a vector for a chunk.
    pub fn insert(&self, chunk_id: i64, embedding: Vec<f32>) {
        {
            let mut vecs = self.vectors.write().unwrap();
            vecs.insert(chunk_id, embedding);
        }
        self.pending_inserts.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove a vector by chunk_id.
    pub fn remove(&self, chunk_id: i64) {
        let mut vecs = self.vectors.write().unwrap();
        vecs.remove(&chunk_id);
        // IVF partitions become slightly stale but search still works (skips missing IDs)
    }

    /// Number of indexed vectors.
    pub fn len(&self) -> usize {
        self.vectors.read().unwrap().len()
    }

    /// Search for the top-k nearest neighbors by cosine similarity.
    /// Uses IVF partitioning when available, falls back to brute-force in memory.
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(i64, f32)> {
        let vecs = self.vectors.read().unwrap();
        if vecs.is_empty() {
            return vec![];
        }

        let centroids = self.centroids.read().unwrap();
        let partitions = self.partition_members.read().unwrap();

        let mut scored: Vec<(i64, f32)> =
            if centroids.is_empty() || vecs.len() < MIN_VECTORS_FOR_IVF {
                // Brute-force over all in-memory vectors
                vecs.iter()
                    .map(|(&id, emb)| (id, cosine_similarity(query, emb)))
                    .collect()
            } else {
                // IVF: search only the nearest partitions
                let probe_indices = nearest_centroids(query, &centroids, IVF_NUM_PROBES);
                let mut seen = HashSet::new();
                let mut results = Vec::new();
                for &ci in &probe_indices {
                    if ci < partitions.len() {
                        for &chunk_id in &partitions[ci] {
                            if seen.insert(chunk_id) {
                                if let Some(emb) = vecs.get(&chunk_id) {
                                    results
                                        .push((chunk_id, cosine_similarity(query, emb)));
                                }
                            }
                        }
                    }
                }
                results
            };

        // Partial sort: O(n) to find top-k, then sort only those
        if scored.len() > top_k {
            scored.select_nth_unstable_by(top_k, |a, b| {
                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
            });
            scored.truncate(top_k);
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Rebuild IVF partitions if enough new vectors have been inserted.
    pub fn rebuild_if_needed(&self) {
        let pending = self.pending_inserts.load(Ordering::Relaxed);
        if pending >= REBUILD_THRESHOLD {
            self.rebuild_ivf();
        }
    }

    /// Force rebuild of IVF partitions using k-means clustering.
    pub fn rebuild_ivf(&self) {
        let vecs = self.vectors.read().unwrap();
        if vecs.len() < MIN_VECTORS_FOR_IVF {
            // Too few vectors — clear partitions and use brute-force
            *self.centroids.write().unwrap() = Vec::new();
            *self.partition_members.write().unwrap() = Vec::new();
            self.pending_inserts.store(0, Ordering::Relaxed);
            return;
        }

        let num_cells = IVF_NUM_CELLS.min(vecs.len() / 4).max(2);

        // Collect (id, embedding_ref) pairs
        let items: Vec<(i64, &Vec<f32>)> =
            vecs.iter().map(|(&id, emb)| (id, emb)).collect();
        let embeddings: Vec<&Vec<f32>> = items.iter().map(|(_, e)| *e).collect();

        // K-means clustering
        let (new_centroids, assignments) = kmeans(&embeddings, num_cells, KMEANS_MAX_ITER);

        // Build partition membership lists
        let mut new_partitions: Vec<Vec<i64>> = vec![Vec::new(); new_centroids.len()];
        for (i, &cluster) in assignments.iter().enumerate() {
            if cluster < new_partitions.len() {
                new_partitions[cluster].push(items[i].0);
            }
        }

        // Store results
        *self.centroids.write().unwrap() = new_centroids;
        *self.partition_members.write().unwrap() = new_partitions;
        self.pending_inserts.store(0, Ordering::Relaxed);

        log::info!(
            "VectorIndex IVF rebuilt: {} vectors across {} partitions",
            vecs.len(),
            num_cells
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// K-Means Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple k-means clustering. Returns (centroids, per-item assignments).
fn kmeans(embeddings: &[&Vec<f32>], k: usize, max_iter: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
    if embeddings.is_empty() || k == 0 {
        return (vec![], vec![]);
    }

    let k = k.min(embeddings.len());
    let dim = embeddings[0].len();

    // Initialize centroids evenly spread across the data
    let step = (embeddings.len() / k).max(1);
    let mut centroids: Vec<Vec<f32>> = (0..k)
        .map(|i| embeddings[(i * step).min(embeddings.len() - 1)].clone())
        .collect();

    let mut assignments = vec![0usize; embeddings.len()];

    for _ in 0..max_iter {
        // Assignment step
        let mut changed = false;
        for (i, emb) in embeddings.iter().enumerate() {
            let mut best = 0;
            let mut best_sim = f32::NEG_INFINITY;
            for (c, centroid) in centroids.iter().enumerate() {
                let sim = cosine_similarity(emb, centroid);
                if sim > best_sim {
                    best_sim = sim;
                    best = c;
                }
            }
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Update step
        let mut sums: Vec<Vec<f32>> = vec![vec![0.0; dim]; k];
        let mut counts = vec![0usize; k];
        for (i, emb) in embeddings.iter().enumerate() {
            let c = assignments[i];
            counts[c] += 1;
            for (d, val) in emb.iter().enumerate() {
                sums[c][d] += val;
            }
        }

        for c in 0..k {
            if counts[c] > 0 {
                for d in 0..dim {
                    centroids[c][d] = sums[c][d] / counts[c] as f32;
                }
            }
        }
    }

    (centroids, assignments)
}

/// Find the `n` nearest centroids to a query vector.
fn nearest_centroids(query: &[f32], centroids: &[Vec<f32>], n: usize) -> Vec<usize> {
    let mut scored: Vec<(usize, f32)> = centroids
        .iter()
        .enumerate()
        .map(|(i, c)| (i, cosine_similarity(query, c)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.iter().take(n).map(|(i, _)| *i).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_index() {
        let idx = VectorIndex::new();
        assert_eq!(idx.len(), 0);
        let results = idx.search(&[1.0, 0.0, 0.0], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_insert_and_search() {
        let idx = VectorIndex::new();
        idx.insert(1, vec![1.0, 0.0, 0.0]);
        idx.insert(2, vec![0.0, 1.0, 0.0]);
        idx.insert(3, vec![0.9, 0.1, 0.0]);

        let results = idx.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1); // Exact match first
        assert_eq!(results[1].0, 3); // Near match second
    }

    #[test]
    fn test_remove() {
        let idx = VectorIndex::new();
        idx.insert(1, vec![1.0, 0.0, 0.0]);
        idx.insert(2, vec![0.0, 1.0, 0.0]);
        assert_eq!(idx.len(), 2);

        idx.remove(1);
        assert_eq!(idx.len(), 1);

        let results = idx.search(&[1.0, 0.0, 0.0], 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 2);
    }

    #[test]
    fn test_ivf_rebuild() {
        let idx = VectorIndex::new();
        // Insert enough vectors to trigger IVF
        for i in 0..200i64 {
            let angle = (i as f32) * std::f32::consts::PI / 100.0;
            idx.insert(i, vec![angle.cos(), angle.sin(), 0.0]);
        }

        idx.rebuild_ivf();

        // Search should still find correct nearest neighbors
        let results = idx.search(&[1.0, 0.0, 0.0], 3);
        assert_eq!(results.len(), 3);
        assert!(results[0].1 > 0.9); // Top result should be very similar
    }

    #[test]
    fn test_rebuild_if_needed() {
        let idx = VectorIndex::new();
        // Below threshold — should not rebuild
        for i in 0..50i64 {
            idx.insert(i, vec![1.0, 0.0, 0.0]);
        }
        idx.rebuild_if_needed();
        assert!(idx.centroids.read().unwrap().is_empty()); // Too few for IVF

        // Above both thresholds
        for i in 50..250i64 {
            idx.insert(i, vec![0.0, 1.0, 0.0]);
        }
        idx.rebuild_if_needed();
        assert!(!idx.centroids.read().unwrap().is_empty()); // IVF built
    }
}
