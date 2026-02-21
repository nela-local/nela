//! Example usage of RAPTOR for hierarchical retrieval
//!
//! This module demonstrates how to use RAPTOR in the GenHat RAG pipeline.
//! Run this example after ingesting a document to see RAPTOR in action.

#[cfg(test)]
mod raptor_example {
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Example: Complete RAPTOR workflow
    ///
    /// This demonstrates the full lifecycle of RAPTOR usage:
    /// 1. Ingest a document (Phase 1)
    /// 2. Enrich chunks (Phase 2)
    /// 3. Build RAPTOR tree (Phase 3)
    /// 4. Query with RAPTOR
    #[tokio::test]
    #[ignore] // Requires full environment setup
    async fn example_raptor_workflow() {
        // This is a pseudo-code example showing the intended usage
        // In a real environment, you would:
        
        // 1. Initialize the RAG pipeline (already done in main.rs)
        // let pipeline = RagPipeline::open(&rag_dir, router.clone()).unwrap();
        
        // 2. Ingest a document (Phase 1)
        // let doc_path = PathBuf::from("/path/to/document.pdf");
        // let status = pipeline.ingest_document(&doc_path).await.unwrap();
        // println!("Ingested doc {} with {} chunks", status.doc_id, status.total_chunks);
        
        // 3. Wait for enrichment or trigger manually (Phase 2)
        // let enriched = pipeline.enrich_pending(10).await.unwrap();
        // println!("Enriched {} chunks", enriched);
        
        // 4. Build RAPTOR tree (Phase 3)
        // let tree_status = pipeline.build_raptor_tree(status.doc_id).await.unwrap();
        // println!("Built RAPTOR tree: {} nodes, {} levels", 
        //          tree_status.nodes_created, tree_status.levels);
        
        // 5. Query with RAPTOR
        // let result = pipeline.query_with_raptor(
        //     status.doc_id,
        //     "What are the main findings?",
        //     5
        // ).await.unwrap();
        // 
        // println!("Answer: {}", result.answer);
        // println!("\nSources:");
        // for source in result.sources {
        //     println!("- [{}] {} (score: {:.2})",
        //              source.chunk_id, source.doc_title, source.score);
        //     println!("  {}", &source.text[..100.min(source.text.len())]);
        // }
    }

    /// Example: Direct RAPTOR retrieval
    #[tokio::test]
    #[ignore]
    async fn example_direct_raptor_retrieve() {
        // This shows how to use the low-level RAPTOR retrieval API
        
        // use crate::rag::raptor;
        
        // let results = raptor::raptor_retrieve(
        //     db.clone(),
        //     router.clone(),
        //     doc_id,
        //     "What are the key findings?",
        //     5,  // top_k
        //     Some(-1.5)  // Custom confidence threshold
        // ).await.unwrap();
        //
        // for (chunk_id, score, text) in results {
        //     println!("Chunk {}: {:.2}", chunk_id, score);
        //     println!("  {}", &text[..100.min(text.len())]);
        // }
    }

    /// Example: Building RAPTOR tree with custom parameters
    #[tokio::test]
    #[ignore]
    async fn example_custom_raptor_build() {
        // To customize RAPTOR parameters, modify the constants in raptor.rs:
        //
        // const DEFAULT_CONFIDENCE_THRESHOLD: f64 = -1.5;  // Lower = more expansion
        // const MAX_CLUSTERS_PER_LEVEL: usize = 10;        // More = finer granularity
        // const MIN_CLUSTER_SIZE: usize = 3;               // Higher = fewer small clusters
        // const MAX_TREE_DEPTH: usize = 2;                 // More = deeper hierarchy
        //
        // Then rebuild:
        // let status = pipeline.build_raptor_tree(doc_id).await.unwrap();
    }

    /// Example: Confidence threshold tuning
    #[test]
    fn example_confidence_thresholds() {
        // Different confidence thresholds affect retrieval behavior:
        //
        // Threshold: -2.0 (very low)
        //   - Almost all summaries are used
        //   - Faster retrieval, more concise results
        //   - Risk: may use generic summaries
        //
        // Threshold: -1.5 (default)
        //   - Balanced: expand low-confidence nodes
        //   - Good trade-off for most cases
        //
        // Threshold: -0.5 (high)
        //   - Most nodes expanded to children
        //   - More detailed results
        //   - Slower, uses more tokens
        //
        // Usage:
        // let results = raptor::raptor_retrieve(
        //     db, router, doc_id, query, top_k,
        //     Some(-0.5)  // High threshold
        // ).await.unwrap();
    }

