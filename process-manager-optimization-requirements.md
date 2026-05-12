# ProcessManager Optimization — Implementation Requirements

> **Purpose**: This document is a self-contained specification for reproducing the ProcessManager efficiency improvements on the NELA `genhat-desktop` Rust backend. Apply these changes to a fresh checkout of the `main` branch. The target environment is 8–16 GB RAM, 4–8 CPU cores, no dedicated GPU. The goal is to eliminate podcast studio cold-start delays (_previously 5–30 s per stage × 3 stages_), prevent memory-budget hard failures, remove unnecessary serialization in TTS, and give interactive requests priority over background work.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Phase 1 — Eviction-Based Memory + Pipeline Reservations + TTS Mutex Removal](#2-phase-1)
3. [Phase 2 — Yield-on-Demand Priority + Enrichment Throttle](#3-phase-2)
4. [Phase 3 — Adaptive Health Poll, Persistent HTTP Client, Incremental ctx_size, Lifecycle Hardening](#4-phase-3)
5. [File-by-File Change Specification](#5-file-by-file-change-specification)
6. [Validation](#6-validation)

---

## 1. Architecture Overview

The Rust backend lives under `genhat-desktop/src-tauri/src/`. Key modules:

| Module | Path | Role |
|--------|------|------|
| ProcessManager | `process/mod.rs` | Central orchestrator — instance lifecycle, memory budget, eviction |
| Pool | `process/pool.rs` | Priority classification, yield constants |
| Lifecycle | `process/lifecycle.rs` | Background health-check loop, idle reaping |
| Router | `router/mod.rs` | Routes `TaskRequest` → model → backend |
| Podcast Engine | `podcast/engine.rs` | Multi-stage podcast pipeline (RAG → Script → TTS → Merge) |
| RAG Pipeline | `rag/pipeline.rs` | Background document enrichment (`enrich_pending`) |
| LlamaServer Backend | `backends/llama_server.rs` | Spawns `llama-server` child processes, HTTP API |
| LlamaCli Backend | `backends/llama_cli.rs` | CLI-based inference backend |
| TTS Inference | `tts/inference.rs` | KittenTTS ONNX engine |
| Registry Types | `registry/types.rs` | Core type definitions (`ProcessHandle`, `ManagedInstance`, `ManagedModel`, etc.) |

---

## 2. Phase 1 — Eviction-Based Memory + Pipeline Reservations + TTS Mutex Removal

### 2.1 Eviction-Based Memory Manager (`process/mod.rs`)

**Problem**: When memory budget is exceeded, `ensure_running()` hard-fails with an error instead of making room.

**Solution**: Replace the hard budget check with an `evict_until_fits()` method that evicts idle/stale models in LRU order across multiple passes of increasing aggressiveness.

#### Requirements:

1. **Add new fields to `ProcessManager` struct**:
   - `reservations: Arc<RwLock<Vec<PipelineReservation>>>` — active pipeline reservations
   - `recent_user_activity: Arc<AtomicBool>` — set by user-facing tasks
   - `last_user_activity_epoch: Arc<AtomicU64>` — epoch timestamp of last high-priority request

2. **Add new imports** to `process/mod.rs`:
   - `std::sync::atomic::{AtomicBool, AtomicU64, Ordering}`
   - `std::time::{Duration, Instant}`

3. **Add `PipelineReservation` struct** before `ProcessManager`:
   ```rust
   #[derive(Debug, Clone)]
   pub struct PipelineReservation {
       pub id: String,
       pub model_ids: Vec<String>,
       pub created_at: Instant,
       pub ttl: Duration,
   }
   
   impl PipelineReservation {
       pub fn is_expired(&self) -> bool {
           self.created_at.elapsed() > self.ttl
       }
   }
   ```

4. **Initialize new fields in `ProcessManager::new()`**:
   ```rust
   reservations: Arc::new(RwLock::new(Vec::new())),
   recent_user_activity: Arc::new(AtomicBool::new(false)),
   last_user_activity_epoch: Arc::new(AtomicU64::new(0)),
   ```

5. **Replace the memory budget check in `ensure_running()`**: In the slow path (write lock section), instead of returning an error when `current_usage + needed_mb > memory_budget_mb`, do:
   ```rust
   if self.memory_budget_mb > 0 {
       let current_usage = self.current_memory_usage_internal(&models);
       if current_usage + needed_mb > self.memory_budget_mb {
           // Drop write lock, run eviction, re-acquire
           drop(models);
           self.evict_until_fits(needed_mb, model_id).await?;
           models = self.models.write().await;
       }
   }
   ```

6. **Add `evict_until_fits()` method**: Multi-pass eviction with these pass types in order: `"error"`, `"ephemeral_expired"`, `"ephemeral_preempt"`, `"persistent_lru"`, `"previous_llm"`. Each pass calls `collect_eviction_candidates()`. Models with `active_requests > 0`, the currently-active LLM, the requesting model, or models in active reservations are NEVER evicted. When enough memory is freed, stop. If all passes exhausted and not enough freed, return an error.

7. **Add `collect_eviction_candidates()` method**: Takes a pass name and returns `Vec<(model_id, memory_mb)>` sorted by oldest activity. Pass logic:
   - `"error"`: instances in `ModelStatus::Error(_)`
   - `"ephemeral_expired"`: ephemeral + idle timeout exceeded
   - `"ephemeral_preempt"`: ephemeral + no active requests
   - `"persistent_lru"`: persistent models idle ≥60s, excluding `previous_llm`
   - `"previous_llm"`: the previously-active LLM (last resort)

8. **Add helper methods**:
   - `has_free_instance(model_id) -> bool`: returns true if any instance is Ready with `active_requests == 0`
   - `instance_count(model_id) -> u32`: returns the number of instances for a model
   - `find_model_for_task(task) -> Option<String>`: wraps `find_models_for_task` returning first match

9. **Update module doc comment** to describe eviction-based memory management, pipeline reservations, and yield-on-demand priority.

### 2.2 Pipeline Reservations (`process/mod.rs` + `podcast/engine.rs`)

**Problem**: Podcast generation cold-starts 3 models sequentially (embed → LLM → TTS), each taking 5–30s.

**Solution**: Add a reservation system that pre-warms all needed models in parallel before the pipeline starts, and pins them from eviction.

#### `process/mod.rs` requirements:

1. **Add `reserve_pipeline()` method**:
   - Takes `model_ids: &[&str]` and `ttl: Duration`
   - Generates a UUID for the reservation
   - Calculates total memory needed for models not already running
   - Calls `evict_until_fits()` if budget would be exceeded
   - Registers the reservation in `self.reservations` BEFORE pre-warming
   - Pre-warms all models in parallel using `futures_util::future::join_all`
   - Logs warnings for models that fail to warm but doesn't fail the reservation
   - Returns `PipelineReservation`

2. **Add `release_pipeline()` method**: Removes the reservation by ID from the list.

3. **Add `reserved_model_ids()` method**: Returns all model IDs across all active (non-expired) reservations. Also garbage-collects expired reservations.

#### `podcast/engine.rs` requirements:

1. **Wrap `generate_podcast()` with reservation logic**: Before the inner pipeline logic:
   - Determine which models the pipeline needs: embed model, script/PodcastScript model, TTS model (using `pm.find_model_for_task()`)
   - Deduplicate the model list
   - Call `pm.reserve_pipeline(&model_ids, Duration::from_secs(600))` — 10 min TTL
   - If reservation fails, log a warning and proceed without pre-warming
   - Emit a `"warmup"` progress event

2. **Extract inner logic to `generate_podcast_inner()`**: Move all existing generation logic into a separate `async fn generate_podcast_inner(...)` function.

3. **In `generate_podcast()` after inner completes**: Call `pm.release_pipeline()` to release the reservation, regardless of success or failure of the inner function.

### 2.3 Remove KittenTTS Mutex (`tts/inference.rs`)

**Problem**: The ONNX `Session` is wrapped in `Mutex<Session>`, serializing all TTS inference. In `ort` 2.x, `Session::run()` takes `&self` (thread-safe), so the Mutex is unnecessary overhead.

**Solution**:

1. **Change `session` field type** from `Mutex<Session>` to `Session` (bare).
2. **Remove `use std::sync::Mutex;`** import.
3. **In `generate_single_chunk()`**: Change `self.session.lock().unwrap().run(...)` to `self.session.run(...)`.
4. **Update doc comment** to explain why no Mutex is needed (ort 2.x `Session::run()` takes `&self`).
5. **In `KittenTtsEngine::load()`**: Store the session directly without wrapping in `Mutex::new()`.

---

## 3. Phase 2 — Yield-on-Demand Priority + Enrichment Throttle

### 3.1 Pool Priority Classification (`process/pool.rs`)

**Problem**: No distinction between user-facing and background tasks.

**Solution**: Add priority helpers and yield constants.

1. **Add `is_high_priority(task: &TaskType) -> bool`** function: Returns true if `task_priority(task) == TaskPriority::High`.

2. **Add yield constants**:
   ```rust
   pub const LOW_PRIORITY_YIELD_RETRIES: u32 = 3;
   pub const LOW_PRIORITY_YIELD_MS: u64 = 500;
   ```

3. **Update module doc comment** to describe yield-on-demand behavior.

### 3.2 Router Yield-on-Demand (`router/mod.rs`)

**Problem**: Background enrichment tasks compete with user chat on the same model instances.

**Solution**: In `route()`, high-priority tasks mark user activity; low-priority tasks yield when the user is active and the model is at capacity.

1. **At the start of `route()`** (after resolving candidates):
   ```rust
   let is_high = pool::is_high_priority(&request.task_type);
   if is_high {
       self.process_manager.mark_user_activity();
   }
   ```

2. **Add yield loop for low-priority tasks** (after the `is_high` block, before the candidate iteration):
   ```rust
   if !is_high && self.process_manager.is_user_active() {
       for retry in 0..pool::LOW_PRIORITY_YIELD_RETRIES {
           let has_free_slot = self.any_candidate_has_free_slot(&candidates).await;
           if has_free_slot { break; }
           log::debug!(
               "Low-priority task '{}' yielding (retry {}/{}) — user is active",
               request.task_type, retry + 1, pool::LOW_PRIORITY_YIELD_RETRIES
           );
           tokio::time::sleep(Duration::from_millis(pool::LOW_PRIORITY_YIELD_MS)).await;
       }
   }
   ```

3. **Add `any_candidate_has_free_slot()` helper** to the router:
   ```rust
   async fn any_candidate_has_free_slot(&self, candidates: &[String]) -> bool {
       for model_id in candidates {
           if self.process_manager.has_free_instance(model_id).await {
               return true;
           }
       }
       false
   }
   ```

4. **Update module doc comment** to mention yield-on-demand and user activity tracking.

### 3.3 User Activity Tracking (`process/mod.rs`)

1. **Add `mark_user_activity()` method**:
   - Sets `recent_user_activity` to `true` (Ordering::Release)
   - Stores current epoch seconds in `last_user_activity_epoch`

2. **Add `is_user_active() -> bool` method**:
   - Returns `false` if `recent_user_activity` flag is not set
   - Checks if `last_user_activity_epoch` is within 30 seconds of now
   - Auto-clears the flag if more than 30s have elapsed
   - Returns `true` only if flag is set AND within 30s window

### 3.4 Enrichment Activity Throttle (`rag/pipeline.rs`)

**Problem**: Background `enrich_pending()` sends a burst of LLM requests that compete with user chat.

**Solution**: Before each chunk in the enrichment loop, check if the user is active and pause if so.

1. **At the top of the per-chunk loop** in `enrich_pending()`, before creating the enrich request:
   ```rust
   if self.router.process_manager.is_user_active() {
       log::debug!("Enrichment: user active, throttling (2s pause)");
       tokio::time::sleep(std::time::Duration::from_secs(2)).await;
   }
   ```

---

## 4. Phase 3 — Adaptive Health Poll, Persistent HTTP Client, Incremental ctx_size, Lifecycle Hardening

### 4.1 Adaptive Health Poll (`backends/llama_server.rs`)

**Problem**: `wait_for_ready()` polls at a fixed interval (e.g., 250ms), wasting time for fast-starting models.

**Solution**: Use adaptive backoff based on elapsed time:

1. **Replace the fixed sleep** in the poll loop of `wait_for_ready()` with:
   ```rust
   let elapsed_ms = start_time.elapsed().as_millis() as u64;
   let poll_ms = if elapsed_ms < 2000 {
       50       // first 2s: poll every 50ms
   } else if elapsed_ms < 5000 {
       100      // 2-5s: every 100ms
   } else if elapsed_ms < 15000 {
       200      // 5-15s: every 200ms
   } else {
       500      // 15s+: every 500ms
   };
   tokio::time::sleep(Duration::from_millis(poll_ms)).await;
   ```

### 4.2 Persistent HTTP Client (`backends/llama_server.rs` + `registry/types.rs`)

**Problem**: Every `execute()` call creates a new `reqwest::Client`, which means a new connection pool and TCP handshake each time.

**Solution**: Create one `reqwest::Client` per llama-server instance at startup and reuse it for all requests.

#### `registry/types.rs`:

1. **Add field to `ProcessHandle`**:
   ```rust
   /// Persistent HTTP client for this instance (connection pooling + keep-alive).
   pub http_client: Option<reqwest::Client>,
   ```

#### `backends/llama_server.rs`:

1. **In `start()`**, create the persistent client before constructing `ProcessHandle`:
   ```rust
   let persistent_client = reqwest::Client::builder()
       .pool_max_idle_per_host(2)
       .tcp_keepalive(std::time::Duration::from_secs(30))
       .build()
       .ok();
   ```
   Then set `http_client: persistent_client` on the `ProcessHandle`.

2. **In `execute()`**, extract both `port` and `persistent_client` from the handle:
   ```rust
   let (port, persistent_client) = match handle {
       Process(ph) => (
           ph.port.ok_or("llama-server has no port assigned")?,
           ph.http_client.clone(),
       ),
       _ => return Err("LlamaServerBackend requires a ProcessHandle".into()),
   };
   ```

3. **Pass `persistent_client` to helper methods**: Update calls to `execute_classification` and `execute_embedding` to pass `persistent_client.as_ref()`.

4. **In the main execute body**, use the persistent client with per-request timeout:
   ```rust
   let client = persistent_client.unwrap_or_else(|| http_client_with_timeout(timeout_secs).unwrap());
   let resp = client
       .post(&url)
       .json(&body)
       .timeout(std::time::Duration::from_secs(timeout_secs))
       .send()
       // ... rest of error handling unchanged
   ```

5. **Update `execute_classification` signature** to accept `persistent_client: Option<&reqwest::Client>`:
   ```rust
   async fn execute_classification(
       &self, port: u16, request: &TaskRequest,
       persistent_client: Option<&reqwest::Client>,
   ) -> ...
   ```
   Then: `let client = persistent_client.cloned().unwrap_or_else(|| http_client_with_timeout(15).unwrap());` and add `.timeout(Duration::from_secs(15))` to the request builder.

6. **Update `execute_embedding` signature** similarly with `persistent_client: Option<&reqwest::Client>`:
   Use `persistent_client.cloned().unwrap_or_else(|| http_client_with_timeout(180).unwrap())` and add `.timeout(Duration::from_secs(180))` to the request builder.

#### `backends/llama_cli.rs`:

1. **Add `http_client: None`** to the `ProcessHandle` constructor. The CLI backend doesn't use HTTP.

### 4.3 Incremental ctx_size Resize (`process/mod.rs`)

**Problem**: `ensure_ctx_size()` stops ALL instances to resize, causing a full cold restart even when multiple instances exist.

**Solution**: Rolling restart — stop instances one-by-one when multiple exist.

1. **After updating the `ctx_size` param in the model def**, check instance count:
   ```rust
   let instance_count = {
       let models = self.models.read().await;
       models.get(model_id).map(|m| m.instances.len()).unwrap_or(0)
   };
   ```

2. **If more than 1 instance**, do a rolling restart — pop and stop each instance individually:
   ```rust
   if instance_count > 1 {
       for _ in 0..instance_count {
           let removed = {
               let mut models = self.models.write().await;
               if let Some(managed) = models.get_mut(model_id) {
                   managed.instances.pop()
               } else { None }
           };
           if let Some(inst) = removed {
               if let Some(backend) = self.backends.read().await.get(model_id).cloned() {
                   let _ = backend.stop(&inst.handle).await;
               }
           }
       }
   } else {
       self.stop_model(model_id).await?;
   }
   ```

### 4.4 Lifecycle Hardening (`process/lifecycle.rs` + `registry/types.rs`)

**Problem**: No active health monitoring — crashed instances linger as "Ready" until a request fails.

**Solution**: The lifecycle loop pings `/health` on every active instance and marks failures.

#### `registry/types.rs`:

1. **Add field to `ManagedInstance`**:
   ```rust
   pub consecutive_health_failures: u8,
   ```
   Initialize to `0` wherever `ManagedInstance` is constructed.

2. **Add field to `ManagedModel`**:
   ```rust
   pub last_crash: Option<Instant>,
   ```
   Initialize to `None` wherever `ManagedModel` is constructed.

#### `process/lifecycle.rs`:

1. **Add imports**:
   ```rust
   use crate::registry::types::{ModelHandle, ModelStatus};
   ```

2. **Add constant**: `const HEALTH_FAILURE_THRESHOLD: u8 = 2;`

3. **Add `health_check_all()` function**:
   - Collects all model IDs (read lock, then release)
   - For each model, collects instance IDs + ports of Ready/Busy instances (read lock, then release)
   - For each instance with a port, sends `GET http://127.0.0.1:{port}/health`
   - On success: reset `consecutive_health_failures` to 0
   - On failure: increment `consecutive_health_failures`; if ≥ threshold, set `status = ModelStatus::Error("health check failed")` and `managed.last_crash = Some(Instant::now())`
   - Uses write lock only for the mutation step

4. **Call `health_check_all(&manager).await`** in the lifecycle loop, before `reap_idle()`.

5. **Update module doc comment** to describe health check behavior.

---

## 5. File-by-File Change Specification

### `genhat-desktop/src-tauri/src/registry/types.rs`

| Change | Description |
|--------|-------------|
| Add `http_client` field to `ProcessHandle` | `pub http_client: Option<reqwest::Client>` |
| Add `consecutive_health_failures` field to `ManagedInstance` | `pub consecutive_health_failures: u8` |
| Add `last_crash` field to `ManagedModel` | `pub last_crash: Option<Instant>` |

### `genhat-desktop/src-tauri/src/process/mod.rs`

| Change | Description |
|--------|-------------|
| Add imports | `AtomicBool`, `AtomicU64`, `Ordering`, `Duration`, `Instant` |
| Add `PipelineReservation` struct | Before `ProcessManager` struct |
| Add 3 new fields to `ProcessManager` | `reservations`, `recent_user_activity`, `last_user_activity_epoch` |
| Initialize new fields in `new()` | Default values |
| Initialize `last_crash: None` in all `ManagedModel` constructors | 2 locations: `new()` and `register_model()` |
| Initialize `consecutive_health_failures: 0` in `ManagedInstance` constructor | 1 location in `ensure_running()` |
| Replace budget hard-fail in `ensure_running()` | Call `evict_until_fits()` instead |
| Add `evict_until_fits()` method | Multi-pass eviction |
| Add `collect_eviction_candidates()` method | Per-pass candidate collection |
| Add `reserve_pipeline()` method | Pre-warm + pin models |
| Add `release_pipeline()` method | Remove reservation |
| Add `reserved_model_ids()` method | Collect pinned model IDs |
| Add `mark_user_activity()` method | Set flag + epoch |
| Add `is_user_active()` method | Check flag with 30s auto-clear |
| Add `has_free_instance()` method | Check for idle Ready instance |
| Add `instance_count()` method | Count instances for a model |
| Modify `ensure_ctx_size()` | Rolling restart for multi-instance |
| Update module doc comment | Describe new features |

### `genhat-desktop/src-tauri/src/process/pool.rs`

| Change | Description |
|--------|-------------|
| Add `is_high_priority()` function | Priority check helper |
| Add `LOW_PRIORITY_YIELD_RETRIES` constant | `3` |
| Add `LOW_PRIORITY_YIELD_MS` constant | `500` |
| Update module doc comment | Describe yield-on-demand |

### `genhat-desktop/src-tauri/src/process/lifecycle.rs`

| Change | Description |
|--------|-------------|
| Add imports | `ModelHandle`, `ModelStatus` |
| Add `HEALTH_FAILURE_THRESHOLD` constant | `2` |
| Add `health_check_all()` function | Ping /health on all active instances |
| Call health checks in lifecycle loop | Before `reap_idle()` |
| Update module doc comment | Describe health check behavior |

### `genhat-desktop/src-tauri/src/router/mod.rs`

| Change | Description |
|--------|-------------|
| Add user activity marking | `mark_user_activity()` for high-priority tasks |
| Add yield-on-demand loop | Low-priority tasks yield when user active + model at capacity |
| Add `any_candidate_has_free_slot()` helper | Check if any candidate model has a free instance |
| Update module doc comment | Describe yield-on-demand + activity marking |

### `genhat-desktop/src-tauri/src/podcast/engine.rs`

| Change | Description |
|--------|-------------|
| Add pipeline reservation in `generate_podcast()` | Determine models, reserve, pre-warm |
| Extract inner logic to `generate_podcast_inner()` | Existing generation code moved |
| Release reservation after inner completes | Cleanup regardless of result |

### `genhat-desktop/src-tauri/src/rag/pipeline.rs`

| Change | Description |
|--------|-------------|
| Add activity-aware throttle in `enrich_pending()` | Check `is_user_active()` before each chunk, sleep 2s if active |

### `genhat-desktop/src-tauri/src/backends/llama_server.rs`

| Change | Description |
|--------|-------------|
| Adaptive backoff in `wait_for_ready()` | 50→100→200→500ms based on elapsed time |
| Create persistent client in `start()` | `reqwest::Client` with connection pooling |
| Store client in `ProcessHandle` | `http_client: persistent_client` |
| Extract persistent client in `execute()` | From `ProcessHandle` alongside port |
| Use persistent client with per-request timeout | `.timeout()` on request builder |
| Pass client to `execute_classification()` | Add `persistent_client: Option<&reqwest::Client>` param |
| Pass client to `execute_embedding()` | Add `persistent_client: Option<&reqwest::Client>` param |
| Both helpers use persistent client | With fallback to `http_client_with_timeout()` |

### `genhat-desktop/src-tauri/src/backends/llama_cli.rs`

| Change | Description |
|--------|-------------|
| Add `http_client: None` to `ProcessHandle` | CLI doesn't use HTTP |

### `genhat-desktop/src-tauri/src/tts/inference.rs`

| Change | Description |
|--------|-------------|
| Change `session` field type | `Mutex<Session>` → `Session` |
| Remove `use std::sync::Mutex` | No longer needed |
| Remove `.lock().unwrap()` in `generate_single_chunk()` | Call `self.session.run()` directly |
| Remove `Mutex::new()` in `load()` | Store `Session` directly |
| Update doc comment | Explain ort 2.x `&self` safety |

---

## 6. Validation

After applying all changes, run:

```bash
cd genhat-desktop/src-tauri && cargo check
```

All code should compile cleanly. No new dependencies are required — this uses only existing crate dependencies (`reqwest`, `tokio`, `ort`, `futures_util`, `uuid`, `portpicker`).

### Key behavioral guarantees:

- **Podcast cold start**: Models pre-warm in parallel; typical wall time goes from `sum(cold_start_A, cold_start_B, cold_start_C)` to `max(cold_start_A, cold_start_B, cold_start_C)`.
- **Memory safety**: Budget exceeded → eviction → retry, never a hard error unless ALL models are actively serving.
- **User responsiveness**: Background enrichment yields to user chat within 500ms × 3 retries max.
- **TTS throughput**: Concurrent inference without Mutex serialization.
- **Connection reuse**: One TCP connection per llama-server instance, reused across all requests.
- **Crash detection**: Unhealthy instances marked Error within 2 lifecycle intervals.
- **ctx_size resize**: Other instances keep serving during rolling restart.
