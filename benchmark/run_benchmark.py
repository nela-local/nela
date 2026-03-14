#!/usr/bin/env python3
import argparse
import csv
import json
import os
import re
import shutil
import signal
import subprocess
import threading
import time
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import psutil


SPAWN_RE = re.compile(r"Spawning new instance '([^']+)' for model '([^']+)'")
READY_RE = re.compile(r"Instance '([^']+)' for model '([^']+)' is ready")
LIFECYCLE_RE = re.compile(r"Lifecycle manager started \(interval=(\d+)s\)")


@dataclass
class Sample:
    ts: float
    elapsed_s: float
    rss_mb: float
    cpu_percent: float
    process_count: int


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
        self.monitoring_active = False
        self.start_time: Optional[float] = None
        self.ready_time: Optional[float] = None
        self.lifecycle_interval_s = 30

        self.samples: List[Sample] = []
        self.spawn_events: Dict[str, List[Tuple[float, float]]] = defaultdict(list)
        self.ready_events: Dict[str, List[Tuple[float, float]]] = defaultdict(list)
        self.model_load_rows: List[dict] = []

        self._log_lock = threading.Lock()

    def run(self):
        try:
            if self.args.mode == "launch":
                self._launch_app()
            else:
                self._attach_to_app()

            self._start_monitoring_thread()
            self._wait_for_ready_if_needed()
            self._capture_idle_metrics_phase()
            self._capture_model_loading_phase()
            self._capture_disk_and_binary_metrics()
            metrics = self._finalize_metrics()
            self._write_outputs(metrics)
            self._run_plotter()
            self._print_summary(metrics)
        finally:
            if self.args.mode == "launch" and self.args.shutdown_after_benchmark:
                shutdown_time = self._graceful_shutdown()
                if shutdown_time is not None:
                    metrics_path = self.results_dir / "metrics.json"
                    if metrics_path.exists():
                        metrics = json.loads(metrics_path.read_text(encoding="utf-8"))
                        metrics["graceful_shutdown_time_s"] = shutdown_time
                        metrics_path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")
                        self._run_plotter()
            self.monitoring_active = False

    def _launch_app(self):
        launch_cmd = self.args.launch_cmd
        if not launch_cmd:
            launch_cmd = "cd genhat-desktop && npx tauri dev"

        self.start_time = time.time()
        env = os.environ.copy()
        env.setdefault("RUST_LOG", "info")

        self.root_process = subprocess.Popen(
            ["bash", "-lc", launch_cmd],
            cwd=str(self.repo_root),
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            env=env,
        )
        self.root_pid = self.root_process.pid

        threading.Thread(target=self._consume_stdout, daemon=True).start()

    def _attach_to_app(self):
        self.start_time = time.time()
        if self.args.attach_pid:
            self.root_pid = self.args.attach_pid
            return

        if not self.args.attach_name:
            raise ValueError("In attach mode, provide --attach-pid or --attach-name")

        target_name = self.args.attach_name.lower()
        for proc in psutil.process_iter(attrs=["pid", "name", "cmdline"]):
            name = (proc.info.get("name") or "").lower()
            cmdline = " ".join(proc.info.get("cmdline") or []).lower()
            if target_name in name or target_name in cmdline:
                self.root_pid = proc.info["pid"]
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

        ready_match = READY_RE.search(line)
        if ready_match:
            _instance, model_id = ready_match.groups()
            rss_now = self._rss_mb_tree()
            self.ready_events[model_id].append((ts, rss_now))
            self._resolve_model_row(model_id)

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
            time.sleep(0.25)

        self.ready_time = time.time()

    def _start_monitoring_thread(self):
        self.monitoring_active = True

        def monitor_loop():
            while self.monitoring_active:
                ts = time.time()
                elapsed = ts - (self.start_time or ts)
                rss_mb = self._rss_mb_tree()
                cpu_pct = self._cpu_percent_tree()
                process_count = self._process_count_tree()
                self.samples.append(Sample(ts, elapsed, rss_mb, cpu_pct, process_count))
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

    def _capture_disk_and_binary_metrics(self):
        return

    def _finalize_metrics(self) -> dict:
        self.monitoring_active = False
        time.sleep(self.args.sample_interval_s * 1.2)

        idle_samples = [s for s in self.samples if s.elapsed_s <= self.args.idle_window_s + 2]
        all_samples = self.samples or []

        idle_memory_mb = self._median([s.rss_mb for s in idle_samples]) if idle_samples else 0.0
        idle_cpu_percent = self._median([s.cpu_percent for s in idle_samples]) if idle_samples else 0.0
        peak_memory_mb = max([s.rss_mb for s in all_samples], default=0.0)
        peak_process_count = max([s.process_count for s in all_samples], default=0)

        idle_memory_ps_mb = self._ps_rss_mb_tree()
        idle_memory_smem_mb = self._smem_rss_mb_tree()
        idle_cpu_pidstat = self._pidstat_cpu_percent()
        if idle_cpu_pidstat is not None:
            idle_cpu_percent = idle_cpu_pidstat

        cold_start_time_s = None
        if self.start_time and self.ready_time:
            cold_start_time_s = max(0.0, self.ready_time - self.start_time)

        app_binary_path = self._resolve_binary_path()
        app_binary_size_bytes = self._file_size_bytes(app_binary_path) if app_binary_path else 0
        app_binary_size_human = self._ls_size_human(app_binary_path) if app_binary_path else "N/A"

        models_disk_bytes, models_disk_human = self._du_size(self.models_dir)
        total_disk_bytes = models_disk_bytes + app_binary_size_bytes
        total_disk_human = self._human_size(total_disk_bytes)

        health_overhead = self._estimate_health_check_overhead()
        root_status = self._proc_status_snapshot()
        root_smaps_rollup_kb = self._proc_smaps_rollup_kb()

        metrics = {
            "mode": self.args.mode,
            "timestamp": datetime.now().isoformat(),
            "root_pid": self.root_pid,
            "cold_start_time_s": round(cold_start_time_s or 0.0, 3),
            "idle_memory_mb": round(idle_memory_mb, 2),
            "idle_memory_ps_mb": round(idle_memory_ps_mb, 2) if idle_memory_ps_mb is not None else None,
            "idle_memory_smem_mb": round(idle_memory_smem_mb, 2) if idle_memory_smem_mb is not None else None,
            "per_model_memory_method": "RSS delta between spawn and ready events",
            "peak_memory_mb": round(peak_memory_mb, 2),
            "total_disk_footprint_bytes": int(total_disk_bytes),
            "total_disk_footprint_human": total_disk_human,
            "app_binary_size_bytes": int(app_binary_size_bytes),
            "app_binary_size_human": app_binary_size_human,
            "models_disk_bytes": int(models_disk_bytes),
            "models_disk_human": models_disk_human,
            "idle_cpu_percent": round(idle_cpu_percent, 2),
            "idle_cpu_pidstat_percent": round(idle_cpu_pidstat, 2) if idle_cpu_pidstat is not None else None,
            "peak_process_count": int(peak_process_count),
            "graceful_shutdown_time_s": None,
            "health_check_interval_s": self.lifecycle_interval_s,
            "health_check_overhead_cpu_percent": round(health_overhead, 4),
            "process_count_method": "psutil descendants + pstree snapshots",
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

        if self.root_pid:
            metrics["pstree_snapshot"] = self._pstree_snapshot(self.root_pid)

        return metrics

    def _write_outputs(self, metrics: dict):
        metrics_path = self.results_dir / "metrics.json"
        metrics_path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")

        with (self.results_dir / "samples.csv").open("w", newline="", encoding="utf-8") as file:
            writer = csv.DictWriter(file, fieldnames=["ts", "elapsed_s", "rss_mb", "cpu_percent", "process_count"])
            writer.writeheader()
            for sample in self.samples:
                writer.writerow(
                    {
                        "ts": round(sample.ts, 6),
                        "elapsed_s": round(sample.elapsed_s, 4),
                        "rss_mb": round(sample.rss_mb, 4),
                        "cpu_percent": round(sample.cpu_percent, 4),
                        "process_count": sample.process_count,
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

    def _run_plotter(self):
        plotter = Path(__file__).resolve().parent / "plot_results.py"
        python_bin = shutil.which("python3") or "python3"
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
        if not self.root_pid:
            return None

        start = time.time()
        try:
            os.kill(self.root_pid, signal.SIGTERM)
        except ProcessLookupError:
            return 0.0

        timeout_s = self.args.shutdown_timeout_s
        while time.time() - start < timeout_s:
            if not psutil.pid_exists(self.root_pid):
                return round(time.time() - start, 4)
            descendants = self._descendant_pids(self.root_pid)
            if not descendants:
                return round(time.time() - start, 4)
            time.sleep(0.2)

        try:
            os.kill(self.root_pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        return round(time.time() - start, 4)

    def _rss_mb_tree(self) -> float:
        if not self.root_pid or not psutil.pid_exists(self.root_pid):
            return 0.0

        pids = [self.root_pid] + self._descendant_pids(self.root_pid)
        total = 0
        for pid in pids:
            try:
                proc = psutil.Process(pid)
                total += proc.memory_info().rss
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
        return total / (1024 * 1024)

    def _cpu_percent_tree(self) -> float:
        if not self.root_pid or not psutil.pid_exists(self.root_pid):
            return 0.0

        pids = [self.root_pid] + self._descendant_pids(self.root_pid)
        total = 0.0
        for pid in pids:
            try:
                proc = psutil.Process(pid)
                total += proc.cpu_percent(interval=0.0)
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
        return total

    def _process_count_tree(self) -> int:
        if not self.root_pid or not psutil.pid_exists(self.root_pid):
            return 0
        return 1 + len(self._descendant_pids(self.root_pid))

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

        if self.root_pid and psutil.pid_exists(self.root_pid):
            try:
                exe = Path(psutil.Process(self.root_pid).exe())
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
        if not ps_bin or not self.root_pid or not psutil.pid_exists(self.root_pid):
            return None

        pids = [self.root_pid] + self._descendant_pids(self.root_pid)
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
        if not smem_bin or not self.root_pid or not psutil.pid_exists(self.root_pid):
            return None

        pids = [self.root_pid] + self._descendant_pids(self.root_pid)
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
        if not pidstat_bin or not self.root_pid:
            return None

        proc = subprocess.run(
            [pidstat_bin, "-u", "-p", str(self.root_pid), "1", "1"],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            return None

        avg_cpu = None
        for line in proc.stdout.splitlines():
            if "Average:" in line and line.strip().endswith(str(self.root_pid)) is False:
                parts = line.split()
                # expected tail format includes %usr %system %guest %wait %CPU CPU Command
                for idx, token in enumerate(parts):
                    if token.replace(".", "", 1).isdigit() and idx + 2 < len(parts):
                        # try to identify %CPU as the 7th numeric in Average line
                        pass
            if "Average:" in line and str(self.root_pid) in line:
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
        if not self.root_pid:
            return {}
        status_path = Path(f"/proc/{self.root_pid}/status")
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
        if not self.root_pid:
            return None
        smaps_path = Path(f"/proc/{self.root_pid}/smaps_rollup")
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
        baseline = [s.cpu_percent for s in self.samples if s.elapsed_s <= baseline_window_s]
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



def parse_args():
    parser = argparse.ArgumentParser(description="GenHat benchmark suite (system + model metrics + graphs)")

    parser.add_argument("--repo-root", default=".", help="Repo root (contains genhat-desktop and models)")
    parser.add_argument("--models-dir", default=None, help="Path to models directory")
    parser.add_argument("--output-dir", default="benchmark/results", help="Output directory for benchmark runs")

    parser.add_argument("--mode", choices=["launch", "attach"], default="launch", help="launch: start app, attach: monitor existing process")
    parser.add_argument("--launch-cmd", default="cd genhat-desktop && npx tauri dev", help="Command used in launch mode")
    parser.add_argument("--attach-pid", type=int, default=None, help="Root PID in attach mode")
    parser.add_argument("--attach-name", default="genhat", help="Process name/cmdline match in attach mode")
    parser.add_argument("--tauri-log-file", default=None, help="Optional tauri log file to parse in attach mode")

    parser.add_argument("--ready-regex", default=r"Lifecycle manager started|RAG enrichment worker started", help="Regex marking first interactive readiness")
    parser.add_argument("--ready-timeout-s", type=int, default=180, help="Timeout to wait for readiness pattern")

    parser.add_argument("--sample-interval-s", type=float, default=1.0, help="Sampling interval for RSS/CPU/process count")
    parser.add_argument("--idle-window-s", type=int, default=90, help="Seconds to capture idle metrics")
    parser.add_argument("--model-load-window-s", type=int, default=180, help="Window to trigger and observe model loading")
    parser.add_argument("--interactive", action="store_true", help="Ask for Enter key during model loading phase")

    parser.add_argument("--app-binary-path", default=None, help="Path to app binary (if known)")
    parser.add_argument("--shutdown-after-benchmark", action="store_true", help="Gracefully shut down launched app and measure shutdown time")
    parser.add_argument("--shutdown-timeout-s", type=int, default=45, help="Shutdown timeout before SIGKILL")

    return parser.parse_args()


if __name__ == "__main__":
    arguments = parse_args()
    runner = BenchmarkRunner(arguments)
    runner.run()
