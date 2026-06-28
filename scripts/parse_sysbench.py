#!/usr/bin/env python3
"""
scripts/parse_sysbench.py
──────────────────────────
Parses benchmark/results/*.txt (sysbench output files) and produces:

  benchmark/results/summary.csv            - all runs in one flat table
  benchmark/results/plots/
      policy_qps.png                       - QPS vs N policies   (threads=32)
      policy_p95.png                       - p95  vs N policies  (threads=32)
      entity_qps.png                       - QPS vs total entities
      entity_p95.png                       - p95  vs total entities
      attr_qps.png                         - QPS vs attrs per entity
      attr_p95.png                         - p95  vs attrs per entity
      concurrency_policy_<N>.png           - QPS vs threads at fixed policy count
      concurrency_entities_<N>.png         - QPS vs threads at fixed entity count

Usage:
  python3 scripts/parse_sysbench.py [--results-dir benchmark/results] [--out benchmark/results/plots]
"""

import argparse
import csv
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    sys.exit("pip install matplotlib")


# ─── Sysbench output parser ───────────────────────────────────────────────────

def parse_sysbench_file(path: Path) -> Optional[Dict]:
    text = path.read_text(errors="replace")

    def find(pattern: str) -> Optional[str]:
        m = re.search(pattern, text, re.MULTILINE)
        return m.group(1) if m else None

    qps     = find(r"transactions:\s+\d+\s+\(([0-9.]+)\s+per sec\.\)")
    avg_lat = find(r"avg:\s+([0-9.]+)")
    min_lat = find(r"min:\s+([0-9.]+)")
    max_lat = find(r"max:\s+([0-9.]+)")
    p95_lat = find(r"95th percentile:\s+([0-9.]+)")
    errors  = find(r"ignored errors:\s+(\d+)")
    total   = find(r"transactions:\s+(\d+)")

    if qps is None:
        return None

    return {
        "qps":       float(qps),
        "avg_ms":    float(avg_lat)  if avg_lat  else None,
        "min_ms":    float(min_lat)  if min_lat  else None,
        "max_ms":    float(max_lat)  if max_lat  else None,
        "p95_ms":    float(p95_lat)  if p95_lat  else None,
        "errors":    int(errors)     if errors   else 0,
        "total_req": int(total)      if total    else None,
    }


def parse_filename(name: str) -> Optional[Dict]:
    """
    Decode filename convention from run_bench.sh:
      policy_<N>_(poltree|stateless)_t<T>
      entities_<N>_(poltree|stateless)_t<T>
      attrs_<N>_(poltree|stateless)_t<T>
    """
    for prefix, experiment in [
        ("policy",   "policy_scaling"),
        ("entities", "entity_scaling"),
        ("attrs",    "attr_scaling"),
    ]:
        m = re.match(rf"{prefix}_(\d+)_(poltree|stateless)_t(\d+)", name)
        if m:
            return {
                "experiment": experiment,
                "n":          int(m.group(1)),
                "backend":    m.group(2),
                "threads":    int(m.group(3)),
            }
    return None


# ─── Load ─────────────────────────────────────────────────────────────────────

def load_results(results_dir: Path) -> List[Dict]:
    rows = []
    for f in sorted(results_dir.glob("*.txt")):
        meta = parse_filename(f.stem)
        if meta is None:
            continue
        metrics = parse_sysbench_file(f)
        if metrics is None:
            print(f"  WARN: could not parse {f.name}")
            continue
        rows.append({**meta, **metrics, "file": f.name})
    return rows


# ─── CSV ──────────────────────────────────────────────────────────────────────

FIELDS = ["experiment", "n", "backend", "threads",
          "qps", "avg_ms", "min_ms", "max_ms", "p95_ms",
          "errors", "total_req", "file"]

def write_csv(rows: List[Dict], out: Path):
    with open(out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=FIELDS, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)
    print(f"  Wrote: {out}")


# ─── Plot helpers ─────────────────────────────────────────────────────────────

COLORS = {"poltree": "#2563EB", "stateless": "#DC2626"}
LABELS = {"poltree": "PolTree (cached)", "stateless": "Stateless Cedar"}
MARKERS = {"poltree": "o", "stateless": "s"}


def _ax_style(ax):
    ax.legend(fontsize=10)
    ax.grid(True, ls=":", lw=0.5, alpha=0.7)
    ax.spines[["top", "right"]].set_visible(False)


def plot_metric_vs_n(
    rows: List[Dict],
    experiment: str,
    metric: str,
    ylabel: str,
    title: str,
    out: Path,
    threads_filter: int = 32,
    log_x: bool = True,
):
    data = [r for r in rows
            if r["experiment"] == experiment and r["threads"] == threads_filter]
    if not data:
        print(f"  SKIP (no data): {out.name}")
        return

    fig, ax = plt.subplots(figsize=(9, 5))
    ax.set_title(f"{title}  (threads={threads_filter})", fontsize=12)
    ax.set_xlabel("N", fontsize=11)
    ax.set_ylabel(ylabel, fontsize=11)

    for backend in ["poltree", "stateless"]:
        pts = sorted(
            [(r["n"], r[metric]) for r in data
             if r["backend"] == backend and r[metric] is not None]
        )
        if not pts:
            continue
        xs, ys = zip(*pts)
        ax.plot(xs, ys, color=COLORS[backend], marker=MARKERS[backend],
                lw=2, ms=6, label=LABELS[backend])

    if log_x:
        ax.set_xscale("log")
        ax.xaxis.set_major_formatter(mticker.ScalarFormatter())

    _ax_style(ax)
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    print(f"  Saved: {out}")
    plt.close(fig)


