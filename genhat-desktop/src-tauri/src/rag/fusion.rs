//! Reciprocal Rank Fusion (RRF) for combining BM25 and vector rankings.
//!
//! Given multiple ranked lists of (chunk_id, score), RRF merges them
//! into a single ranking using: RRF_score(d) = Σ 1 / (k + rank(d))
//!
//! k = 60 is the standard constant that prevents high-ranked items
//! from dominating excessively.

use std::collections::HashMap;

/// Default RRF constant (standard in literature).
const DEFAULT_K: f64 = 60.0;

/// A single scored result.
#[derive(Debug, Clone)]
pub struct FusedResult {
    pub chunk_id: i64,
    pub rrf_score: f64,
}

/// Fuse multiple ranked lists using Reciprocal Rank Fusion.
///
/// Each input list should be sorted by relevance (best first).
/// Returns chunk IDs sorted by descending RRF score.
pub fn rrf_fuse(rankings: &[Vec<(i64, f32)>]) -> Vec<FusedResult> {
    rrf_fuse_with_k(rankings, DEFAULT_K)
}

/// RRF with a custom k constant.
pub fn rrf_fuse_with_k(rankings: &[Vec<(i64, f32)>], k: f64) -> Vec<FusedResult> {
    let mut scores: HashMap<i64, f64> = HashMap::new();

    for ranked_list in rankings {
        for (rank, (chunk_id, _score)) in ranked_list.iter().enumerate() {
            let rrf_contribution = 1.0 / (k + (rank as f64) + 1.0);
            *scores.entry(*chunk_id).or_insert(0.0) += rrf_contribution;
        }
    }

    let mut results: Vec<FusedResult> = scores
        .into_iter()
        .map(|(chunk_id, rrf_score)| FusedResult {
            chunk_id,
            rrf_score,
        })
        .collect();

    // Sort descending by RRF score
    results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap());
    results
}

/// Weighted RRF: each ranking list gets a weight multiplier.
///
/// Useful for boosting vector results over BM25 or vice versa
/// depending on query classification.
pub fn weighted_rrf_fuse(
    rankings: &[(Vec<(i64, f32)>, f64)], // (ranked_list, weight)
    k: f64,
) -> Vec<FusedResult> {
    let mut scores: HashMap<i64, f64> = HashMap::new();

    for (ranked_list, weight) in rankings {
        for (rank, (chunk_id, _score)) in ranked_list.iter().enumerate() {
            let rrf_contribution = weight / (k + (rank as f64) + 1.0);
            *scores.entry(*chunk_id).or_insert(0.0) += rrf_contribution;
        }
    }

    let mut results: Vec<FusedResult> = scores
        .into_iter()
        .map(|(chunk_id, rrf_score)| FusedResult {
            chunk_id,
            rrf_score,
        })
        .collect();

    results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap());
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_basic() {
        let bm25 = vec![(1, 5.0f32), (2, 3.0), (3, 1.0)];
        let vec_search = vec![(2, 0.9f32), (3, 0.8), (1, 0.5)];

        let fused = rrf_fuse(&[bm25, vec_search]);

        // chunk 2 appears rank 1 in both → highest RRF
        assert_eq!(fused[0].chunk_id, 2);
        // All 3 chunks present
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn test_rrf_empty() {
        let fused = rrf_fuse(&[]);
        assert!(fused.is_empty());
    }

    #[test]
    fn test_weighted_rrf() {
        let bm25 = vec![(1, 5.0f32)];
        let vec_search = vec![(2, 0.9f32)];

        // Weight vector search 2x higher
        let fused = weighted_rrf_fuse(
            &[(bm25, 1.0), (vec_search, 2.0)],
            DEFAULT_K,
        );

        // Chunk 2 (vector result at rank 0 with weight 2) should score higher
        // than chunk 1 (BM25 at rank 0 with weight 1)
        assert_eq!(fused[0].chunk_id, 2);
    }
}
