#!/usr/bin/env python3
import argparse
import csv
import json
import math
import os
import platform
import re
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Any

import psutil


SPAWN_RE = re.compile(r"Spawning new instance '([^']+)' for model '([^']+)'")
READY_RE = re.compile(r"Instance '([^']+)' for model '([^']+)' is ready")
LIFECYCLE_RE = re.compile(r"Lifecycle manager started \(interval=(\d+)s\)")

# Optional: explicit BENCH markers emitted by the app (Rust log output).
# This benchmark suite works without these markers.
BENCH_PID_RE = re.compile(r"\[BENCH\]\s+APP_PID\s+(\d+)")
BENCH_START_MS_RE = re.compile(r"\[BENCH\]\s+APP_START_MS\s+(\d+)")
BENCH_MARK_RE = re.compile(r"\[BENCH\]\s+MARK\s+([a-zA-Z0-9_\-\.]+)\s+(\d+)")
BENCH_SHUTDOWN_BEGIN_RE = re.compile(r"\[BENCH\]\s+SHUTDOWN_BEGIN\s+(\d+)")
BENCH_SHUTDOWN_END_RE = re.compile(r"\[BENCH\]\s+SHUTDOWN_END\s+(\d+)\s+elapsed_ms=(\d+)")
BENCH_MODEL_BEGIN_RE = re.compile(r"\[BENCH\]\s+MODEL_START_BEGIN\s+model_id=([^\s]+)\s+ts_ms=(\d+)")
BENCH_MODEL_READY_RE = re.compile(r"\[BENCH\]\s+MODEL_START_READY\s+model_id=([^\s]+)\s+ts_ms=(\d+)\s+elapsed_ms=(\d+)")


@dataclass
class Sample:
    ts: float
    elapsed_s: float
    rss_mb: float
    cpu_percent: float
    cpu_user_percent: float
    cpu_system_percent: float
    cpu_percent_normalized: float
    process_count: int


@dataclass
class ExtendedSample:
    ts: float
    elapsed_s: float
    rss_mb: float
    vms_mb: float
    pss_mb: Optional[float]
    uss_mb: Optional[float]
    shared_mb: Optional[float]
    cpu_percent: float
    cpu_user_percent: float
    cpu_system_percent: float
    cpu_percent_normalized: float
    process_count: int
    threads: int
    open_fds: Optional[int]
    read_bytes: int
    write_bytes: int
    read_rate_bps: Optional[float]
    write_rate_bps: Optional[float]
    minor_faults: int
    major_faults: int
    minor_faults_rate: Optional[float]
    major_faults_rate: Optional[float]
    ctx_switches_voluntary: Optional[int]
    ctx_switches_involuntary: Optional[int]
    llama_server_count: int


