//! Pipeline executor — runs a pipeline sequentially, node by node.

use super::nodes::{execute_node, RunContext};
use super::types::{Pipeline, PipelineRun, NodeRunState, RunStatus};
use crate::router::TaskRouter;
use std::collections::HashMap;
use std::sync::Arc;
use std::path::PathBuf;
use tauri::Emitter;
use uuid::Uuid;

/// Execute a pipeline to completion, emitting `playground-run-update` events via
/// `app_handle` after each node so the frontend can show live progress without
/// waiting for the full run to finish.
pub async fn run_pipeline(
    pipeline: &Pipeline,
    router: Arc<TaskRouter>,
    app_data_dir: PathBuf,
    app_handle: tauri::AppHandle,
) -> PipelineRun {
    let run_id = Uuid::new_v4().to_string();
    let mut node_states: HashMap<String, NodeRunState> = HashMap::new();
    let mut log: Vec<String> = Vec::new();

    // Build adjacency for topological order
    let ordered = topological_order(pipeline);

    let mut ctx = RunContext::new();

    let mut final_status = RunStatus::Success;

    for node_id in &ordered {
        let node = match pipeline.nodes.iter().find(|n| &n.id == node_id) {
            Some(n) => n,
            None => continue,
        };

        log.push(format!("[{}] Starting node '{}'", node.data.kind, node.data.label));

        node_states.insert(
            node_id.clone(),
            NodeRunState {
                node_id: node_id.clone(),
                status: RunStatus::Running,
                output: None,
                error: None,
            },
        );

        // Emit "node running" snapshot
        emit_update(&app_handle, &run_id, pipeline, &node_states, RunStatus::Running, &log);

        match execute_node(node, &mut ctx, &router, &app_data_dir).await {
            Ok(output) => {
                log.push(format!(
                    "[{}] '{}' OK ({} chars)",
                    node.data.kind,
                    node.data.label,
                    output.len()
                ));
                ctx.output = output.clone();
                node_states.insert(
                    node_id.clone(),
                    NodeRunState {
                        node_id: node_id.clone(),
                        status: RunStatus::Success,
                        output: Some(output),
                        error: None,
                    },
                );
            }
            Err(e) => {
                let msg = format!("{:#}", e);
                log.push(format!(
                    "[{}] '{}' ERROR: {}",
                    node.data.kind, node.data.label, msg
                ));
                node_states.insert(
                    node_id.clone(),
                    NodeRunState {
                        node_id: node_id.clone(),
                        status: RunStatus::Error,
                        output: None,
                        error: Some(msg),
                    },
                );
                final_status = RunStatus::Error;
                break; // stop on first error
            }
        }

        // Emit post-node snapshot (success or error)
        emit_update(&app_handle, &run_id, pipeline, &node_states, final_status.clone(), &log);
    }

    let run = PipelineRun {
        id: run_id,
        pipeline_id: pipeline.id.clone(),
        status: final_status,
        node_states,
        log,
    };

    // Emit final completion event
    let _ = app_handle.emit("playground-run-complete", &run);

    run
}

fn emit_update(
    app_handle: &tauri::AppHandle,
    run_id: &str,
    pipeline: &Pipeline,
    node_states: &HashMap<String, NodeRunState>,
    status: RunStatus,
    log: &[String],
) {
    let snapshot = PipelineRun {
        id: run_id.to_string(),
        pipeline_id: pipeline.id.clone(),
        status,
        node_states: node_states.clone(),
        log: log.to_vec(),
    };
    let _ = app_handle.emit("playground-run-update", &snapshot);
}

/// Very simple topological sort: follow source→target edges with BFS from roots.
fn topological_order(pipeline: &Pipeline) -> Vec<String> {
    let all_ids: std::collections::HashSet<&str> =
        pipeline.nodes.iter().map(|n| n.id.as_str()).collect();
    let has_incoming: std::collections::HashSet<&str> =
        pipeline.edges.iter().map(|e| e.target.as_str()).collect();

    // Roots = nodes with no incoming edges
    let mut queue: std::collections::VecDeque<&str> = pipeline
        .nodes
        .iter()
        .filter(|n| !has_incoming.contains(n.id.as_str()))
        .map(|n| n.id.as_str())
        .collect();

    // Build adjacency list
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &pipeline.edges {
        adj.entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
    }

    let mut visited = std::collections::HashSet::new();
    let mut order = Vec::new();

    while let Some(id) = queue.pop_front() {
        if visited.contains(id) {
            continue;
        }
        if all_ids.contains(id) {
            order.push(id.to_string());
            visited.insert(id);
        }
        if let Some(children) = adj.get(id) {
            for child in children {
                queue.push_back(child);
            }
        }
    }

    order
}
