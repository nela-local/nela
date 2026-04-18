# GenHat Application Benchmark Suite

This folder contains an application-only benchmark pipeline (no app code changes required) that captures:

- Startup timing (cold start)
- Process-tree resource use over time (CPU, RSS, process count)
- Extended collector metrics (best-effort; full Linux `/proc` support, graceful fallback on Windows/macOS)
- Per-model load time + memory deltas (parsed from runtime logs)
- Disk footprint (models + app binary)
- Graceful shutdown timing (launch mode)
- Aggregated series statistics (min/max/mean/median/p95/p99/stddev)

The suite generates both PNG charts and interactive HTML charts.

---

## 1) Setup

From repository root:

```bash
python3 -m venv .venv-benchmark
source .venv-benchmark/bin/activate
pip install -r benchmark/requirements.txt
```

Optional Linux tools (recommended for deeper validation):

```bash
sudo apt-get install -y smem sysstat psmisc procps
```

- `smem` for extra memory validation
- `pidstat` (from `sysstat`) for CPU profiling
- `pstree` (from `psmisc`) for process tree snapshots

---

## 2) Quick Start (Launch Mode)

Launches GenHat and benchmarks from startup.

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --profile standard \
  --interactive \
  --shutdown-after-benchmark
```

### Run until the app exits

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --profile standard \
  --run-until-exit
```

### Fixed duration mode

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --profile quick \
  --duration-s 180
```

---

## 3) Attach Mode

Attach mode is useful when you launch the app in a separate known-good terminal and only want benchmark observation:

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-name app \
  --profile standard
```

If name matching fails, use a PID:

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-pid <PID>
```

For better model event parsing in attach mode, provide a live tauri log file:

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-pid <PID> \
  --tauri-log-file /path/to/tauri.log
```

---

## 4) Profiles

`--profile` sets default timing/sampling values unless explicitly overridden.

- `quick`: short check, faster turnaround
- `standard`: balanced default for day-to-day benchmarking
- `long`: denser sampling and longer steady-state windows

You can still override any profile default directly:

- `--sample-interval-s`
- `--extended-sample-interval-s`
- `--idle-window-s`
- `--model-load-window-s`

---

## 5) Outputs

Each run creates a timestamped folder in `benchmark/results/<timestamp>/`:

- `metrics.json`: core metrics, capabilities, and aggregate stats
- `events.json`: structured event timeline parsed from logs
- `samples.csv`: time-series core samples (`rss_mb`, `cpu_percent`, `cpu_user_percent`, `cpu_system_percent`, `cpu_percent_normalized`, `process_count`)
- `extended_samples.csv`: extended samples (collector-dependent)
- `model_metrics.csv`: per-model load time + memory delta
- `percentile_metrics.csv`: per-series window stats (`min/max/mean/median/p95/p99/stddev`)
- `tauri_runtime.log`: captured runtime logs
- `plots/` PNG outputs:
  - `rss_over_time.png`
  - `cpu_over_time.png`
  - `process_count_over_time.png`
  - `memory_breakdown_over_time.png`
  - `io_rates_over_time.png`
  - `fault_rates_over_time.png`
  - `threads_fds_over_time.png`
  - `llama_server_count_over_time.png` (backend process telemetry)
  - `model_load_time.png`
  - `model_memory_delta.png`
  - `summary_metrics.png`
- `plots/` HTML outputs (interactive):
  - `rss_over_time.html`
  - `cpu_over_time.html`
  - `process_count_over_time.html`
  - `memory_breakdown_over_time.html`
  - `io_rates_over_time.html`
  - `fault_rates_over_time.html`
  - `threads_fds_over_time.html`
  - `backend_process_count_over_time.html`
  - `model_load_time.html`
  - `model_memory_delta.html`
  - `summary_metrics.html`
  - `dashboard.html`

---

## 6) Metrics Coverage

### Timing

- Cold start time: launch timestamp to readiness marker/regex
- Shutdown time (launch mode): `SIGTERM` to process tree exit

### Process tree time series

- CPU% over time
- CPU user/system split over time
- CPU normalized by logical core count
- RSS MB over time
- Process count over time

### Extended collectors (best-effort)

Linux provides full `/proc` collection where available:

- PSS/USS/shared memory
- I/O bytes and rates
- Minor/major faults and rates
- Voluntary/involuntary context switches
- Open file descriptor counts

Windows and macOS runs still collect core process telemetry and write explicit capability flags so missing collectors do not fail the benchmark.

### Per-model metrics

- Model spawn to ready load time
- RSS delta (ready minus spawn)

### Aggregate statistics

For `rss_mb`, `cpu_percent`, and `process_count`, both full-run and idle-window stats include:

- `min`
- `max`
- `mean`
- `median`
- `p95`
- `p99`
- `stddev`

---

## 7) Useful Flags

- `--profile quick|standard|long`
- `--duration-s <seconds>`
- `--interactive`
- `--run-until-exit`
- `--sample-interval-s` / `--extended-sample-interval-s`
- `--no-smaps-rollup`, `--no-proc-io`, `--no-proc-faults`, `--no-proc-fds`, `--no-proc-ctx-switches`
- `--sanitize-launch-env`

---

## 8) Notes

- This folder now benchmarks application behavior only.
- If `[BENCH]` markers exist in logs, they are used when present, but they are not required.
- If optional Linux tools are not installed, the benchmark still runs.
