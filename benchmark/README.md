# GenHat Benchmark Suite

This folder contains a benchmark-only (no app code changes required) pipeline that captures:

- Startup timing (cold start)
- Process-tree resource use over time (CPU, RSS, process count)
- Extended Linux `/proc` metrics (PSS/USS, I/O bytes + rates, page faults + rates, threads, open FDs, context switches)
- Per-model load time + memory deltas (parsed from existing runtime logs; best-effort)
- Disk footprint (models + app binary)
- Graceful shutdown time (launch mode)

It also generates visual graphs automatically.

---

## 1) Setup

From repository root:

```bash
python3 -m venv .venv-benchmark
source .venv-benchmark/bin/activate
pip install -r benchmark/requirements.txt
```

Optional Linux tools (recommended):

```bash
sudo apt-get install -y smem sysstat psmisc procps
```

- `smem` → extra memory validation
- `pidstat` (from `sysstat`) → CPU profiling
- `pstree` (from `psmisc`) → process tree snapshots

---

## 2) Quick Start (Launch Mode)

Launches GenHat and benchmarks from startup.

If your environment has Snap/loader issues (common on some Ubuntu setups), keep `--sanitize-launch-env` enabled (default).

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --interactive \
  --shutdown-after-benchmark \
  --sanitize-launch-env
```

### Launch mode, but run until you close the app

This keeps collecting metrics while you use the app normally, and stops when you close the app window (no forced shutdown step):

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --run-until-exit \
  --sanitize-launch-env
```

During the interactive model phase, load models from the UI, then press Enter.

---

## 3) Attach Mode (recommended if you start the app yourself)

Attach mode is the most robust if you prefer launching the app in a “known-good” terminal session (and then letting the benchmark only observe the process tree).

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-name app \
  --interactive
```

If your process name is different, try `--attach-name genhat` (or use `--attach-pid`).

If `--attach-name app` doesn’t match on your machine, find the PID and use `--attach-pid`:

```bash
pgrep -af "target/debug/app|genhat" \
  | head
```

For best model-load metrics in attach mode, provide a live tauri log file:

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-pid <PID> \
  --tauri-log-file /path/to/tauri.log \
  --interactive
```

---

## 4) Outputs

Each run creates a timestamped folder in `benchmark/results/<timestamp>/`:

- `metrics.json` → all core metrics
- `events.json` → structured event timeline parsed from logs (spawn/ready and optional `[BENCH]` markers)
- `samples.csv` → time series samples (`rss_mb`, `cpu_percent`, `process_count`)
- `extended_samples.csv` → extended samples (best-effort Linux `/proc` stats: PSS/USS, I/O, faults, threads, fds, ctx switches, llama-server count)
- `model_metrics.csv` → per-model load time + memory delta
- `tauri_runtime.log` → captured runtime logs
- `plots/`:
  - `rss_over_time.png`
  - `cpu_over_time.png`
  - `process_count_over_time.png`
  - `memory_breakdown_over_time.png`
  - `io_rates_over_time.png`
  - `fault_rates_over_time.png`
  - `threads_fds_over_time.png`
  - `llama_server_count_over_time.png`
  - `model_load_time.png`
  - `model_memory_delta.png`
  - `summary_metrics.png`

---

## 5) Exhaustive Metrics Checklist (benchmark-only)

This suite is designed to work without any app-side instrumentation.

### A) Timing

- Cold start time (best-effort): launch timestamp → “app ready” heuristic (or `[BENCH]` markers if present in logs)
- Shutdown time (launch mode): `SIGTERM` issued → process tree exits

### B) Background overhead

- Lifecycle / health-check overhead: best-effort CPU% overhead estimate from the periodic health-check loop

### C) Process tree (what gets sampled)

- The runner discovers the “real app PID” from the launch wrapper process tree (filters out `node/npx/npm/cargo/vite` wrappers) and samples the full descendant tree.

### D) Resource time series

- CPU% over time (process tree)
- RSS MB over time (process tree)
- Process count over time (process tree)

### E) Extended Linux `/proc` metrics (best-effort)

- PSS MB / USS MB over time (from `/proc/<pid>/smaps_rollup` when permitted)
- I/O bytes and read/write rates (from `/proc/<pid>/io`)
- Minor/major faults and derived fault rates (from `/proc/<pid>/stat`)
- Voluntary/involuntary context switches (from `/proc/<pid>/status`)
- Thread count, VMS, open file descriptors (via `psutil` where allowed)
- `llama-server` process count within the measured process tree

### F) Per-model metrics (log-derived)

- Model load time (spawn → ready) parsed from existing runtime logs
- Per-model RSS delta (RSS at ready − RSS at spawn)

### G) Disk footprint

- App binary size
- Models directory footprint

## 6) Useful Flags

- `--sample-interval-s` / `--extended-sample-interval-s`: tune sampling frequency
- `--no-smaps-rollup`, `--no-proc-io`, `--no-proc-faults`, `--no-proc-fds`, `--no-proc-ctx-switches`: disable expensive collectors
- `--sanitize-launch-env`: strips Snap-injected loader paths (helps avoid `libpthread` / `GLIBC_PRIVATE` symbol errors)

---

## 7) Notes

- For the most complete benchmark (shutdown timing + full capture), use `--mode launch` and `--shutdown-after-benchmark`.
- Per-model metrics depend on runtime logs that contain:
  - `Spawning new instance ... for model ...`
  - `Instance ... for model ... is ready`
- If `[BENCH]` markers exist in logs, the runner will use them, but they are not required.
- In-process models may show smaller or mixed memory deltas compared to child-process backends.
- If `smem/pidstat/pstree` are not installed, the benchmark still runs (they are optional).