class BenchmarkRunner:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.repo_root = Path(args.repo_root).resolve()
        self.genhat_desktop = (self.repo_root / "genhat-desktop").resolve()
        self.models_dir = Path(args.models_dir).resolve() if args.models_dir else (self.repo_root / "models").resolve()

        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        self.results_dir = Path(args.output_dir).resolve() / ts
        self.results_dir.mkdir(parents=True, exist_ok=True)
        self.log_file = self.results_dir / "tauri_runtime.log"

        self.root_process: Optional[subprocess.Popen] = None
        self.root_pid: Optional[int] = None
        self.app_pid: Optional[int] = None
        self.measure_pid: Optional[int] = None
        self.monitoring_active = False
        self.start_time: Optional[float] = None
        self.ready_time: Optional[float] = None
        self.lifecycle_interval_s = 30

        self.app_start_ms: Optional[int] = None
        self.ui_ready_ms: Optional[int] = None
        self.shutdown_begin_ms: Optional[int] = None
        self.shutdown_end_ms: Optional[int] = None

        self.samples: List[Sample] = []
        self.extended_samples: List[ExtendedSample] = []
        self.spawn_events: Dict[str, List[Tuple[float, float]]] = defaultdict(list)
        self.ready_events: Dict[str, List[Tuple[float, float]]] = defaultdict(list)
        self.model_load_rows: List[dict] = []

        self.events: List[dict] = []

        self._pid_discovery_lock = threading.Lock()
        self._last_extended_ts: Optional[float] = None
        self._last_io: Optional[Tuple[float, int, int]] = None
        self._last_faults: Optional[Tuple[float, int, int]] = None

        self._log_lock = threading.Lock()
        self.capabilities = self._detect_capabilities()
        self._last_cpu_snapshot: Optional[Dict[str, float]] = None

    def run(self):
        metrics: Optional[dict] = None
        try:
            if self.args.mode == "launch":
                self._launch_app()
            else:
                self._attach_to_app()

            self._start_monitoring_thread()
            self._wait_for_ready_if_needed()
            if getattr(self.args, "run_until_exit", False):
                self._wait_until_exit()
            else:
                if float(getattr(self.args, "duration_s", 0.0)) > 0:
                    self._capture_fixed_duration_phase()
                else:
                    self._capture_idle_metrics_phase()
                    self._capture_model_loading_phase()
            self._capture_disk_and_binary_metrics()
            metrics = self._finalize_metrics()
            self._write_outputs(metrics)
            self._run_plotter()
            self._print_summary(metrics)
        finally:
            if (
                self.args.mode == "launch"
                and self.args.shutdown_after_benchmark
                and not getattr(self.args, "run_until_exit", False)
            ):
                shutdown_time = self._graceful_shutdown()
                if shutdown_time is not None:
                    metrics_path = self.results_dir / "metrics.json"
                    if metrics_path.exists():
                        metrics = json.loads(metrics_path.read_text(encoding="utf-8"))
                        metrics["graceful_shutdown_time_s"] = shutdown_time
                        metrics_path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")
                        self._run_plotter()
            self.monitoring_active = False

    def _wait_until_exit(self):
        """Keep sampling until the app exits (user closes window / process ends)."""
        print("\n=== Run-Until-Exit Mode ===")
        print("Use the app normally. Close it when finished.")
        print("Benchmark will stop automatically and write results/plots.\n")

        missing_target_count = 0
        # Prefer the discovered app PID; fall back to the current measure/root PID.
        while True:
            # If launch wrapper exits, we are done.
            if self.args.mode == "launch" and self.root_process and self.root_process.poll() is not None:
                break

            target_pid = self.app_pid or self.measure_pid or self.root_pid
            if target_pid and not psutil.pid_exists(int(target_pid)):
                missing_target_count += 1
            else:
                missing_target_count = 0

            # Require a few consecutive misses to avoid races during PID switching.
            if missing_target_count >= 6:
                break

            time.sleep(0.5)

        # In launch mode, ensure we do not leave the dev wrapper (npx tauri dev) running.
        if self.args.mode == "launch" and self.root_process and self.root_process.poll() is None:
            try:
                self.root_process.terminate()
                self.root_process.wait(timeout=8)
            except Exception:
                try:
                    self.root_process.kill()
                except Exception:
                    pass

    def _launch_app(self):
        launch_cmd = self.args.launch_cmd
        if not launch_cmd:
            launch_cmd = "cd genhat-desktop && npx tauri dev"

        self.start_time = time.time()
        env = os.environ.copy()
        env.setdefault("RUST_LOG", "info")
        if getattr(self.args, "sanitize_launch_env", False):
            env = self._sanitize_launch_env(env)

        popen_kwargs = {
            "cwd": str(self.repo_root),
            "stdout": subprocess.PIPE,
            "stderr": subprocess.STDOUT,
            "text": True,
            "bufsize": 1,
            "env": env,
        }
        if os.name == "nt":
            self.root_process = subprocess.Popen(launch_cmd, shell=True, **popen_kwargs)
        else:
            self.root_process = subprocess.Popen(["bash", "-lc", launch_cmd], **popen_kwargs)
        self.root_pid = self.root_process.pid
        self.measure_pid = self.root_pid

        # Start a background PID discovery loop to identify the actual Tauri app PID
        # (and not the wrapper bash/node/cargo process tree root).
        threading.Thread(target=self._pid_discovery_loop, daemon=True).start()

        threading.Thread(target=self._consume_stdout, daemon=True).start()

    def _sanitize_launch_env(self, env: dict) -> dict:
        """Best-effort env cleanup to avoid Snap runtime library injection.

        Some Linux setups (especially Snap) may add `/snap/...` libc paths that break
        locally built Rust binaries with symbol lookup errors.
        """
        cleaned = dict(env)

        # Remove SNAP-related variables that can influence runtime loader behavior.
        for key in list(cleaned.keys()):
            if key.startswith("SNAP"):
                cleaned.pop(key, None)

        ld = cleaned.get("LD_LIBRARY_PATH")
        if ld:
            parts = [p for p in ld.split(":") if "/snap/" not in p]
            if parts:
                cleaned["LD_LIBRARY_PATH"] = ":".join(parts)
            else:
                cleaned.pop("LD_LIBRARY_PATH", None)

        # If a preload is set (rare), drop it to reduce surprise.
        cleaned.pop("LD_PRELOAD", None)
        return cleaned

    def _attach_to_app(self):
        self.start_time = time.time()
        if self.args.attach_pid:
            self.root_pid = self.args.attach_pid
            self.measure_pid = self.root_pid
            return

        if not self.args.attach_name:
            raise ValueError("In attach mode, provide --attach-pid or --attach-name")

        target_name = self.args.attach_name.lower()
        for proc in psutil.process_iter(attrs=["pid", "name", "cmdline"]):
            name = (proc.info.get("name") or "").lower()
            cmdline = " ".join(proc.info.get("cmdline") or []).lower()
            if target_name in name or target_name in cmdline:
                self.root_pid = proc.info["pid"]
                self.measure_pid = self.root_pid
                break

        if not self.root_pid:
            raise RuntimeError(f"Could not find running process matching --attach-name={self.args.attach_name}")

        if self.args.tauri_log_file:
            threading.Thread(target=self._tail_log_file, daemon=True).start()

    def _consume_stdout(self):
        if not self.root_process or not self.root_process.stdout:
            return

        with self.log_file.open("w", encoding="utf-8") as log_f:
            for line in self.root_process.stdout:
                now = time.time()
                log_f.write(line)
                log_f.flush()
                self._parse_log_line(line, now)

    def _tail_log_file(self):
        log_path = Path(self.args.tauri_log_file).resolve()
        if not log_path.exists():
            return

        with self.log_file.open("w", encoding="utf-8") as merged_out:
            with log_path.open("r", encoding="utf-8", errors="replace") as file:
                file.seek(0, os.SEEK_END)
                while self.monitoring_active or not self.samples:
                    line = file.readline()
                    if not line:
                        time.sleep(0.2)
                        continue
                    now = time.time()
                    merged_out.write(line)
                    merged_out.flush()
                    self._parse_log_line(line, now)

    def _parse_log_line(self, line: str, ts: float):
        line = line.rstrip("\n")

        pid_match = BENCH_PID_RE.search(line)
        if pid_match:
            try:
                self.app_pid = int(pid_match.group(1))
                self.measure_pid = self.app_pid
                self._record_event("app_pid", ts, {"app_pid": self.app_pid})
            except ValueError:
                pass

        # Even without BENCH markers, try to discover the real app PID.
        self._maybe_discover_app_pid()

        start_ms_match = BENCH_START_MS_RE.search(line)
        if start_ms_match:
            try:
                self.app_start_ms = int(start_ms_match.group(1))
                self._record_event("app_start_ms", ts, {"app_start_ms": self.app_start_ms})
            except ValueError:
                pass

        mark_match = BENCH_MARK_RE.search(line)
        if mark_match:
            name, ts_ms = mark_match.groups()
            ts_ms_i: Optional[int]
            try:
                ts_ms_i = int(ts_ms)
            except ValueError:
                ts_ms_i = None
            self._record_event("mark", ts, {"name": name, "ts_ms": ts_ms_i})
            if name in {"ui_ready", "interactive_ready"} and ts_ms_i is not None:
                # Prefer explicit app-provided timestamp for cold-start timing.
                self.ui_ready_ms = ts_ms_i
            if name in {"ui_ready", "interactive_ready"} and self.start_time and self.ready_time is None:
                self.ready_time = ts

        shutdown_begin_match = BENCH_SHUTDOWN_BEGIN_RE.search(line)
        if shutdown_begin_match:
            try:
                self.shutdown_begin_ms = int(shutdown_begin_match.group(1))
                self._record_event("shutdown_begin", ts, {"ts_ms": self.shutdown_begin_ms})
            except ValueError:
                pass

        shutdown_end_match = BENCH_SHUTDOWN_END_RE.search(line)
        if shutdown_end_match:
            end_ms, elapsed_ms = shutdown_end_match.groups()
            try:
                self.shutdown_end_ms = int(end_ms)
                self._record_event(
                    "shutdown_end",
                    ts,
                    {"ts_ms": self.shutdown_end_ms, "elapsed_ms": int(elapsed_ms)},
                )
            except ValueError:
                pass

        model_begin_match = BENCH_MODEL_BEGIN_RE.search(line)
        if model_begin_match:
            model_id, ts_ms = model_begin_match.groups()
            try:
                self._record_event(
                    "model_start_begin",
                    ts,
                    {"model_id": model_id, "ts_ms": int(ts_ms)},
                )
            except ValueError:
                pass

        model_ready_match = BENCH_MODEL_READY_RE.search(line)
        if model_ready_match:
            model_id, ts_ms, elapsed_ms = model_ready_match.groups()
            try:
                self._record_event(
                    "model_start_ready",
                    ts,
                    {"model_id": model_id, "ts_ms": int(ts_ms), "elapsed_ms": int(elapsed_ms)},
                )
            except ValueError:
                pass

        lifecycle_match = LIFECYCLE_RE.search(line)
        if lifecycle_match:
            self.lifecycle_interval_s = int(lifecycle_match.group(1))
            if not self.ready_time and self.start_time:
                self.ready_time = ts

        if self.args.ready_regex and self.start_time and self.ready_time is None:
            if re.search(self.args.ready_regex, line):
                self.ready_time = ts

        spawn_match = SPAWN_RE.search(line)
        if spawn_match:
            _instance, model_id = spawn_match.groups()
            rss_now = self._rss_mb_tree()
            self.spawn_events[model_id].append((ts, rss_now))
            self._record_event("model_spawn", ts, {"model_id": model_id, "rss_mb": rss_now})

        ready_match = READY_RE.search(line)
        if ready_match:
            _instance, model_id = ready_match.groups()
            rss_now = self._rss_mb_tree()
            self.ready_events[model_id].append((ts, rss_now))
            self._resolve_model_row(model_id)
            self._record_event("model_ready", ts, {"model_id": model_id, "rss_mb": rss_now})

    def _record_event(self, event_type: str, ts: float, data: dict):
        elapsed = ts - (self.start_time or ts)
        row = {
            "ts": round(ts, 6),
            "elapsed_s": round(elapsed, 4),
            "type": event_type,
            "data": data,
        }
        with self._log_lock:
            self.events.append(row)

    def _resolve_model_row(self, model_id: str):
        if not self.spawn_events[model_id] or not self.ready_events[model_id]:
            return

        spawn_ts, spawn_rss = self.spawn_events[model_id].pop(0)
        ready_ts, ready_rss = self.ready_events[model_id].pop(0)
        if ready_ts < spawn_ts:
            return

        self.model_load_rows.append(
            {
                "model_id": model_id,
                "spawn_ts": spawn_ts,
                "ready_ts": ready_ts,
                "load_time_s": round(ready_ts - spawn_ts, 4),
                "rss_at_spawn_mb": round(spawn_rss, 2),
                "rss_at_ready_mb": round(ready_rss, 2),
                "rss_delta_mb": round(ready_rss - spawn_rss, 2),
            }
        )

    def _wait_for_ready_if_needed(self):
        if self.args.mode != "launch":
            return

        timeout_s = self.args.ready_timeout_s
        start = time.time()

        while time.time() - start < timeout_s:
            if self.ready_time is not None:
                return
            if self.root_process and self.root_process.poll() is not None:
                raise RuntimeError("Launch process exited before app became ready")
            self._maybe_discover_app_pid()
            time.sleep(0.25)

        self.ready_time = time.time()

    def _start_monitoring_thread(self):
        self.monitoring_active = True

        def monitor_loop():
            while self.monitoring_active:
                ts = time.time()
                elapsed = ts - (self.start_time or ts)
                self._maybe_discover_app_pid()

                rss_mb = self._rss_mb_tree()
                cpu_stats = self._cpu_percent_tree()
                cpu_pct = cpu_stats["total"]
                process_count = self._process_count_tree()
                self.samples.append(
                    Sample(
                        ts=ts,
                        elapsed_s=elapsed,
                        rss_mb=rss_mb,
                        cpu_percent=cpu_pct,
                        cpu_user_percent=cpu_stats["user"],
                        cpu_system_percent=cpu_stats["system"],
                        cpu_percent_normalized=cpu_stats["normalized"],
                        process_count=process_count,
                    )
                )

                self._maybe_collect_extended_sample(ts, elapsed, rss_mb, cpu_pct, cpu_stats, process_count)
                time.sleep(self.args.sample_interval_s)

        threading.Thread(target=monitor_loop, daemon=True).start()

    def _capture_idle_metrics_phase(self):
        idle_duration = self.args.idle_window_s
        if idle_duration <= 0:
            return
        time.sleep(idle_duration)

    def _capture_model_loading_phase(self):
        if self.args.model_load_window_s <= 0:
            return

        if self.args.interactive:
            print("\n=== Model Load Phase ===")
            print("Open GenHat UI now and load models (start each model once).")
            print("Press ENTER when done, or wait for timeout.")

            event = threading.Event()

            def wait_enter():
                try:
                    input()
                except EOFError:
                    pass
                event.set()

            threading.Thread(target=wait_enter, daemon=True).start()
            event.wait(timeout=self.args.model_load_window_s)
        else:
            time.sleep(self.args.model_load_window_s)

    def _capture_fixed_duration_phase(self):
        duration_s = float(getattr(self.args, "duration_s", 0.0))
        if duration_s <= 0:
            return
        print(f"\n=== Fixed-Duration Mode ({duration_s:.1f}s) ===")
        end_at = time.time() + duration_s
        while True:
            remaining = end_at - time.time()
            if remaining <= 0:
                break
            time.sleep(min(0.5, remaining))

    def _capture_disk_and_binary_metrics(self):
        return

    def _pid_discovery_loop(self):
        # Run for a bounded time; if attach mode is used, this is not called.
        start = time.time()
        timeout_s = max(10.0, float(getattr(self.args, "ready_timeout_s", 180)))
        while self.monitoring_active and time.time() - start < timeout_s:
            if self.app_pid and self.measure_pid == self.app_pid:
                return
            self._maybe_discover_app_pid()
            time.sleep(0.5)

    def _maybe_discover_app_pid(self):
        if self.args.mode != "launch":
            return
        if not self.root_pid or not psutil.pid_exists(self.root_pid):
            return
        # If BENCH already set a better PID, keep it.
        if self.app_pid and self.measure_pid == self.app_pid:
            return

        with self._pid_discovery_lock:
            candidate = self._discover_app_pid_from_tree(self.root_pid)
            if candidate and candidate != self.measure_pid:
                self.app_pid = candidate
                self.measure_pid = candidate
                self._record_event("app_pid_heuristic", time.time(), {"app_pid": candidate})

    def _discover_app_pid_from_tree(self, root_pid: int) -> Optional[int]:
        try:
            root = psutil.Process(root_pid)
            procs = [root] + root.children(recursive=True)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            return None

        best_score = -10_000
        best_pid: Optional[int] = None

        for proc in procs:
            try:
                pid = proc.pid
                name = (proc.name() or "").lower()
                cmdline = " ".join(proc.cmdline() or []).lower()
                exe = (proc.exe() or "").lower()
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue

            score = 0
            # Strong positive signals.
            if "/target/debug/app" in exe or cmdline.endswith("target/debug/app") or "target/debug/app" in cmdline:
                score += 200
            if "genhat" in exe or "genhat" in cmdline:
                score += 120
            if name in {"app", "genhat", "genhat-desktop"}:
                score += 80

            # Penalize build/dev tooling wrappers.
            if name in {"bash", "node", "npm", "npx", "cargo", "rustc", "vite", "esbuild"}:
                score -= 200
            if "tauri" in cmdline and name in {"node", "bash"}:
                score -= 50

            # Prefer non-root descendants (actual app is usually not the root wrapper).
            if pid != root_pid:
                score += 10

            if score > best_score:
                best_score = score
                best_pid = pid

        # Require a minimum confidence.
        if best_pid is None or best_score < 80:
            return None
        return best_pid

    def _maybe_collect_extended_sample(
        self,
        ts: float,
        elapsed: float,
        rss_mb: float,
        cpu_pct: float,
        cpu_stats: Dict[str, float],
        process_count: int,
    ):
        interval = float(getattr(self.args, "extended_sample_interval_s", 5.0))
        if interval <= 0:
            return
        if self._last_extended_ts is not None and ts - self._last_extended_ts < interval:
            return
        self._last_extended_ts = ts

        pid = self.measure_pid or self.root_pid
        if not pid or not psutil.pid_exists(pid):
            return

        pids = [pid] + self._descendant_pids(pid)
        stats = self._collect_proc_tree_stats(pids)

        read_rate = None
        write_rate = None
        if self._last_io is not None:
            last_ts, last_r, last_w = self._last_io
            dt = ts - last_ts
            if dt > 0:
                read_rate = max(0.0, (stats["read_bytes"] - last_r) / dt)
                write_rate = max(0.0, (stats["write_bytes"] - last_w) / dt)
        self._last_io = (ts, stats["read_bytes"], stats["write_bytes"])

        minflt_rate = None
        majflt_rate = None
        if self._last_faults is not None:
            last_ts, last_minflt, last_majflt = self._last_faults
            dt = ts - last_ts
            if dt > 0:
                minflt_rate = (stats["minor_faults"] - last_minflt) / dt
                majflt_rate = (stats["major_faults"] - last_majflt) / dt
        self._last_faults = (ts, stats["minor_faults"], stats["major_faults"])

        llama_count = self._count_processes_matching(pids, ["llama-server", "llama_server", "llama"])

        self.extended_samples.append(
            ExtendedSample(
                ts=ts,
                elapsed_s=elapsed,
                rss_mb=rss_mb,
                vms_mb=stats.get("vms_mb", 0.0),
                pss_mb=stats.get("pss_mb"),
                uss_mb=stats.get("uss_mb"),
                shared_mb=stats.get("shared_mb"),
                cpu_percent=cpu_pct,
                cpu_user_percent=cpu_stats.get("user", 0.0),
                cpu_system_percent=cpu_stats.get("system", 0.0),
                cpu_percent_normalized=cpu_stats.get("normalized", 0.0),
                process_count=process_count,
                threads=int(stats.get("threads", 0)),
                open_fds=stats.get("open_fds"),
                read_bytes=int(stats.get("read_bytes", 0)),
                write_bytes=int(stats.get("write_bytes", 0)),
                read_rate_bps=read_rate,
                write_rate_bps=write_rate,
                minor_faults=int(stats.get("minor_faults", 0)),
                major_faults=int(stats.get("major_faults", 0)),
                minor_faults_rate=minflt_rate,
                major_faults_rate=majflt_rate,
                ctx_switches_voluntary=stats.get("ctx_vol"),
                ctx_switches_involuntary=stats.get("ctx_invol"),
                llama_server_count=llama_count,
            )
        )

    def _count_processes_matching(self, pids: List[int], needles: List[str]) -> int:
        needles_l = [n.lower() for n in needles]
        count = 0
        for pid in pids:
            try:
                proc = psutil.Process(pid)
                name = (proc.name() or "").lower()
                cmdline = " ".join(proc.cmdline() or []).lower()
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
            hay = f"{name} {cmdline}"
            if any(n in hay for n in needles_l):
                count += 1
        return count

    def _collect_proc_tree_stats(self, pids: List[int]) -> Dict[str, Any]:
        # Aggregates multiple /proc-derived metrics across a process tree.
        total_vms = 0
        total_threads = 0
        total_fds: Optional[int] = 0
        total_read = 0
        total_write = 0
        total_minflt = 0
        total_majflt = 0
        total_ctx_vol: Optional[int] = 0
        total_ctx_invol: Optional[int] = 0

        pss_kb: Optional[int] = 0
        uss_kb: Optional[int] = 0
        shared_kb: Optional[int] = 0

        enable_smaps = bool(self.capabilities.get("smaps_rollup", False))
        enable_io = bool(self.capabilities.get("proc_io", False))
        enable_faults = bool(self.capabilities.get("proc_faults", False))
        enable_fds = bool(self.capabilities.get("open_fds", False))
        enable_ctx = bool(self.capabilities.get("proc_ctx_switches", False))

        for pid in pids:
            # psutil vms + threads are cheap.
            try:
                proc = psutil.Process(pid)
                mi = proc.memory_info()
                total_vms += getattr(mi, "vms", 0)
                total_threads += int(proc.num_threads())
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue

            if enable_fds:
                try:
                    # Linux-only.
                    fds = proc.num_fds()
                    total_fds = (total_fds or 0) + int(fds)
                except (AttributeError, psutil.Error):
                    total_fds = None

            if enable_io:
                io = self._read_proc_io(pid)
                if io:
                    total_read += io.get("read_bytes", 0)
                    total_write += io.get("write_bytes", 0)

            if enable_faults:
                faults = self._read_proc_faults(pid)
                if faults:
                    total_minflt += faults.get("minor_faults", 0)
                    total_majflt += faults.get("major_faults", 0)

            if enable_ctx:
                ctx = self._read_proc_ctx_switches(pid)
                if ctx:
                    if total_ctx_vol is not None:
                        vol = ctx.get("voluntary")
                        if vol is None:
                            total_ctx_vol = None
                        else:
                            total_ctx_vol += int(vol)
                    if total_ctx_invol is not None:
                        invol = ctx.get("involuntary")
                        if invol is None:
                            total_ctx_invol = None
                        else:
                            total_ctx_invol += int(invol)

            if enable_smaps:
                smaps = self._read_proc_smaps_rollup(pid)
                if smaps:
                    if pss_kb is not None:
                        pss_val = smaps.get("pss_kb")
                        if pss_val is None:
                            pss_kb = None
                        else:
                            pss_kb += int(pss_val)
                    if uss_kb is not None:
                        uss_val = smaps.get("uss_kb")
                        if uss_val is None:
                            uss_kb = None
                        else:
                            uss_kb += int(uss_val)
                    if shared_kb is not None:
                        sh_val = smaps.get("shared_kb")
                        if sh_val is None:
                            shared_kb = None
                        else:
                            shared_kb += int(sh_val)

        out: Dict[str, Any] = {
            "vms_mb": total_vms / (1024 * 1024),
            "threads": total_threads,
            "open_fds": total_fds,
            "read_bytes": total_read,
            "write_bytes": total_write,
            "minor_faults": total_minflt,
            "major_faults": total_majflt,
            "ctx_vol": total_ctx_vol,
            "ctx_invol": total_ctx_invol,
        }

        if enable_smaps:
            out["pss_mb"] = (pss_kb / 1024.0) if isinstance(pss_kb, int) else None
            out["uss_mb"] = (uss_kb / 1024.0) if isinstance(uss_kb, int) else None
            out["shared_mb"] = (shared_kb / 1024.0) if isinstance(shared_kb, int) else None
        else:
            out["pss_mb"] = None
            out["uss_mb"] = None
            out["shared_mb"] = None

        return out

    def _read_proc_io(self, pid: int) -> Optional[Dict[str, int]]:
        path = Path(f"/proc/{pid}/io")
        if not path.exists():
            return None
        out: Dict[str, int] = {}
        try:
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                if ":" not in line:
                    continue
                k, v = line.split(":", 1)
                k = k.strip()
                v = v.strip()
                if k in {"read_bytes", "write_bytes"}:
                    try:
                        out[k] = int(v)
                    except ValueError:
                        continue
        except OSError:
            return None
        return out

    def _read_proc_faults(self, pid: int) -> Optional[Dict[str, int]]:
        # /proc/<pid>/stat: fields 10 (minflt) and 12 (majflt)
        path = Path(f"/proc/{pid}/stat")
        if not path.exists():
            return None
        try:
            raw = path.read_text(encoding="utf-8", errors="replace")
            # process name may contain spaces inside parentheses; split accordingly.
            after = raw.rsplit(")", 1)
            if len(after) != 2:
                return None
            parts = after[1].strip().split()
            # After the ')' split, parts[0] is state (field 3).
            # So minflt (field 10) becomes index 7, majflt (field 12) becomes index 9.
            minflt = int(parts[7])
            majflt = int(parts[9])
            return {"minor_faults": minflt, "major_faults": majflt}
        except (OSError, ValueError, IndexError):
            return None

    def _read_proc_ctx_switches(self, pid: int) -> Optional[Dict[str, int]]:
        path = Path(f"/proc/{pid}/status")
        if not path.exists():
            return None
        vol = None
        invol = None
        try:
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("voluntary_ctxt_switches"):
                    try:
                        vol = int(line.split(":", 1)[1].strip())
                    except ValueError:
                        vol = None
                elif line.startswith("nonvoluntary_ctxt_switches"):
                    try:
                        invol = int(line.split(":", 1)[1].strip())
                    except ValueError:
                        invol = None
        except OSError:
            return None
        if vol is None and invol is None:
            return None
        out: Dict[str, int] = {}
        if vol is not None:
            out["voluntary"] = vol
        if invol is not None:
            out["involuntary"] = invol
        return out

    def _read_proc_smaps_rollup(self, pid: int) -> Optional[Dict[str, int]]:
        path = Path(f"/proc/{pid}/smaps_rollup")
        if not path.exists():
            return None
        pss = None
        priv_clean = 0
        priv_dirty = 0
        shared_clean = 0
        shared_dirty = 0
        try:
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("Pss:"):
                    pss = int(line.split()[1])
                elif line.startswith("Private_Clean:"):
                    priv_clean += int(line.split()[1])
                elif line.startswith("Private_Dirty:"):
                    priv_dirty += int(line.split()[1])
                elif line.startswith("Shared_Clean:"):
                    shared_clean += int(line.split()[1])
                elif line.startswith("Shared_Dirty:"):
                    shared_dirty += int(line.split()[1])
        except (OSError, ValueError):
            return None

        uss = priv_clean + priv_dirty
        shared = shared_clean + shared_dirty
        out = {
            "pss_kb": pss,
            "uss_kb": uss,
            "shared_kb": shared,
        }
        return out

    def _finalize_metrics(self) -> dict:
        self.monitoring_active = False
        time.sleep(self.args.sample_interval_s * 1.2)

        all_samples = self.samples or []
        all_rss = [s.rss_mb for s in all_samples]
        all_cpu = [s.cpu_percent for s in all_samples]
        all_cpu_user = [s.cpu_user_percent for s in all_samples]
        all_cpu_system = [s.cpu_system_percent for s in all_samples]
        all_cpu_norm = [s.cpu_percent_normalized for s in all_samples]
        all_proc = [float(s.process_count) for s in all_samples]

        # "Idle" should represent steady-state after the app becomes interactive.
        idle_samples: List[Sample] = []
        if all_samples and self.start_time:
            ready_elapsed = 0.0
            if self.ready_time is not None:
                ready_elapsed = max(0.0, self.ready_time - self.start_time)
            idle_start = ready_elapsed
            idle_end = ready_elapsed + max(0.0, self.args.idle_window_s)
            idle_samples = [s for s in all_samples if idle_start <= s.elapsed_s <= idle_end]

        idle_rss = [s.rss_mb for s in idle_samples]
        idle_cpu = [s.cpu_percent for s in idle_samples]
        idle_cpu_user_vals = [s.cpu_user_percent for s in idle_samples]
        idle_cpu_system_vals = [s.cpu_system_percent for s in idle_samples]
        idle_cpu_norm_vals = [s.cpu_percent_normalized for s in idle_samples]
        idle_proc = [float(s.process_count) for s in idle_samples]

        rss_stats_all = self._series_stats(all_rss)
        cpu_stats_all = self._series_stats(all_cpu)
        cpu_user_stats_all = self._series_stats(all_cpu_user)
        cpu_system_stats_all = self._series_stats(all_cpu_system)
        cpu_norm_stats_all = self._series_stats(all_cpu_norm)
        proc_stats_all = self._series_stats(all_proc)
        rss_stats_idle = self._series_stats(idle_rss)
        cpu_stats_idle = self._series_stats(idle_cpu)
        cpu_user_stats_idle = self._series_stats(idle_cpu_user_vals)
        cpu_system_stats_idle = self._series_stats(idle_cpu_system_vals)
        cpu_norm_stats_idle = self._series_stats(idle_cpu_norm_vals)
        proc_stats_idle = self._series_stats(idle_proc)

        idle_memory_mb = rss_stats_idle["median"]
        idle_cpu_percent = cpu_stats_idle["median"]
        idle_cpu_user_percent = cpu_user_stats_idle["median"]
        idle_cpu_system_percent = cpu_system_stats_idle["median"]
        idle_cpu_percent_normalized = cpu_norm_stats_idle["median"]
        peak_memory_mb = rss_stats_all["max"]
        peak_cpu_percent = cpu_stats_all["max"]
        peak_cpu_percent_normalized = cpu_norm_stats_all["max"]
        peak_process_count = int(round(proc_stats_all["max"]))

        idle_pss_mb = None
        idle_uss_mb = None
        peak_pss_mb = None
        peak_uss_mb = None
        peak_llama_server_count = None
        if self.extended_samples:
            # Align idle window for extended samples too.
            ex_idle = []
            if self.start_time:
                ready_elapsed = 0.0
                if self.ready_time is not None:
                    ready_elapsed = max(0.0, self.ready_time - self.start_time)
                idle_start = ready_elapsed
                idle_end = ready_elapsed + max(0.0, self.args.idle_window_s)
                ex_idle = [s for s in self.extended_samples if idle_start <= s.elapsed_s <= idle_end]

            pss_vals = [s.pss_mb for s in ex_idle if s.pss_mb is not None]
            uss_vals = [s.uss_mb for s in ex_idle if s.uss_mb is not None]
            if pss_vals:
                idle_pss_mb = self._median([float(v) for v in pss_vals])
            if uss_vals:
                idle_uss_mb = self._median([float(v) for v in uss_vals])

            all_pss = [s.pss_mb for s in self.extended_samples if s.pss_mb is not None]
            all_uss = [s.uss_mb for s in self.extended_samples if s.uss_mb is not None]
            if all_pss:
                peak_pss_mb = max(float(v) for v in all_pss)
            if all_uss:
                peak_uss_mb = max(float(v) for v in all_uss)
            peak_llama_server_count = max([s.llama_server_count for s in self.extended_samples], default=0)

        idle_memory_ps_mb = self._ps_rss_mb_tree()
        idle_memory_smem_mb = self._smem_rss_mb_tree()
        idle_cpu_pidstat = self._pidstat_cpu_percent()
        if idle_cpu_pidstat is not None:
            idle_cpu_percent = idle_cpu_pidstat

        cold_start_time_s = None
        cold_start_method = None
        if self.app_start_ms is not None and self.ui_ready_ms is not None:
            cold_start_time_s = max(0.0, (self.ui_ready_ms - self.app_start_ms) / 1000.0)
            cold_start_method = "[BENCH] APP_START_MS -> [BENCH] MARK ui_ready"
        elif self.start_time and self.ready_time:
            cold_start_time_s = max(0.0, self.ready_time - self.start_time)
            cold_start_method = "wall clock (launch -> ready regex)"

        app_binary_path = self._resolve_binary_path()
        app_binary_size_bytes = self._file_size_bytes(app_binary_path) if app_binary_path else 0
        app_binary_size_human = self._ls_size_human(app_binary_path) if app_binary_path else "N/A"

        models_disk_bytes, models_disk_human = self._du_size(self.models_dir)
        total_disk_bytes = models_disk_bytes + app_binary_size_bytes
        total_disk_human = self._human_size(total_disk_bytes)

        health_overhead = self._estimate_health_check_overhead()
        root_status = self._proc_status_snapshot()
        root_smaps_rollup_kb = self._proc_smaps_rollup_kb()

        app_reported_shutdown_time_s = None
        if self.shutdown_begin_ms is not None and self.shutdown_end_ms is not None:
            app_reported_shutdown_time_s = max(0.0, (self.shutdown_end_ms - self.shutdown_begin_ms) / 1000.0)

        metrics = {
            "profile": self.args.profile,
            "mode": self.args.mode,
            "timestamp": datetime.now().isoformat(),
            "duration_s": round(float(getattr(self.args, "duration_s", 0.0)), 2),
            "root_pid": self.root_pid,
            "app_pid": self.app_pid,
            "measure_pid": self.measure_pid,
            "app_start_ms": self.app_start_ms,
            "ui_ready_ms": self.ui_ready_ms,
            "cold_start_method": cold_start_method,
            "cold_start_time_s": round(cold_start_time_s or 0.0, 3),
            "idle_memory_mb": round(idle_memory_mb, 2),
            "idle_pss_mb": round(idle_pss_mb, 2) if idle_pss_mb is not None else None,
            "idle_uss_mb": round(idle_uss_mb, 2) if idle_uss_mb is not None else None,
            "idle_memory_ps_mb": round(idle_memory_ps_mb, 2) if idle_memory_ps_mb is not None else None,
            "idle_memory_smem_mb": round(idle_memory_smem_mb, 2) if idle_memory_smem_mb is not None else None,
            "per_model_memory_method": "RSS delta between spawn and ready events",
            "peak_memory_mb": round(peak_memory_mb, 2),
            "peak_pss_mb": round(peak_pss_mb, 2) if peak_pss_mb is not None else None,
            "peak_uss_mb": round(peak_uss_mb, 2) if peak_uss_mb is not None else None,
            "total_disk_footprint_bytes": int(total_disk_bytes),
            "total_disk_footprint_human": total_disk_human,
            "app_binary_size_bytes": int(app_binary_size_bytes),
            "app_binary_size_human": app_binary_size_human,
            "models_disk_bytes": int(models_disk_bytes),
            "models_disk_human": models_disk_human,
            "idle_cpu_percent": round(idle_cpu_percent, 2),
            "idle_cpu_user_percent": round(idle_cpu_user_percent, 2),
            "idle_cpu_system_percent": round(idle_cpu_system_percent, 2),
            "idle_cpu_percent_normalized": round(idle_cpu_percent_normalized, 2),
            "peak_cpu_percent": round(peak_cpu_percent, 2),
            "peak_cpu_percent_normalized": round(peak_cpu_percent_normalized, 2),
            "idle_cpu_pidstat_percent": round(idle_cpu_pidstat, 2) if idle_cpu_pidstat is not None else None,
            "peak_process_count": int(peak_process_count),
            "peak_llama_server_count": int(peak_llama_server_count)
            if peak_llama_server_count is not None
            else None,
            "graceful_shutdown_time_s": None,
            "app_reported_shutdown_time_s": round(app_reported_shutdown_time_s, 4)
            if app_reported_shutdown_time_s is not None
            else None,
            "health_check_interval_s": self.lifecycle_interval_s,
            "health_check_overhead_cpu_percent": round(health_overhead, 4),
            "process_count_method": "psutil descendants + pstree snapshots",
            "event_count": int(len(self.events)),
            "sample_count": int(len(self.samples)),
            "extended_sample_count": int(len(self.extended_samples)),
            "collector_capabilities": self.capabilities,
            "series_stats": {
                "rss_mb": {
                    "all": self._round_stats(rss_stats_all),
                    "idle": self._round_stats(rss_stats_idle),
                },
                "cpu_percent": {
                    "all": self._round_stats(cpu_stats_all),
                    "idle": self._round_stats(cpu_stats_idle),
                },
                "cpu_user_percent": {
                    "all": self._round_stats(cpu_user_stats_all),
                    "idle": self._round_stats(cpu_user_stats_idle),
                },
                "cpu_system_percent": {
                    "all": self._round_stats(cpu_system_stats_all),
                    "idle": self._round_stats(cpu_system_stats_idle),
                },
                "cpu_percent_normalized": {
                    "all": self._round_stats(cpu_norm_stats_all),
                    "idle": self._round_stats(cpu_norm_stats_idle),
                },
                "process_count": {
                    "all": self._round_stats(proc_stats_all),
                    "idle": self._round_stats(proc_stats_idle),
                },
            },
            "tooling": {
                "time": "internal wall clock + launch timestamps",
                "ps": self._which("ps"),
                "smem": self._which("smem"),
                "pidstat": self._which("pidstat"),
                "pstree": self._which("pstree"),
                "du": self._which("du"),
                "ls": self._which("ls"),
            },
            "app_binary_path": str(app_binary_path) if app_binary_path else None,
            "models_dir": str(self.models_dir),
            "proc_status_snapshot": root_status,
            "proc_smaps_rollup_rss_kb": root_smaps_rollup_kb,
        }

        if self.measure_pid:
            metrics["pstree_snapshot"] = self._pstree_snapshot(self.measure_pid)
        elif self.root_pid:
            metrics["pstree_snapshot"] = self._pstree_snapshot(self.root_pid)

        return metrics

    def _write_outputs(self, metrics: dict):
        metrics_path = self.results_dir / "metrics.json"
        metrics_path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")

        with self._log_lock:
            events_snapshot = list(self.events)
        (self.results_dir / "events.json").write_text(
            json.dumps(events_snapshot, indent=2),
            encoding="utf-8",
        )

        with (self.results_dir / "samples.csv").open("w", newline="", encoding="utf-8") as file:
            writer = csv.DictWriter(
                file,
                fieldnames=[
                    "ts",
                    "elapsed_s",
                    "rss_mb",
                    "cpu_percent",
                    "cpu_user_percent",
                    "cpu_system_percent",
                    "cpu_percent_normalized",
                    "process_count",
                ],
            )
            writer.writeheader()
            for sample in self.samples:
                writer.writerow(
                    {
                        "ts": round(sample.ts, 6),
                        "elapsed_s": round(sample.elapsed_s, 4),
                        "rss_mb": round(sample.rss_mb, 4),
                        "cpu_percent": round(sample.cpu_percent, 4),
                        "cpu_user_percent": round(sample.cpu_user_percent, 4),
                        "cpu_system_percent": round(sample.cpu_system_percent, 4),
                        "cpu_percent_normalized": round(sample.cpu_percent_normalized, 4),
                        "process_count": sample.process_count,
                    }
                )

        if self.extended_samples:
            with (self.results_dir / "extended_samples.csv").open("w", newline="", encoding="utf-8") as file:
                fieldnames = [
                    "ts",
                    "elapsed_s",
                    "rss_mb",
                    "vms_mb",
                    "pss_mb",
                    "uss_mb",
                    "shared_mb",
                    "cpu_percent",
                    "cpu_user_percent",
                    "cpu_system_percent",
                    "cpu_percent_normalized",
                    "process_count",
                    "threads",
                    "open_fds",
                    "read_bytes",
                    "write_bytes",
                    "read_rate_bps",
                    "write_rate_bps",
                    "minor_faults",
                    "major_faults",
                    "minor_faults_rate",
                    "major_faults_rate",
                    "ctx_switches_voluntary",
                    "ctx_switches_involuntary",
                    "llama_server_count",
                ]
                writer = csv.DictWriter(file, fieldnames=fieldnames)
                writer.writeheader()
                for s in self.extended_samples:
                    writer.writerow(
                        {
                            "ts": round(s.ts, 6),
                            "elapsed_s": round(s.elapsed_s, 4),
                            "rss_mb": round(s.rss_mb, 4),
                            "vms_mb": round(s.vms_mb, 4),
                            "pss_mb": round(s.pss_mb, 4) if s.pss_mb is not None else None,
                            "uss_mb": round(s.uss_mb, 4) if s.uss_mb is not None else None,
                            "shared_mb": round(s.shared_mb, 4) if s.shared_mb is not None else None,
                            "cpu_percent": round(s.cpu_percent, 4),
                            "cpu_user_percent": round(s.cpu_user_percent, 4),
                            "cpu_system_percent": round(s.cpu_system_percent, 4),
                            "cpu_percent_normalized": round(s.cpu_percent_normalized, 4),
                            "process_count": s.process_count,
                            "threads": s.threads,
                            "open_fds": s.open_fds,
                            "read_bytes": s.read_bytes,
                            "write_bytes": s.write_bytes,
                            "read_rate_bps": round(s.read_rate_bps, 4) if s.read_rate_bps is not None else None,
                            "write_rate_bps": round(s.write_rate_bps, 4) if s.write_rate_bps is not None else None,
                            "minor_faults": s.minor_faults,
                            "major_faults": s.major_faults,
                            "minor_faults_rate": round(s.minor_faults_rate, 6) if s.minor_faults_rate is not None else None,
                            "major_faults_rate": round(s.major_faults_rate, 6) if s.major_faults_rate is not None else None,
                            "ctx_switches_voluntary": s.ctx_switches_voluntary,
                            "ctx_switches_involuntary": s.ctx_switches_involuntary,
                            "llama_server_count": s.llama_server_count,
                        }
                    )

        with (self.results_dir / "model_metrics.csv").open("w", newline="", encoding="utf-8") as file:
            fieldnames = [
                "model_id",
                "spawn_ts",
                "ready_ts",
                "load_time_s",
                "rss_at_spawn_mb",
                "rss_at_ready_mb",
                "rss_delta_mb",
            ]
            writer = csv.DictWriter(file, fieldnames=fieldnames)
            writer.writeheader()
            for row in self.model_load_rows:
                writer.writerow(row)

        stats = metrics.get("series_stats", {})
        if isinstance(stats, dict) and stats:
            with (self.results_dir / "percentile_metrics.csv").open("w", newline="", encoding="utf-8") as file:
                fieldnames = ["metric", "window", "min", "max", "mean", "median", "p95", "p99", "stddev"]
                writer = csv.DictWriter(file, fieldnames=fieldnames)
                writer.writeheader()
                for metric_name, windows in stats.items():
                    if not isinstance(windows, dict):
                        continue
                    for window_name, row in windows.items():
                        if not isinstance(row, dict):
                            continue
                        writer.writerow(
                            {
                                "metric": metric_name,
                                "window": window_name,
                                "min": row.get("min"),
                                "max": row.get("max"),
                                "mean": row.get("mean"),
                                "median": row.get("median"),
                                "p95": row.get("p95"),
                                "p99": row.get("p99"),
                                "stddev": row.get("stddev"),
                            }
                        )

    def _run_plotter(self):
        plotter = Path(__file__).resolve().parent / "plot_results.py"
        python_bin = shutil.which("python3") or shutil.which("python") or sys.executable or "python"
        subprocess.run([python_bin, str(plotter), "--results-dir", str(self.results_dir)], check=False)

    def _print_summary(self, metrics: dict):
        print("\nBenchmark complete.")
        print(f"Results directory: {self.results_dir}")
        print(f"Cold start time: {metrics.get('cold_start_time_s')} s")
        print(f"Idle memory: {metrics.get('idle_memory_mb')} MB")
        print(f"Peak memory: {metrics.get('peak_memory_mb')} MB")
        print(f"Idle CPU: {metrics.get('idle_cpu_percent')} %")
        print(f"App binary size: {metrics.get('app_binary_size_human')}")
        print(f"Total disk footprint: {metrics.get('total_disk_footprint_human')}")

    def _graceful_shutdown(self) -> Optional[float]:
        target_pid = self.measure_pid or self.root_pid
        if not target_pid:
            return None

        start = time.time()
        try:
            os.kill(target_pid, signal.SIGTERM)
        except ProcessLookupError:
            return 0.0

        timeout_s = self.args.shutdown_timeout_s
        while time.time() - start < timeout_s:
            if not psutil.pid_exists(target_pid):
                return round(time.time() - start, 4)
            descendants = self._descendant_pids(target_pid)
            if not descendants:
                return round(time.time() - start, 4)
            time.sleep(0.2)

        try:
            os.kill(target_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

        # If we launched via a wrapper shell, attempt to stop it too.
        if self.root_pid and self.root_pid != target_pid:
            try:
                os.kill(self.root_pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        return round(time.time() - start, 4)

    def _rss_mb_tree(self) -> float:
        pid = self.measure_pid or self.root_pid
        if not pid or not psutil.pid_exists(pid):
            return 0.0

        pids = [pid] + self._descendant_pids(pid)
        total = 0
        for pid in pids:
            try:
                proc = psutil.Process(pid)
                total += proc.memory_info().rss
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
        return total / (1024 * 1024)

    def _cpu_percent_tree(self) -> Dict[str, float]:
        """Return process-tree CPU usage as percent of one core using time deltas.

        Using psutil.Process.cpu_percent(interval=0.0) can return unstable zeros without
        per-process priming. This delta-based method is deterministic across platforms.
        """
        pid = self.measure_pid or self.root_pid
        if not pid or not psutil.pid_exists(pid):
            self._last_cpu_snapshot = None
            return {
                "total": 0.0,
                "user": 0.0,
                "system": 0.0,
                "normalized": 0.0,
            }

        pids = [pid] + self._descendant_pids(pid)
        total_user = 0.0
        total_system = 0.0
        for pid in pids:
            try:
                proc = psutil.Process(pid)
                cpu_times = proc.cpu_times()
                total_user += float(getattr(cpu_times, "user", 0.0))
                total_system += float(getattr(cpu_times, "system", 0.0))
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue

        now = time.time()
        current = {
            "ts": now,
            "user_s": total_user,
            "system_s": total_system,
        }

        if self._last_cpu_snapshot is None:
            self._last_cpu_snapshot = current
            return {
                "total": 0.0,
                "user": 0.0,
                "system": 0.0,
                "normalized": 0.0,
            }

        dt = max(0.0, current["ts"] - self._last_cpu_snapshot["ts"])
        delta_user = max(0.0, current["user_s"] - self._last_cpu_snapshot["user_s"])
        delta_system = max(0.0, current["system_s"] - self._last_cpu_snapshot["system_s"])
        self._last_cpu_snapshot = current

        if dt <= 0.0:
            return {
                "total": 0.0,
                "user": 0.0,
                "system": 0.0,
                "normalized": 0.0,
            }

        total_pct = ((delta_user + delta_system) / dt) * 100.0
        user_pct = (delta_user / dt) * 100.0
        system_pct = (delta_system / dt) * 100.0
        cpu_count = max(1, int(psutil.cpu_count(logical=True) or 1))
        normalized_pct = total_pct / float(cpu_count)
        return {
            "total": total_pct,
            "user": user_pct,
            "system": system_pct,
            "normalized": normalized_pct,
        }

    def _process_count_tree(self) -> int:
        pid = self.measure_pid or self.root_pid
        if not pid or not psutil.pid_exists(pid):
            return 0
        return 1 + len(self._descendant_pids(pid))

    def _descendant_pids(self, pid: int) -> List[int]:
        try:
            proc = psutil.Process(pid)
            return [child.pid for child in proc.children(recursive=True)]
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            return []

    def _resolve_binary_path(self) -> Optional[Path]:
        if self.args.app_binary_path:
            path = Path(self.args.app_binary_path).resolve()
            return path if path.exists() else None

        candidates = [
            self.repo_root / "genhat-desktop" / "src-tauri" / "target" / "release" / "genhat-desktop",
            self.repo_root / "genhat-desktop" / "src-tauri" / "target" / "release" / "GenHat",
            self.repo_root / "genhat-desktop" / "src-tauri" / "target" / "release" / "bundle" / "deb",
            self.repo_root / "genhat-desktop" / "src-tauri" / "target" / "release" / "bundle" / "appimage",
        ]

        for candidate in candidates:
            if candidate.exists():
                if candidate.is_file():
                    return candidate
                files = list(candidate.rglob("*"))
                file_candidates = [f for f in files if f.is_file()]
                if file_candidates:
                    file_candidates.sort(key=lambda p: p.stat().st_size, reverse=True)
                    return file_candidates[0]

        pid = self.measure_pid or self.root_pid
        if pid and psutil.pid_exists(pid):
            try:
                exe = Path(psutil.Process(pid).exe())
                if exe.exists():
                    return exe
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass

        return None

    def _file_size_bytes(self, path: Path) -> int:
        if not path.exists():
            return 0
        if path.is_file():
            return path.stat().st_size
        total = 0
        for child in path.rglob("*"):
            if child.is_file():
                total += child.stat().st_size
        return total

    def _du_size(self, path: Path) -> Tuple[int, str]:
        if not path.exists():
            return 0, "0B"

        du_bin = self._which("du")
        if du_bin:
            proc = subprocess.run([du_bin, "-sb", str(path)], capture_output=True, text=True, check=False)
            if proc.returncode == 0 and proc.stdout.strip():
                size = int(proc.stdout.split()[0])
                return size, self._human_size(size)

        size = self._file_size_bytes(path)
        return size, self._human_size(size)

    def _ls_size_human(self, path: Path) -> str:
        ls_bin = self._which("ls")
        if not ls_bin or not path.exists():
            return self._human_size(self._file_size_bytes(path))

        proc = subprocess.run([ls_bin, "-lh", str(path)], capture_output=True, text=True, check=False)
        if proc.returncode == 0 and proc.stdout.strip():
            last = proc.stdout.strip().splitlines()[-1]
            parts = last.split()
            if len(parts) >= 5:
                return parts[4]

        return self._human_size(self._file_size_bytes(path))

    def _pstree_snapshot(self, pid: int) -> str:
        pstree_bin = self._which("pstree")
        if not pstree_bin:
            return "pstree not found"
        proc = subprocess.run([pstree_bin, "-p", str(pid)], capture_output=True, text=True, check=False)
        return proc.stdout.strip() if proc.returncode == 0 else "pstree failed"

    def _ps_rss_mb_tree(self) -> Optional[float]:
        ps_bin = self._which("ps")
        pid = self.measure_pid or self.root_pid
        if not ps_bin or not pid or not psutil.pid_exists(pid):
            return None

        pids = [pid] + self._descendant_pids(pid)
        pid_arg = ",".join(str(pid) for pid in pids)
        proc = subprocess.run(
            [ps_bin, "-o", "rss=", "-p", pid_arg],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            return None

        total_kb = 0
        for line in proc.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                total_kb += int(line)
            except ValueError:
                continue

        return total_kb / 1024.0

    def _smem_rss_mb_tree(self) -> Optional[float]:
        smem_bin = self._which("smem")
        pid = self.measure_pid or self.root_pid
        if not smem_bin or not pid or not psutil.pid_exists(pid):
            return None

        pids = [pid] + self._descendant_pids(pid)
        total_kb = 0

        for pid in pids:
            proc = subprocess.run(
                [smem_bin, "-P", f"^{pid}$", "-c", "pid rss", "-H"],
                capture_output=True,
                text=True,
                check=False,
            )
            if proc.returncode != 0:
                continue
            for line in proc.stdout.splitlines():
                parts = line.split()
                if len(parts) >= 2 and parts[0].isdigit():
                    try:
                        total_kb += int(parts[1])
                    except ValueError:
                        pass

        if total_kb == 0:
            return None
        return total_kb / 1024.0

    def _pidstat_cpu_percent(self) -> Optional[float]:
        pidstat_bin = self._which("pidstat")
        pid = self.measure_pid or self.root_pid
        if not pidstat_bin or not pid:
            return None

        proc = subprocess.run(
            [pidstat_bin, "-u", "-p", str(pid), "1", "1"],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            return None

        avg_cpu = None
        for line in proc.stdout.splitlines():
            if "Average:" in line and line.strip().endswith(str(pid)) is False:
                parts = line.split()
                # expected tail format includes %usr %system %guest %wait %CPU CPU Command
                for idx, token in enumerate(parts):
                    if token.replace(".", "", 1).isdigit() and idx + 2 < len(parts):
                        # try to identify %CPU as the 7th numeric in Average line
                        pass
            if "Average:" in line and str(pid) in line:
                parts = line.split()
                numeric = []
                for token in parts:
                    try:
                        numeric.append(float(token))
                    except ValueError:
                        continue
                if numeric:
                    # pidstat Average row numeric order usually starts with PID then %usr %system %guest %wait %CPU CPU
                    # choose the second last numeric as %CPU when available.
                    if len(numeric) >= 3:
                        avg_cpu = numeric[-2]
                    else:
                        avg_cpu = numeric[-1]

        return avg_cpu

    def _proc_status_snapshot(self) -> dict:
        pid = self.measure_pid or self.root_pid
        if not pid:
            return {}
        status_path = Path(f"/proc/{pid}/status")
        if not status_path.exists():
            return {}

        wanted = {"Name", "State", "VmRSS", "VmSize", "Threads"}
        snapshot = {}
        try:
            for line in status_path.read_text(encoding="utf-8", errors="replace").splitlines():
                if ":" not in line:
                    continue
                key, value = line.split(":", 1)
                key = key.strip()
                if key in wanted:
                    snapshot[key] = value.strip()
        except OSError:
            return {}

        return snapshot

    def _proc_smaps_rollup_kb(self) -> Optional[int]:
        pid = self.measure_pid or self.root_pid
        if not pid:
            return None
        smaps_path = Path(f"/proc/{pid}/smaps_rollup")
        if not smaps_path.exists():
            return None

        try:
            for line in smaps_path.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("Rss:"):
                    parts = line.split()
                    if len(parts) >= 2:
                        return int(parts[1])
        except (OSError, ValueError):
            return None

        return None

    def _estimate_health_check_overhead(self) -> float:
        if not self.samples:
            return 0.0

        baseline_window_s = min(10, self.args.idle_window_s)
        ready_elapsed = 0.0
        if self.start_time and self.ready_time is not None:
            ready_elapsed = max(0.0, self.ready_time - self.start_time)

        baseline = [
            s.cpu_percent
            for s in self.samples
            if ready_elapsed <= s.elapsed_s <= ready_elapsed + baseline_window_s
        ]
        if not baseline:
            baseline = [s.cpu_percent for s in self.samples[:10]]
        baseline_median = self._median(baseline)

        interval = self.lifecycle_interval_s
        peak_points = []
        for sample in self.samples:
            if sample.elapsed_s < interval:
                continue
            phase = sample.elapsed_s % interval
            if phase <= 1.0 or phase >= interval - 1.0:
                peak_points.append(sample.cpu_percent)

        if not peak_points:
            return 0.0

        peak_mean = sum(peak_points) / len(peak_points)
        overhead = max(0.0, peak_mean - baseline_median)
        return overhead

    def _median(self, values: List[float]) -> float:
        if not values:
            return 0.0
        arr = sorted(values)
        n = len(arr)
        mid = n // 2
        if n % 2 == 1:
            return float(arr[mid])
        return float((arr[mid - 1] + arr[mid]) / 2.0)

    def _human_size(self, size: int) -> str:
        units = ["B", "KB", "MB", "GB", "TB"]
        value = float(size)
        for unit in units:
            if value < 1024.0 or unit == units[-1]:
                return f"{value:.2f}{unit}"
            value /= 1024.0
        return f"{size}B"

    def _which(self, cmd: str) -> Optional[str]:
        return shutil.which(cmd)

    def _detect_capabilities(self) -> dict:
        has_procfs = Path("/proc").exists()
        supports_num_fds = hasattr(psutil.Process, "num_fds")
        return {
            "platform": platform.system().lower(),
            "procfs": has_procfs,
            "smaps_rollup": has_procfs and bool(getattr(self.args, "enable_smaps_rollup", True)),
            "proc_io": has_procfs and bool(getattr(self.args, "enable_proc_io", True)),
            "proc_faults": has_procfs and bool(getattr(self.args, "enable_proc_faults", True)),
            "proc_ctx_switches": has_procfs and bool(getattr(self.args, "enable_proc_ctx_switches", True)),
            "open_fds": supports_num_fds and bool(getattr(self.args, "enable_proc_fds", True)),
            "backend_process_telemetry": True,
        }

    def _round_stats(self, stats: Dict[str, float], ndigits: int = 4) -> Dict[str, float]:
        return {k: round(float(v), ndigits) for k, v in stats.items()}

    def _percentile(self, values: List[float], percentile: float) -> float:
        if not values:
            return 0.0
        arr = sorted(float(v) for v in values)
        if len(arr) == 1:
            return arr[0]
        rank = (len(arr) - 1) * (percentile / 100.0)
        lower = int(math.floor(rank))
        upper = int(math.ceil(rank))
        if lower == upper:
            return arr[lower]
        weight = rank - lower
        return arr[lower] * (1.0 - weight) + arr[upper] * weight

    def _series_stats(self, values: List[float]) -> Dict[str, float]:
        arr = [float(v) for v in values]
        if not arr:
            return {
                "min": 0.0,
                "max": 0.0,
                "mean": 0.0,
                "median": 0.0,
                "p95": 0.0,
                "p99": 0.0,
                "stddev": 0.0,
            }
        stddev = statistics.pstdev(arr) if len(arr) > 1 else 0.0
        return {
            "min": min(arr),
            "max": max(arr),
            "mean": statistics.fmean(arr),
            "median": self._median(arr),
            "p95": self._percentile(arr, 95.0),
            "p99": self._percentile(arr, 99.0),
            "stddev": stddev,
        }


PROFILE_DEFAULTS = {
    "quick": {
        "sample_interval_s": 1.0,
        "extended_sample_interval_s": 5.0,
        "idle_window_s": 30,
        "model_load_window_s": 60,
    },
    "standard": {
        "sample_interval_s": 1.0,
        "extended_sample_interval_s": 5.0,
        "idle_window_s": 90,
        "model_load_window_s": 180,
    },
    "long": {
        "sample_interval_s": 0.5,
        "extended_sample_interval_s": 2.0,
        "idle_window_s": 180,
        "model_load_window_s": 300,
    },
}


def _apply_profile_defaults(args: argparse.Namespace, argv: List[str]) -> argparse.Namespace:
    defaults = PROFILE_DEFAULTS.get(args.profile, PROFILE_DEFAULTS["standard"])
    flag_map = {
        "sample_interval_s": ["--sample-interval-s"],
        "extended_sample_interval_s": ["--extended-sample-interval-s"],
        "idle_window_s": ["--idle-window-s"],
        "model_load_window_s": ["--model-load-window-s"],
    }
    for attr, value in defaults.items():
        if not any(flag in argv for flag in flag_map[attr]):
            setattr(args, attr, value)
    if float(getattr(args, "duration_s", 0.0)) < 0:
        raise ValueError("--duration-s must be >= 0")
    return args



def parse_args():
    parser = argparse.ArgumentParser(description="GenHat application benchmark suite (system + model metrics + charts)")

    parser.add_argument("--repo-root", default=".", help="Repo root (contains genhat-desktop and models)")
    parser.add_argument("--models-dir", default=None, help="Path to models directory")
    parser.add_argument("--output-dir", default="benchmark/results", help="Output directory for benchmark runs")
    parser.add_argument(
        "--profile",
        choices=["quick", "standard", "long"],
        default="standard",
        help="Benchmark profile that sets default sampling and timing windows.",
    )

    parser.add_argument("--mode", choices=["launch", "attach"], default="launch", help="launch: start app, attach: monitor existing process")
    parser.add_argument("--launch-cmd", default="cd genhat-desktop && npx tauri dev", help="Command used in launch mode")
    parser.add_argument("--attach-pid", type=int, default=None, help="Root PID in attach mode")
    parser.add_argument("--attach-name", default="genhat", help="Process name/cmdline match in attach mode")
    parser.add_argument("--tauri-log-file", default=None, help="Optional tauri log file to parse in attach mode")

    parser.add_argument("--ready-regex", default=r"Lifecycle manager started|RAG enrichment worker started", help="Regex marking first interactive readiness")
    parser.add_argument("--ready-timeout-s", type=int, default=180, help="Timeout to wait for readiness pattern")

    parser.add_argument(
        "--sanitize-launch-env",
        action="store_true",
        default=(os.name == "posix"),
        help="Best-effort remove Snap/LD_LIBRARY_PATH injection when launching (helps avoid libpthread symbol errors)",
    )

    parser.add_argument("--sample-interval-s", type=float, default=1.0, help="Sampling interval for RSS/CPU/process count")
    parser.add_argument(
        "--extended-sample-interval-s",
        type=float,
        default=5.0,
        help="Sampling interval for extended /proc metrics (PSS/USS, IO, faults, fds, ctx switches). 0 disables.",
    )
    # Toggle-able extended collectors (enabled by default for exhaustive runs).
    parser.add_argument(
        "--smaps-rollup",
        dest="enable_smaps_rollup",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Enable reading /proc/<pid>/smaps_rollup for PSS/USS (heavier; disable with --no-smaps-rollup).",
    )
    parser.add_argument(
        "--proc-io",
        dest="enable_proc_io",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Enable reading /proc/<pid>/io counters (disable with --no-proc-io).",
    )
    parser.add_argument(
        "--proc-faults",
        dest="enable_proc_faults",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Enable reading /proc/<pid>/stat page-fault counters (disable with --no-proc-faults).",
    )
    parser.add_argument(
        "--proc-fds",
        dest="enable_proc_fds",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Enable collecting open FD counts (disable with --no-proc-fds).",
    )
    parser.add_argument(
        "--proc-ctx-switches",
        dest="enable_proc_ctx_switches",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Enable collecting context switches from /proc/<pid>/status (disable with --no-proc-ctx-switches).",
    )
    parser.add_argument("--idle-window-s", type=int, default=90, help="Seconds to capture idle metrics")
    parser.add_argument("--model-load-window-s", type=int, default=180, help="Window to trigger and observe model loading")
    parser.add_argument("--interactive", action="store_true", help="Ask for Enter key during model loading phase")

    parser.add_argument(
        "--run-until-exit",
        action="store_true",
        help="Keep sampling until the app process exits (close the app normally to stop).",
    )
    parser.add_argument(
        "--duration-s",
        type=float,
        default=0.0,
        help="Fixed benchmark duration in seconds after readiness (0 keeps idle/model window phases).",
    )

    parser.add_argument("--app-binary-path", default=None, help="Path to app binary (if known)")
    parser.add_argument("--shutdown-after-benchmark", action="store_true", help="Gracefully shut down launched app and measure shutdown time")
    parser.add_argument("--shutdown-timeout-s", type=int, default=45, help="Shutdown timeout before SIGKILL")

    parsed = parser.parse_args()
    return _apply_profile_defaults(parsed, sys.argv[1:])


if __name__ == "__main__":
    arguments = parse_args()
    runner = BenchmarkRunner(arguments)
    runner.run()
