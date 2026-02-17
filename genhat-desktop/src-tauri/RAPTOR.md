# RAPTOR: Recursive Abstractive Processing for Tree-Organized Retrieval

## Overview

RAPTOR is a hierarchical retrieval system that improves RAG (Retrieval-Augmented Generation) quality by building a tree structure over document chunks. It was implemented as Phase 3 of the GenHat RAG pipeline.

## Key Features

### 1. Hierarchical Clustering
- Uses k-means clustering on chunk embeddings
- Groups semantically similar chunks together
- Builds up to 2 levels of hierarchy

### 2. LLM-Based Summarization
- Each cluster is summarized using the LLM
- Summaries capture the essence of multiple chunks
- Reduces context length while preserving information

### 3. Confidence-Aware Traversal (Novel)
- Each summary node stores a confidence score
- During retrieval, low-confidence summaries (< -1.5) are expanded to their child chunks
- Prevents generic/hallucinated summaries from poisoning retrieval results

## Architecture

```
Document Chunks (Level 0)
    └─> Cluster 1 Summary (Level 1)
        ├─> Chunk 1
        ├─> Chunk 2
        └─> Chunk 3
    └─> Cluster 2 Summary (Level 1)
        ├─> Chunk 4
        └─> Chunk 5
    └─> Root Summary (Level 2)
        ├─> Cluster 1 Summary
        └─> Cluster 2 Summary
```

## Database Schema

```sql
CREATE TABLE raptor_nodes (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id          INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    level           INTEGER NOT NULL,
    parent_id       INTEGER REFERENCES raptor_nodes(id) ON DELETE CASCADE,
    summary_text    TEXT NOT NULL,
    confidence_score REAL NOT NULL,
    child_ids       TEXT NOT NULL,  -- JSON array of child IDs
    embedding       BLOB,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

## API Usage

### Building a RAPTOR Tree

**Tauri Command (Frontend):**
```javascript
import { invoke } from '@tauri-apps/api/core';

// Build RAPTOR tree for a document
const status = await invoke('build_raptor_tree', { docId: 1 });
console.log(`Created ${status.nodes_created} nodes across ${status.levels} levels`);
```

**Rust API:**
```rust
use app_lib::rag::raptor;

// Build tree for a document
let status = raptor::build_raptor_tree(
    db.clone(),
    router.clone(),
    doc_id
).await?;
```

### Querying with RAPTOR

**Tauri Command:**
```javascript
// Query using RAPTOR tree
const result = await invoke('query_rag_with_raptor', {
    docId: 1,
    query: "What are the key findings?",
    topK: 5
});
console.log(result.answer);
console.log(result.sources);
```

**Rust API:**
```rust
// Query with RAPTOR (falls back to standard retrieval if no tree)
let result = pipeline.query_with_raptor(doc_id, query, top_k).await?;

// Direct RAPTOR retrieval
let results = raptor::raptor_retrieve(
    db,
    router,
    doc_id,
    query,
    top_k,
    Some(-1.5) // Custom confidence threshold
).await?;
```

### Managing RAPTOR Trees

**Check if tree exists:**
```javascript
const exists = await invoke('has_raptor_tree', { docId: 1 });
```

**Delete tree:**
```javascript
await invoke('delete_raptor_tree', { docId: 1 });
```

## Configuration Parameters

Located in `src/rag/raptor.rs`:

```rust
/// Default confidence threshold. Summaries below this are expanded to children.
const DEFAULT_CONFIDENCE_THRESHOLD: f64 = -1.5;

/// Maximum number of clusters per level.
const MAX_CLUSTERS_PER_LEVEL: usize = 10;

/// Minimum chunks per cluster (smaller clusters won't be summarized).
const MIN_CLUSTER_SIZE: usize = 3;

