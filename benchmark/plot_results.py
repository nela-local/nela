#!/usr/bin/env python3
import argparse
import csv
import json
from pathlib import Path

import matplotlib.pyplot as plt


def read_csv(path: Path):
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8") as file:
        reader = csv.DictReader(file)
        for row in reader:
            rows.append(row)
    return rows

def plot_process_count(samples, out_path: Path):
    if not samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in samples]
    ys = [to_float(row.get("process_count")) for row in samples]
    plt.figure(figsize=(10, 4.5))
    plt.plot(xs, ys, color="#4C72B0", linewidth=1.6)
    plt.title("Process Count Over Time")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("Process count")
    plt.grid(alpha=0.25)
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_memory_breakdown(ext_samples, out_path: Path):
    if not ext_samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    rss = [to_float(row.get("rss_mb")) for row in ext_samples]
    pss = [to_float(row.get("pss_mb"), None) for row in ext_samples]
    uss = [to_float(row.get("uss_mb"), None) for row in ext_samples]

    plt.figure(figsize=(10, 4.8))
    plt.plot(xs, rss, label="RSS", color="#4C72B0", linewidth=1.6)
    if any(v is not None for v in pss):
        plt.plot(xs, [v if v is not None else float('nan') for v in pss], label="PSS", color="#55A868", linewidth=1.6)
    if any(v is not None for v in uss):
        plt.plot(xs, [v if v is not None else float('nan') for v in uss], label="USS", color="#C44E52", linewidth=1.6)
    plt.title("Memory Breakdown Over Time")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("MB")
    plt.grid(alpha=0.25)
    plt.legend()
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_io_rates(ext_samples, out_path: Path):
    if not ext_samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    rr = [to_float(row.get("read_rate_bps"), None) for row in ext_samples]
    wr = [to_float(row.get("write_rate_bps"), None) for row in ext_samples]

    if not any(v is not None for v in rr) and not any(v is not None for v in wr):
        return

    rr_mb = [(v / (1024 * 1024)) if v is not None else float('nan') for v in rr]
    wr_mb = [(v / (1024 * 1024)) if v is not None else float('nan') for v in wr]

    plt.figure(figsize=(10, 4.8))
    plt.plot(xs, rr_mb, label="Read MB/s", color="#4C72B0", linewidth=1.6)
    plt.plot(xs, wr_mb, label="Write MB/s", color="#55A868", linewidth=1.6)
    plt.title("Disk I/O Rates (Process Tree)")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("MB/s")
    plt.grid(alpha=0.25)
    plt.legend()
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_fault_rates(ext_samples, out_path: Path):
    if not ext_samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    mn = [to_float(row.get("minor_faults_rate"), None) for row in ext_samples]
    mj = [to_float(row.get("major_faults_rate"), None) for row in ext_samples]
    if not any(v is not None for v in mn) and not any(v is not None for v in mj):
        return
    plt.figure(figsize=(10, 4.8))
    plt.plot(xs, [v if v is not None else float('nan') for v in mn], label="Minor faults/s", color="#8172B2", linewidth=1.6)
    plt.plot(xs, [v if v is not None else float('nan') for v in mj], label="Major faults/s", color="#C44E52", linewidth=1.6)
    plt.title("Page Fault Rates (Process Tree)")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("faults/s")
    plt.grid(alpha=0.25)
    plt.legend()
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_threads_fds(ext_samples, out_path: Path):
    if not ext_samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    threads = [to_float(row.get("threads")) for row in ext_samples]
    fds = [to_float(row.get("open_fds"), None) for row in ext_samples]

    plt.figure(figsize=(10, 4.8))
    plt.plot(xs, threads, label="Threads", color="#4C72B0", linewidth=1.6)
    if any(v is not None for v in fds):
        plt.plot(xs, [v if v is not None else float('nan') for v in fds], label="Open FDs", color="#55A868", linewidth=1.6)
    plt.title("Threads / Open File Descriptors")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("count")
    plt.grid(alpha=0.25)
    plt.legend()
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_llama_server_count(ext_samples, out_path: Path):
    if not ext_samples:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    ys = [to_float(row.get("llama_server_count")) for row in ext_samples]
    if not ys:
        return
    plt.figure(figsize=(10, 4.5))
    plt.plot(xs, ys, color="#C44E52", linewidth=1.6)
    plt.title("llama-server Process Count (Process Tree)")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("count")
    plt.grid(alpha=0.25)
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def to_float(value, default=0.0):
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def plot_rss(samples, out_path: Path):
    if not samples:
        return
    times = [to_float(row.get("elapsed_s")) for row in samples]
    rss_mb = [to_float(row.get("rss_mb")) for row in samples]

    plt.figure(figsize=(10, 4.5))
    plt.plot(times, rss_mb, linewidth=2)
    plt.title("GenHat Process Tree RSS Over Time")
    plt.xlabel("Elapsed Time (s)")
    plt.ylabel("RSS (MB)")
    plt.grid(alpha=0.25)
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_cpu(samples, out_path: Path):
    if not samples:
        return
    times = [to_float(row.get("elapsed_s")) for row in samples]
    cpu_pct = [to_float(row.get("cpu_percent")) for row in samples]

    plt.figure(figsize=(10, 4.5))
    plt.plot(times, cpu_pct, linewidth=2, color="#C44E52")
    plt.title("GenHat Process Tree CPU% Over Time")
    plt.xlabel("Elapsed Time (s)")
    plt.ylabel("CPU (%)")
    plt.grid(alpha=0.25)
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_model_loads(rows, out_path: Path):
    if not rows:
        return
    model_ids = [row.get("model_id", "unknown") for row in rows]
    load_s = [to_float(row.get("load_time_s")) for row in rows]

    plt.figure(figsize=(10, 5))
    bars = plt.bar(model_ids, load_s, color="#4C72B0")
    plt.title("Model Load Time (Spawn → Ready)")
    plt.xlabel("Model ID")
    plt.ylabel("Load Time (s)")
    plt.xticks(rotation=30, ha="right")
    plt.grid(axis="y", alpha=0.25)

    for bar, value in zip(bars, load_s):
        plt.text(bar.get_x() + bar.get_width() / 2, bar.get_height(), f"{value:.2f}s", ha="center", va="bottom", fontsize=8)

    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_model_memory(rows, out_path: Path):
    if not rows:
        return
    model_ids = [row.get("model_id", "unknown") for row in rows]
    delta_mb = [to_float(row.get("rss_delta_mb")) for row in rows]

    plt.figure(figsize=(10, 5))
    bars = plt.bar(model_ids, delta_mb, color="#55A868")
    plt.title("Per-Model Memory Delta (RSS at Ready - RSS at Spawn)")
    plt.xlabel("Model ID")
    plt.ylabel("RSS Delta (MB)")
    plt.xticks(rotation=30, ha="right")
    plt.grid(axis="y", alpha=0.25)

    for bar, value in zip(bars, delta_mb):
        plt.text(bar.get_x() + bar.get_width() / 2, bar.get_height(), f"{value:.1f}MB", ha="center", va="bottom", fontsize=8)

    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_summary(metrics: dict, out_path: Path):
    keys = [
        "cold_start_time_s",
        "idle_memory_mb",
        "idle_pss_mb",
        "peak_memory_mb",
        "peak_pss_mb",
        "idle_cpu_percent",
        "graceful_shutdown_time_s",
        "health_check_overhead_cpu_percent",
    ]

    labels = [
        "Cold Start (s)",
        "Idle RSS (MB)",
        "Idle PSS (MB)",
        "Peak RSS (MB)",
        "Peak PSS (MB)",
        "Idle CPU (%)",
        "Shutdown (s)",
        "Lifecycle CPU%",
    ]

    values = [to_float(metrics.get(key), 0.0) for key in keys]

    plt.figure(figsize=(9.5, 4.5))
    bars = plt.bar(labels, values, color="#8172B2")
    plt.title("GenHat Benchmark Summary")
    plt.ylabel("Value")
    plt.xticks(rotation=20, ha="right")
    plt.grid(axis="y", alpha=0.25)

    for bar, value in zip(bars, values):
        plt.text(bar.get_x() + bar.get_width() / 2, bar.get_height(), f"{value:.2f}", ha="center", va="bottom", fontsize=8)

    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def main():
    parser = argparse.ArgumentParser(description="Generate benchmark graphs from benchmark output files")
    parser.add_argument("--results-dir", required=True, help="Directory containing metrics.json and CSV files")
    args = parser.parse_args()

    results_dir = Path(args.results_dir).resolve()
    plots_dir = results_dir / "plots"
    plots_dir.mkdir(parents=True, exist_ok=True)

    metrics_path = results_dir / "metrics.json"
    if not metrics_path.exists():
        raise FileNotFoundError(f"metrics.json not found: {metrics_path}")

    metrics = json.loads(metrics_path.read_text(encoding="utf-8"))
    samples = read_csv(results_dir / "samples.csv")
    extended_samples = read_csv(results_dir / "extended_samples.csv")
    model_metrics = read_csv(results_dir / "model_metrics.csv")

    plot_rss(samples, plots_dir / "rss_over_time.png")
    plot_cpu(samples, plots_dir / "cpu_over_time.png")
    plot_process_count(samples, plots_dir / "process_count_over_time.png")
    plot_memory_breakdown(extended_samples, plots_dir / "memory_breakdown_over_time.png")
    plot_io_rates(extended_samples, plots_dir / "io_rates_over_time.png")
    plot_fault_rates(extended_samples, plots_dir / "fault_rates_over_time.png")
    plot_threads_fds(extended_samples, plots_dir / "threads_fds_over_time.png")
    plot_llama_server_count(extended_samples, plots_dir / "llama_server_count_over_time.png")
    plot_model_loads(model_metrics, plots_dir / "model_load_time.png")
    plot_model_memory(model_metrics, plots_dir / "model_memory_delta.png")
    plot_summary(metrics, plots_dir / "summary_metrics.png")

    print(f"Graphs generated in: {plots_dir}")


if __name__ == "__main__":
    main()