    /// Example: When to use RAPTOR vs standard retrieval
    #[test]
    fn example_use_cases() {
        // Use RAPTOR when:
        // ✓ Document has 50+ chunks (20+ pages)
        // ✓ Asking for summaries ("What is this about?")
        // ✓ Multi-topic documents benefit from clustering
        // ✓ Standard retrieval has low confidence results
        //
        // Use standard retrieval when:
        // ✓ Short documents (< 10 chunks)
        // ✓ Exact fact lookup ("What is the date?")
        // ✓ Need precise citations
        // ✓ Speed is critical
        //
        // Example decision logic:
        // if document.chunk_count > 50 && query.is_summary_question() {
        //     pipeline.query_with_raptor(doc_id, query, top_k).await
        // } else {
        //     pipeline.query(query, top_k).await
        // }
    }
}

/// Frontend integration examples (JavaScript/TypeScript)
#[cfg(test)]
mod frontend_examples {
    /// Example: Building RAPTOR tree from frontend
    ///
    /// ```javascript
    /// import { invoke } from '@tauri-apps/api/core';
    /// 
    /// async function buildRaptorTree(docId) {
    ///   try {
    ///     const status = await invoke('build_raptor_tree', { docId });
    ///     console.log(`RAPTOR tree built: ${status.nodes_created} nodes, ${status.levels} levels`);
    ///     return status;
    ///   } catch (error) {
    ///     console.error('Failed to build RAPTOR tree:', error);
    ///     throw error;
    ///   }
    /// }
    /// ```
    #[test]
    fn example_frontend_build() {}

    /// Example: Querying with RAPTOR from frontend
    ///
    /// ```javascript
    /// async function queryWithRaptor(docId, query) {
    ///   const result = await invoke('query_rag_with_raptor', {
    ///     docId,
    ///     query,
    ///     topK: 5
    ///   });
    ///   
    ///   console.log('Answer:', result.answer);
    ///   console.log('Sources:');
    ///   result.sources.forEach((source, i) => {
    ///     console.log(`  ${i+1}. [${source.doc_title}] (score: ${source.score.toFixed(2)})`);
    ///     console.log(`     ${source.text.substring(0, 100)}...`);
    ///   });
    ///   
    ///   return result;
    /// }
    /// ```
    #[test]
    fn example_frontend_query() {}

    /// Example: Complete document ingestion with RAPTOR
    ///
    /// ```javascript
    /// import { invoke } from '@tauri-apps/api/core';
    /// import { open } from '@tauri-apps/plugin-dialog';
    /// 
    /// async function ingestWithRaptor() {
    ///   // 1. Pick file
    ///   const file = await open({
    ///     filters: [{ name: 'Documents', extensions: ['pdf', 'docx', 'txt'] }]
    ///   });
    ///   
    ///   if (!file) return;
    ///   
    ///   // 2. Ingest document
    ///   console.log('Ingesting document...');
    ///   const doc = await invoke('ingest_document', { path: file });
    ///   console.log(`Ingested: ${doc.title} (${doc.total_chunks} chunks)`);
    ///   
    ///   // 3. Enrich chunks (optional, can run in background)
    ///   console.log('Enriching chunks...');
    ///   await invoke('enrich_rag_documents', { batchSize: 10 });
    ///   
    ///   // 4. Build RAPTOR tree
    ///   console.log('Building RAPTOR tree...');
    ///   const tree = await invoke('build_raptor_tree', { docId: doc.doc_id });
    ///   console.log(`RAPTOR tree: ${tree.nodes_created} nodes, ${tree.levels} levels`);
    ///   
    ///   // 5. Query
    ///   const result = await invoke('query_rag_with_raptor', {
    ///     docId: doc.doc_id,
    ///     query: 'Summarize the main points',
    ///     topK: 5
    ///   });
    ///   
    ///   return { doc, tree, result };
    /// }
    /// ```
    #[test]
    fn example_frontend_workflow() {}

    /// Example: UI integration with progress tracking
    ///
    /// ```javascript
    /// // React component for RAPTOR tree building
    /// function RaptorBuilder({ docId }) {
    ///   const [status, setStatus] = useState('idle');
    ///   const [progress, setProgress] = useState(null);
    ///   
    ///   const buildTree = async () => {
    ///     setStatus('building');
    ///     try {
    ///       const result = await invoke('build_raptor_tree', { docId });
    ///       setProgress(result);
    ///       setStatus('complete');
    ///     } catch (error) {
    ///       setStatus('error');
    ///       console.error(error);
    ///     }
    ///   };
    ///   
    ///   return (
    ///     <div>
    ///       <button onClick={buildTree} disabled={status === 'building'}>
    ///         Build RAPTOR Tree
    ///       </button>
    ///       {status === 'building' && <p>Building tree...</p>}
    ///       {status === 'complete' && (
    ///         <p>✓ Tree built: {progress.nodes_created} nodes, {progress.levels} levels</p>
    ///       )}
    ///     </div>
    ///   );
    /// }
    /// ```
    #[test]
    fn example_frontend_ui() {}
}
