//! Playground scheduler — registers cron jobs for pipelines with auto_resume = true.

use super::executor::run_pipeline;
use super::store::PipelineStore;
use crate::router::TaskRouter;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Start the scheduler. Called once on app startup from main.rs.
///
/// Loads all pipelines with `auto_resume = true` and registers cron jobs.
pub async fn start_scheduler(
    store: Arc<PipelineStore>,
    router: Arc<TaskRouter>,
    app_data_dir: PathBuf,
    app_handle: tauri::AppHandle,
) {
    let sched = match JobScheduler::new().await {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to create job scheduler: {e}");
            return;
        }
    };

    let pipelines = match store.list_auto_resume() {
        Ok(ps) => ps,
        Err(e) => {
            log::warn!("Failed to load auto-resume pipelines: {e}");
            return;
        }
    };

    let count = pipelines.len();

    for pipeline in pipelines {
        // Extract cron from the first Schedule node
        let cron = pipeline
            .nodes
            .iter()
            .find(|n| n.data.kind == "Schedule")
            .and_then(|n| {
                n.data
                    .config
                    .get("cron")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

        let cron = match cron {
            Some(c) if !c.is_empty() => c,
            _ => {
                log::warn!(
                    "Pipeline '{}' has auto_resume but no Schedule node with a cron — skipping",
                    pipeline.name
                );
                continue;
            }
        };

        let pipeline_clone = pipeline.clone();
        let router_clone = router.clone();
        let data_dir_clone = app_data_dir.clone();
        let app_handle_clone = app_handle.clone();

        match Job::new_async(cron.as_str(), move |_uuid, _lock| {
            let p = pipeline_clone.clone();
            let r = router_clone.clone();
            let d = data_dir_clone.clone();
            let h = app_handle_clone.clone();
            Box::pin(async move {
                log::info!("[Scheduler] Running pipeline '{}'", p.name);
                // Scheduled runs are not interactively cancellable; create a dummy receiver.
                let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                let result = run_pipeline(&p, r, d, h, cancel_rx).await;
                log::info!(
                    "[Scheduler] Pipeline '{}' finished: {:?}",
                    p.name,
                    result.status
                );
            })
        }) {
            Ok(job) => {
                if let Err(e) = sched.add(job).await {
                    log::warn!("Failed to schedule pipeline '{}': {e}", pipeline.name);
                }
            }
            Err(e) => {
                log::warn!(
                    "Invalid cron '{}' for pipeline '{}': {e}",
                    cron,
                    pipeline.name
                );
            }
        }
    }

    if let Err(e) = sched.start().await {
        log::error!("Scheduler failed to start: {e}");
        return;
    }

    log::info!("[Playground] Scheduler started — {count} auto-resume pipeline(s) registered");

    // Keep the scheduler alive (do not drop it)
    std::mem::forget(sched);
}
