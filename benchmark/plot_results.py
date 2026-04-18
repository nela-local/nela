#!/usr/bin/env python3
import argparse
import csv
import json
from pathlib import Path
from typing import Dict, List, Optional

try:
    import matplotlib.pyplot as plt
except Exception:
    plt = None

try:
    import plotly.graph_objects as go
except Exception:
    go = None


def read_csv(path: Path):
    rows = []
    if not path.exists():
        return rows
    with path.open("r", encoding="utf-8") as file:
        reader = csv.DictReader(file)
        for row in reader:
            rows.append(row)
    return rows


def _valid_points(xs: List[float], ys: List[Optional[float]]):
    out_x = []
    out_y = []
    for x, y in zip(xs, ys):
        if y is None:
            continue
        out_x.append(x)
        out_y.append(y)
    return out_x, out_y


def _write_plotly_html(fig, out_path: Path):
    if go is None:
        return
    fig.write_html(str(out_path), include_plotlyjs="cdn", full_html=True)

def plot_process_count(samples, out_path: Path):
    if not samples or plt is None:
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


def plot_process_count_html(samples, out_path: Path):
    if not samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in samples]
    ys = [to_float(row.get("process_count")) for row in samples]
    fig = go.Figure()
    fig.add_trace(go.Scatter(x=xs, y=ys, mode="lines", name="Process Count", line={"color": "#4C72B0"}))
    fig.update_layout(title="Process Count Over Time", xaxis_title="Elapsed (s)", yaxis_title="Process count")
    _write_plotly_html(fig, out_path)


def plot_memory_breakdown(ext_samples, out_path: Path):
    if not ext_samples or plt is None:
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


def plot_memory_breakdown_html(ext_samples, out_path: Path):
    if not ext_samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    rss = [to_float(row.get("rss_mb"), None) for row in ext_samples]
    pss = [to_float(row.get("pss_mb"), None) for row in ext_samples]
    uss = [to_float(row.get("uss_mb"), None) for row in ext_samples]

    fig = go.Figure()
    x_rss, y_rss = _valid_points(xs, rss)
    if y_rss:
        fig.add_trace(go.Scatter(x=x_rss, y=y_rss, mode="lines", name="RSS", line={"color": "#4C72B0"}))
    x_pss, y_pss = _valid_points(xs, pss)
    if y_pss:
        fig.add_trace(go.Scatter(x=x_pss, y=y_pss, mode="lines", name="PSS", line={"color": "#55A868"}))
    x_uss, y_uss = _valid_points(xs, uss)
    if y_uss:
        fig.add_trace(go.Scatter(x=x_uss, y=y_uss, mode="lines", name="USS", line={"color": "#C44E52"}))

    if not fig.data:
        return
    fig.update_layout(title="Memory Breakdown Over Time", xaxis_title="Elapsed (s)", yaxis_title="MB")
    _write_plotly_html(fig, out_path)


def plot_io_rates(ext_samples, out_path: Path):
    if not ext_samples or plt is None:
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


def plot_io_rates_html(ext_samples, out_path: Path):
    if not ext_samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    rr = [to_float(row.get("read_rate_bps"), None) for row in ext_samples]
    wr = [to_float(row.get("write_rate_bps"), None) for row in ext_samples]

    fig = go.Figure()
    x_rr, y_rr = _valid_points(xs, [(v / (1024 * 1024)) if v is not None else None for v in rr])
    x_wr, y_wr = _valid_points(xs, [(v / (1024 * 1024)) if v is not None else None for v in wr])
    if y_rr:
        fig.add_trace(go.Scatter(x=x_rr, y=y_rr, mode="lines", name="Read MB/s", line={"color": "#4C72B0"}))
    if y_wr:
        fig.add_trace(go.Scatter(x=x_wr, y=y_wr, mode="lines", name="Write MB/s", line={"color": "#55A868"}))
    if not fig.data:
        return
    fig.update_layout(title="Disk I/O Rates (Process Tree)", xaxis_title="Elapsed (s)", yaxis_title="MB/s")
    _write_plotly_html(fig, out_path)


def plot_fault_rates(ext_samples, out_path: Path):
    if not ext_samples or plt is None:
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


def plot_fault_rates_html(ext_samples, out_path: Path):
    if not ext_samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    mn = [to_float(row.get("minor_faults_rate"), None) for row in ext_samples]
    mj = [to_float(row.get("major_faults_rate"), None) for row in ext_samples]
    fig = go.Figure()
    x_mn, y_mn = _valid_points(xs, mn)
    x_mj, y_mj = _valid_points(xs, mj)
    if y_mn:
        fig.add_trace(go.Scatter(x=x_mn, y=y_mn, mode="lines", name="Minor faults/s", line={"color": "#8172B2"}))
    if y_mj:
        fig.add_trace(go.Scatter(x=x_mj, y=y_mj, mode="lines", name="Major faults/s", line={"color": "#C44E52"}))
    if not fig.data:
        return
    fig.update_layout(title="Page Fault Rates (Process Tree)", xaxis_title="Elapsed (s)", yaxis_title="faults/s")
    _write_plotly_html(fig, out_path)


