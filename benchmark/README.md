# GenHat Benchmark Suite

This folder contains a complete benchmark pipeline for GenHat that captures:

- Cold start time
- Idle memory footprint
- Per-model memory deltas
- Peak memory (all models loaded during benchmark window)
- Total disk footprint
- App binary size
- Model load time (spawn → ready)
- Idle CPU usage
- Process count
- Graceful shutdown time
- Health check overhead (30s lifecycle loop estimate)

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

Launches GenHat and benchmarks from startup:

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode launch \
  --launch-cmd "cd genhat-desktop && npx tauri dev" \
  --interactive \
  --shutdown-after-benchmark
```

During the interactive model phase, load models from the UI, then press Enter.

---

## 3) Attach Mode (if app is already ON)

```bash
python3 benchmark/run_benchmark.py \
  --repo-root . \
  --mode attach \
  --attach-name genhat \
  --interactive
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
- `samples.csv` → time series samples (`rss_mb`, `cpu_percent`, `process_count`)
- `model_metrics.csv` → per-model load time + memory delta
- `tauri_runtime.log` → captured runtime logs
- `plots/`:
  - `rss_over_time.png`
  - `cpu_over_time.png`
  - `model_load_time.png`
  - `model_memory_delta.png`
  - `summary_metrics.png`

---

## 5) Metrics Mapping

| Metric | Implementation |
|---|---|
| Cold start time | launch timestamp → first readiness log regex match |
| Idle memory footprint | median RSS during idle window |
| Per-model memory | RSS at model `spawn` vs model `ready` from logs |
| Peak memory | max process-tree RSS during run |
| Total disk footprint | app binary size + models folder size (`du`) |
| App binary size | explicit path or auto-resolved binary (`ls -lh`) |
| Model load time | `ready_ts - spawn_ts` per model |
| Idle CPU usage | median process-tree CPU% during idle window |
| Process count | process-tree sample count + `pstree` snapshot |
| Graceful shutdown time | `SIGTERM` issued → process tree exits |
| Health check overhead | estimated CPU delta near lifecycle interval boundaries |

---

## 6) Notes

- For the most complete benchmark (all metrics), use `--mode launch` and `--shutdown-after-benchmark`.
- Per-model metrics depend on runtime logs that contain:
  - `Spawning new instance ... for model ...`
  - `Instance ... for model ... is ready`
- In-process models may show smaller or mixed memory deltas compared to child-process backends.
- If `smem/pidstat/pstree` are not installed, benchmark still runs using psutil fallback.