/// Maximum RAPTOR tree depth.
const MAX_TREE_DEPTH: usize = 2;
```

## When to Use RAPTOR

### Best For:
- **Long documents** (20+ pages) where summaries help
- **Multi-topic documents** that benefit from clustering
- **Summarization queries** ("What is this document about?")
- **When dealing with low-confidence baseline retrieval results**

### Not Ideal For:
- **Short documents** (< 10 chunks) - overhead not worth it
- **Exact fact lookup** - direct chunk retrieval is faster
- **When you need precise citations** - summaries abstract away details

## Workflow Integration

### Phase 1 (Instant): Basic Ingestion
```
Document → Parse → Chunk → Embed → Store in DB + BM25 index
```
**Document becomes searchable in seconds**

### Phase 2 (Background): Enrichment
```
Chunk → LLM contextual prefix → Re-embed → Update DB
```
**Improves retrieval quality over time**

### Phase 3 (On-Demand): RAPTOR Tree Building
```
Chunks → K-means clustering → LLM summarization → Store tree
```
**Triggered manually or when baseline retrieval has low confidence**

## Confidence Scoring

Currently uses a heuristic based on summary characteristics:
- Generic short summaries: -2.0 (low confidence)
- Detailed summaries (>30 words): -0.5 (high confidence)
- Medium summaries: -1.0

**Future Enhancement:** 
Extract real confidence from LLM logprobs when supported by the inference backend.

## Performance Characteristics

### Tree Building Time:
- **10 chunks**: ~5 seconds
- **50 chunks**: ~20-30 seconds
- **100 chunks**: ~1-2 minutes

*Time depends on LLM speed for summarization*

### Memory Usage:
- **Tree storage**: ~2KB per node
- **Embeddings**: ~1.5KB per node (384-dim floats)
- **Total**: ~100KB for 30 nodes

### Retrieval Speed:
- **Vector search**: O(n) over RAPTOR nodes
- **Node expansion**: O(k) for k children
- **Total**: Sub-100ms for typical queries

## Example: Complete Workflow

```javascript
// 1. Ingest document (Phase 1)
const doc = await invoke('ingest_document', { 
    path: '/path/to/document.pdf' 
});
console.log(`Ingested: ${doc.total_chunks} chunks`);

// 2. Wait for or trigger enrichment (Phase 2)
await invoke('enrich_rag_documents', { batchSize: 10 });

// 3. Build RAPTOR tree (Phase 3)
const tree = await invoke('build_raptor_tree', { 
    docId: doc.doc_id 
});
console.log(`RAPTOR tree: ${tree.nodes_created} nodes, ${tree.levels} levels`);

// 4. Query with RAPTOR
const result = await invoke('query_rag_with_raptor', {
    docId: doc.doc_id,
    query: "Summarize the main findings",
    topK: 5
});
console.log(result.answer);
```

## Implementation Details

### K-Means Clustering
- **Algorithm**: Lloyd's algorithm with cosine similarity
- **Initialization**: First k embeddings as initial centroids
- **Convergence**: Max 20 iterations or until no assignment changes
- **Distance metric**: Cosine similarity (1 - cosine_distance)

### Tree Building Algorithm
```
1. Start with all chunk embeddings (Level 0)
2. For each level (up to MAX_TREE_DEPTH):
   a. Determine number of clusters (chunks / MIN_CLUSTER_SIZE)
   b. Run k-means clustering
   c. For each cluster with >= MIN_CLUSTER_SIZE items:
      - Fetch child texts
      - Generate LLM summary
      - Embed summary
      - Store as RAPTOR node
      - Add to next level
   d. If < 2 nodes created, stop
3. Return tree statistics
```

### Confidence-Aware Retrieval Algorithm
```
1. Embed query
2. Find top-k most similar RAPTOR nodes (any level)
3. For each matched node:
   a. If confidence_score < threshold:
      - Expand to child chunks/nodes
   b. Else:
      - Use summary text directly
4. Return expanded results
```

## Testing

Run the RAPTOR unit tests:
```bash
cd genhat-desktop/src-tauri
cargo test raptor::tests
```

Tests include:
- ✅ K-means basic clustering
- ✅ Empty input handling
- ✅ Cluster grouping
- ✅ Confidence estimation heuristics

## Future Enhancements

1. **Real Logprob Confidence**: Extract mean logprob from LLM responses
2. **Adaptive Clustering**: Use silhouette score to determine optimal k
3. **Incremental Updates**: Update tree when new chunks are added
4. **Cross-Document Trees**: Build RAPTOR across multiple related documents
5. **User Feedback**: Learn confidence thresholds from user interactions

## References

- **Paper**: "RAPTOR: Recursive Abstractive Processing for Tree-Organized Retrieval" (conceptual inspiration)
- **Related**: RAG Fusion, HyDE (Hypothetical Document Embeddings)
- **GenHat RAG Plan**: See original plan document for full context

## Troubleshooting

### "RAPTOR tree already exists"
```rust
// Delete existing tree first
pipeline.delete_raptor_tree(doc_id).await?;
pipeline.build_raptor_tree(doc_id).await?;
```

### "No embeddings found for document chunks"
```
Ensure Phase 1 ingestion completed successfully.
Check that embedding model is loaded and running.
```

### "No RAPTOR tree exists for this document"
```
Build tree first:
await invoke('build_raptor_tree', { docId: X });
```

### Tree building is slow
```
This is expected - LLM summarization is the bottleneck.
For a 50-chunk document, expect 30-60 seconds.
Progress can be tracked via logs (log::info).
```