def plot_concurrency(
    rows: List[Dict],
    experiment: str,
    n_filter: int,
    out: Path,
):
    data = [r for r in rows
            if r["experiment"] == experiment and r["n"] == n_filter]
    if not data:
        print(f"  SKIP (no data): {out.name}")
        return

    fig, axes = plt.subplots(1, 2, figsize=(14, 5))
    fig.suptitle(f"Concurrency scaling — {experiment.replace('_', ' ')} N={n_filter}", fontsize=12)

    for ax, (metric, ylabel) in zip(axes, [("qps", "QPS (req/s)"), ("p95_ms", "p95 latency (ms)")]):
        ax.set_xlabel("Threads", fontsize=11)
        ax.set_ylabel(ylabel, fontsize=11)

        for backend in ["poltree", "stateless"]:
            pts = sorted(
                [(r["threads"], r[metric]) for r in data
                 if r["backend"] == backend and r[metric] is not None]
            )
            if not pts:
                continue
            xs, ys = zip(*pts)
            ax.plot(xs, ys, color=COLORS[backend], marker=MARKERS[backend],
                    lw=2, ms=6, label=LABELS[backend])

        _ax_style(ax)

    fig.tight_layout()
    fig.savefig(out, dpi=150)
    print(f"  Saved: {out}")
    plt.close(fig)


# ─── Summary table (stdout) ───────────────────────────────────────────────────

def print_summary(rows: List[Dict]):
    print("\n── QPS summary (threads=32, largest N per experiment) ───────────────────────")
    print(f"{'Experiment':<20} {'Backend':<20} {'N':>8} {'QPS':>10} {'p95 ms':>10} {'errors':>8}")
    print("─" * 78)

    for exp in ["policy_scaling", "entity_scaling", "attr_scaling"]:
        exp_rows = [r for r in rows if r["experiment"] == exp and r["threads"] == 32]
        if not exp_rows:
            continue
        max_n = max(r["n"] for r in exp_rows)
        for backend in ["poltree", "stateless"]:
            r = next((r for r in exp_rows if r["n"] == max_n and r["backend"] == backend), None)
            if r:
                print(f"{exp:<20} {LABELS[backend]:<20} {r['n']:>8,} "
                      f"{r['qps']:>10,.0f} {(r['p95_ms'] or 0):>10.2f} {r['errors']:>8}")
        print()


# ─── CLI ──────────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--results-dir", default="benchmark/results")
    ap.add_argument("--out",         default="benchmark/results/plots")
    args = ap.parse_args()

    results_dir = Path(args.results_dir)
    out_dir     = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"\nLoading results from {results_dir}/ …")
    rows = load_results(results_dir)
    if not rows:
        print("No results found. Run benchmark/run_bench.sh first.")
        return
    print(f"  Found {len(rows)} result files\n")

    write_csv(rows, results_dir / "summary.csv")

    print("\nGenerating plots…")

    # ── Policy scaling ────────────────────────────────────────────────────────
    plot_metric_vs_n(rows, "policy_scaling", "qps",
        "QPS (req/s)", "Policy scaling — QPS vs N policies",
        out_dir / "policy_qps.png", threads_filter=32)

    plot_metric_vs_n(rows, "policy_scaling", "p95_ms",
        "p95 latency (ms)", "Policy scaling — p95 vs N policies",
        out_dir / "policy_p95.png", threads_filter=32)

    # ── Entity scaling ────────────────────────────────────────────────────────
    plot_metric_vs_n(rows, "entity_scaling", "qps",
        "QPS (req/s)", "Entity scaling — QPS vs total entities",
        out_dir / "entity_qps.png", threads_filter=32)

    plot_metric_vs_n(rows, "entity_scaling", "p95_ms",
        "p95 latency (ms)", "Entity scaling — p95 vs total entities",
        out_dir / "entity_p95.png", threads_filter=32)

    # ── Attribute scaling ─────────────────────────────────────────────────────
    plot_metric_vs_n(rows, "attr_scaling", "qps",
        "QPS (req/s)", "Attribute scaling — QPS vs attrs per entity",
        out_dir / "attr_qps.png", threads_filter=32, log_x=False)

    plot_metric_vs_n(rows, "attr_scaling", "p95_ms",
        "p95 latency (ms)", "Attribute scaling — p95 vs attrs per entity",
        out_dir / "attr_p95.png", threads_filter=32, log_x=False)

    # ── Concurrency plots ─────────────────────────────────────────────────────
    for n in [100, 1000]:
        plot_concurrency(rows, "policy_scaling", n,
            out_dir / f"concurrency_policy_{n}.png")

    for n in [10000, 100000]:
        plot_concurrency(rows, "entity_scaling", n,
            out_dir / f"concurrency_entities_{n}.png")

    print_summary(rows)
    print(f"\nAll done. Plots → {out_dir}/")


if __name__ == "__main__":
    main()