def plot_threads_fds(ext_samples, out_path: Path):
    if not ext_samples or plt is None:
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


def plot_threads_fds_html(ext_samples, out_path: Path):
    if not ext_samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    threads = [to_float(row.get("threads"), None) for row in ext_samples]
    fds = [to_float(row.get("open_fds"), None) for row in ext_samples]
    fig = go.Figure()
    x_th, y_th = _valid_points(xs, threads)
    x_fd, y_fd = _valid_points(xs, fds)
    if y_th:
        fig.add_trace(go.Scatter(x=x_th, y=y_th, mode="lines", name="Threads", line={"color": "#4C72B0"}))
    if y_fd:
        fig.add_trace(go.Scatter(x=x_fd, y=y_fd, mode="lines", name="Open FDs", line={"color": "#55A868"}))
    if not fig.data:
        return
    fig.update_layout(title="Threads / Open File Descriptors", xaxis_title="Elapsed (s)", yaxis_title="count")
    _write_plotly_html(fig, out_path)


def plot_llama_server_count(ext_samples, out_path: Path):
    if not ext_samples or plt is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    ys = [to_float(row.get("llama_server_count")) for row in ext_samples]
    if not ys:
        return
    plt.figure(figsize=(10, 4.5))
    plt.plot(xs, ys, color="#C44E52", linewidth=1.6)
    plt.title("Backend Process Count (llama-server matches)")
    plt.xlabel("Elapsed (s)")
    plt.ylabel("count")
    plt.grid(alpha=0.25)
    plt.tight_layout()
    plt.savefig(out_path, dpi=140)
    plt.close()


def plot_backend_process_count_html(ext_samples, out_path: Path):
    if not ext_samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in ext_samples]
    ys = [to_float(row.get("llama_server_count"), None) for row in ext_samples]
    x_ok, y_ok = _valid_points(xs, ys)
    if not y_ok:
        return
    fig = go.Figure()
    fig.add_trace(go.Scatter(x=x_ok, y=y_ok, mode="lines", name="Backend count", line={"color": "#C44E52"}))
    fig.update_layout(
        title="Backend Process Count (llama-server matches)",
        xaxis_title="Elapsed (s)",
        yaxis_title="count",
    )
    _write_plotly_html(fig, out_path)


def to_float(value, default=0.0):
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def plot_rss(samples, out_path: Path):
    if not samples or plt is None:
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


def plot_rss_html(samples, out_path: Path):
    if not samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in samples]
    ys = [to_float(row.get("rss_mb")) for row in samples]
    fig = go.Figure()
    fig.add_trace(go.Scatter(x=xs, y=ys, mode="lines", name="RSS", line={"color": "#4C72B0"}))
    fig.update_layout(title="GenHat Process Tree RSS Over Time", xaxis_title="Elapsed Time (s)", yaxis_title="RSS (MB)")
    _write_plotly_html(fig, out_path)


def plot_cpu(samples, out_path: Path):
    if not samples or plt is None:
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


def plot_cpu_html(samples, out_path: Path):
    if not samples or go is None:
        return
    xs = [to_float(row.get("elapsed_s")) for row in samples]
    ys = [to_float(row.get("cpu_percent")) for row in samples]
    fig = go.Figure()
    fig.add_trace(go.Scatter(x=xs, y=ys, mode="lines", name="CPU%", line={"color": "#C44E52"}))
    fig.update_layout(title="GenHat Process Tree CPU% Over Time", xaxis_title="Elapsed Time (s)", yaxis_title="CPU (%)")
    _write_plotly_html(fig, out_path)


def plot_model_loads(rows, out_path: Path):
    if not rows or plt is None:
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


def plot_model_loads_html(rows, out_path: Path):
    if not rows or go is None:
        return
    model_ids = [row.get("model_id", "unknown") for row in rows]
    load_s = [to_float(row.get("load_time_s")) for row in rows]
    fig = go.Figure()
    fig.add_trace(go.Bar(x=model_ids, y=load_s, marker_color="#4C72B0", name="Load time (s)"))
    fig.update_layout(title="Model Load Time (Spawn -> Ready)", xaxis_title="Model ID", yaxis_title="Load Time (s)")
    _write_plotly_html(fig, out_path)


def plot_model_memory(rows, out_path: Path):
    if not rows or plt is None:
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


def plot_model_memory_html(rows, out_path: Path):
    if not rows or go is None:
        return
    model_ids = [row.get("model_id", "unknown") for row in rows]
    delta_mb = [to_float(row.get("rss_delta_mb")) for row in rows]
    fig = go.Figure()
    fig.add_trace(go.Bar(x=model_ids, y=delta_mb, marker_color="#55A868", name="RSS delta (MB)"))
    fig.update_layout(
        title="Per-Model Memory Delta (RSS at Ready - RSS at Spawn)",
        xaxis_title="Model ID",
        yaxis_title="RSS Delta (MB)",
    )
    _write_plotly_html(fig, out_path)


