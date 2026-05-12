//! Lifecycle management: health checks, crash recovery, idle reaping.
//!
//! Runs a background loop that periodically:
//! 1. Health-checks all running instances and increments failure counters
//! 2. Marks unhealthy instances as Error after consecutive failures
//! 3. Reaps idle ephemeral instances
//! 4. Logs memory usage

use crate::process::ProcessManager;
use std::sync::Arc;
use std::time::Duration;

/// Number of consecutive health-check failures before marking an instance as Error.
const HEALTH_FAILURE_THRESHOLD: u8 = 2;

/// Start the lifecycle management loop in a background thread.
/// Uses std::thread to avoid Tokio runtime context issues during Tauri setup.
pub fn start_lifecycle_thread(manager: Arc<ProcessManager>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);
    log::info!(
        "Lifecycle manager started (interval={}s)",
        interval_secs
    );

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);

            tauri::async_runtime::block_on(async {
                // Health-check all running instances before reaping.
                manager.health_check_all(HEALTH_FAILURE_THRESHOLD).await;

                // Reap idle ephemeral instances.
                manager.reap_idle().await;

                // Log memory usage.
                let mem = manager.memory_usage().await;
                if mem > 0 {
                    log::debug!("ProcessManager: estimated memory usage = {mem} MB");
                }
            });
        }
    });
}