def plot_summary(metrics: dict, out_path: Path):
    if plt is None:
        return
    keys = [
        "cold_start_time_s",
        "idle_memory_mb",
        "idle_pss_mb",
        "peak_memory_mb",
        "peak_pss_mb",
        "idle_cpu_percent",
        "graceful_shutdown_time_s",
        "health_check_overhead_cpu_percent",
        "duration_s",
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
        "Duration (s)",
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


def plot_summary_html(metrics: dict, out_path: Path):
    if go is None:
        return
    keys = [
        "cold_start_time_s",
        "idle_memory_mb",
        "peak_memory_mb",
        "idle_cpu_percent",
        "health_check_overhead_cpu_percent",
        "duration_s",
    ]
    labels = [
        "Cold Start (s)",
        "Idle RSS (MB)",
        "Peak RSS (MB)",
        "Idle CPU (%)",
        "Lifecycle CPU%",
        "Duration (s)",
    ]
    values = [to_float(metrics.get(key), 0.0) for key in keys]
    fig = go.Figure()
    fig.add_trace(go.Bar(x=labels, y=values, marker_color="#8172B2"))
    fig.update_layout(title="GenHat Benchmark Summary", xaxis_title="Metric", yaxis_title="Value")
    _write_plotly_html(fig, out_path)


def write_dashboard(metrics: Dict, plots_dir: Path, html_paths: List[Path]):
    dashboard_path = plots_dir / "dashboard.html"
    links = []
    for path in html_paths:
        if path.exists():
            links.append(f'<li><a href="{path.name}">{path.name}</a></li>')
    summary = {
        "profile": metrics.get("profile", "standard"),
        "mode": metrics.get("mode"),
        "cold_start_time_s": metrics.get("cold_start_time_s"),
        "idle_memory_mb": metrics.get("idle_memory_mb"),
        "peak_memory_mb": metrics.get("peak_memory_mb"),
        "idle_cpu_percent": metrics.get("idle_cpu_percent"),
    }
    rows = "".join(f"<tr><th>{k}</th><td>{v}</td></tr>" for k, v in summary.items())
    html = f"""<!doctype html>
<html lang=\"en\">
<head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>Benchmark Dashboard</title>
    <style>
        body {{ font-family: Segoe UI, Tahoma, sans-serif; margin: 24px; color: #1e293b; }}
        h1 {{ margin-bottom: 8px; }}
        .grid {{ display: grid; grid-template-columns: 1fr 1fr; gap: 20px; }}
        @media (max-width: 860px) {{ .grid {{ grid-template-columns: 1fr; }} }}
        table {{ border-collapse: collapse; width: 100%; }}
        th, td {{ border: 1px solid #d0d7de; padding: 8px; text-align: left; }}
        th {{ background: #f3f4f6; width: 45%; }}
        ul {{ line-height: 1.7; }}
    </style>
</head>
<body>
    <h1>GenHat Benchmark Dashboard</h1>
    <p>Interactive charts generated from benchmark run outputs.</p>
    <div class=\"grid\">
        <section>
            <h2>Run Summary</h2>
            <table>{rows}</table>
        </section>
        <section>
            <h2>Interactive Charts</h2>
            <ul>{''.join(links)}</ul>
        </section>
    </div>
</body>
</html>
"""
    dashboard_path.write_text(html, encoding="utf-8")


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

    html_files = [
        plots_dir / "rss_over_time.html",
        plots_dir / "cpu_over_time.html",
        plots_dir / "process_count_over_time.html",
        plots_dir / "memory_breakdown_over_time.html",
        plots_dir / "io_rates_over_time.html",
        plots_dir / "fault_rates_over_time.html",
        plots_dir / "threads_fds_over_time.html",
        plots_dir / "backend_process_count_over_time.html",
        plots_dir / "model_load_time.html",
        plots_dir / "model_memory_delta.html",
        plots_dir / "summary_metrics.html",
    ]
    plot_rss_html(samples, html_files[0])
    plot_cpu_html(samples, html_files[1])
    plot_process_count_html(samples, html_files[2])
    plot_memory_breakdown_html(extended_samples, html_files[3])
    plot_io_rates_html(extended_samples, html_files[4])
    plot_fault_rates_html(extended_samples, html_files[5])
    plot_threads_fds_html(extended_samples, html_files[6])
    plot_backend_process_count_html(extended_samples, html_files[7])
    plot_model_loads_html(model_metrics, html_files[8])
    plot_model_memory_html(model_metrics, html_files[9])
    plot_summary_html(metrics, html_files[10])

    write_dashboard(metrics, plots_dir, html_files)

    print(f"Graphs generated in: {plots_dir}")


if __name__ == "__main__":
    main()
