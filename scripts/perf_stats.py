#!/usr/bin/env python3
"""Summary statistics for mesh-ui performance logs.

The mesh-ui CSV logger writes one row per performance sample, with a
timestamp, the originating ``node_id`` and a set of metric columns (CPU
cycle counts and their nanosecond equivalents). This script reads one or
more of those logs and reports mean / std / min / max (plus count and
median) for every metric column.

The set of metric columns is read from each file's header, so the script
works regardless of which firmware revision produced the log (e.g. the
older ``broadcast_clone_dist_cycles`` schema or the newer per-stage
schema) and automatically picks up any columns added later.

The node count a run was performed with is read from the log filename:
mesh-ui writes ``perf-log-<N>nodes-<timestamp>.csv`` when the ``MESH_NODES``
environment variable is set at record time. Pass ``--nodes`` to override
or supply it for older/untagged logs (one value for every file, or one per
file in order). With ``--combine`` files sharing a node count are pooled
together, which is handy when you have several runs per topology size.

Examples
--------
    # Single run captured with 5 nodes
    python scripts/perf_stats.py logs/perf-log-*.csv --nodes 5

    # Compare two runs of different sizes
    python scripts/perf_stats.py run5.csv run10.csv --nodes 5 10

    # Pool all 5-node runs and all 10-node runs, ns columns only
    python scripts/perf_stats.py run5a.csv run5b.csv run10.csv \
        --nodes 5 5 10 --combine --unit ns

    # Break the stats down per originating node and save to CSV
    python scripts/perf_stats.py logs/perf-log-123.csv --nodes 8 \
        --by-node --out summary.csv
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import pandas as pd

# Columns that identify a sample rather than measure something.
NON_METRIC_COLUMNS = {"unix_ms", "node_id"}

# mesh-ui tags logs as `perf-log-<N>nodes-<timestamp>.csv` when MESH_NODES is
# set; this pulls the node count straight back out of the filename.
NODES_IN_NAME = re.compile(r"(\d+)nodes")

# The statistics reported for each metric, in output order.
AGGREGATIONS = ["count", "mean", "std", "min", "median", "max"]


def parse_node_list(raw: str) -> list[int]:
    """Parse the --nodes value, a single int or comma-separated list of ints."""
    try:
        return [int(part) for part in raw.split(",") if part.strip()]
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"expected an int or comma-separated ints (e.g. 5 or 5,5,10), got {raw!r}"
        ) from None


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compute summary statistics over mesh-ui performance logs.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "logs",
        nargs="+",
        type=Path,
        help="One or more perf-log CSV files.",
    )
    parser.add_argument(
        "--nodes",
        type=parse_node_list,
        metavar="N[,N...]",
        help=(
            "Override the node count for each run. By default it is read from "
            "the filename (mesh-ui writes perf-log-<N>nodes-... when MESH_NODES "
            "is set). Provide a single value (e.g. --nodes 5) to apply to every "
            "file, or a comma-separated list (e.g. --nodes 5,5,10) with one "
            "value per file in the same order as the log arguments."
        ),
    )
    parser.add_argument(
        "--combine",
        action="store_true",
        help="Pool rows from files that share a node count into one group.",
    )
    parser.add_argument(
        "--by-node",
        action="store_true",
        help="Additionally break statistics down per originating node_id.",
    )
    parser.add_argument(
        "--unit",
        choices=["cycles", "ns", "both"],
        default="both",
        help="Restrict metrics to cycle counts, nanoseconds, or report both.",
    )
    parser.add_argument(
        "--out",
        type=Path,
        metavar="FILE",
        help="Write the summary table to this CSV file instead of (only) printing.",
    )
    return parser.parse_args(argv)


def node_count_from_name(path: Path) -> int | None:
    """Recover the node count mesh-ui baked into the log filename, if present."""
    match = NODES_IN_NAME.search(path.name)
    return int(match.group(1)) if match else None


def resolve_node_counts(logs: list[Path], nodes: list[int] | None) -> list[int | None]:
    """Map each log file to its node count.

    By default the count is read from the filename (mesh-ui writes it there
    when MESH_NODES is set). ``--nodes`` overrides that for files where it is
    missing or wrong: pass one value for every file, or one value per file.
    """
    if nodes is None:
        return [node_count_from_name(path) for path in logs]
    if len(nodes) == 1:
        return nodes * len(logs)
    if len(nodes) == len(logs):
        return nodes
    sys.exit(
        f"--nodes expects 1 value or exactly {len(logs)} (one per log), "
        f"got {len(nodes)}."
    )


def select_metric_columns(columns: list[str], unit: str) -> list[str]:
    """Pick the metric columns from a header, filtered by unit."""
    metrics = [c for c in columns if c not in NON_METRIC_COLUMNS]
    if unit == "cycles":
        metrics = [c for c in metrics if c.endswith("_cycles")]
    elif unit == "ns":
        metrics = [c for c in metrics if c.endswith("_ns")]
    return metrics


def load_log(path: Path, node_count: int | None, unit: str) -> pd.DataFrame:
    """Read one log into a long-form frame: one row per (sample, metric)."""
    if not path.exists():
        sys.exit(f"Log file not found: {path}")
    df = pd.read_csv(path)

    metrics = select_metric_columns(list(df.columns), unit)
    if not metrics:
        sys.exit(f"No metric columns matching unit '{unit}' in {path}")

    id_vars = [c for c in ("node_id",) if c in df.columns]
    long = df.melt(
        id_vars=id_vars,
        value_vars=metrics,
        var_name="metric",
        value_name="value",
    )
    long["nodes"] = node_count
    long["file"] = path.name
    return long


def summarize(data: pd.DataFrame, by_node: bool) -> pd.DataFrame:
    """Aggregate the long-form samples into a summary table."""
    group_cols = ["nodes", "file"]
    if by_node and "node_id" in data.columns:
        group_cols.append("node_id")
    group_cols.append("metric")

    # nodes may be None (NaN); keep those groups instead of dropping them.
    summary = (
        data.groupby(group_cols, dropna=False)["value"].agg(AGGREGATIONS).reset_index()
    )

    # Stable, human-friendly ordering: by topology size, then file, then metric.
    sort_cols = [c for c in group_cols if c != "metric"] + ["metric"]
    return summary.sort_values(sort_cols).reset_index(drop=True)


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    node_counts = resolve_node_counts(args.logs, args.nodes)

    frames = [
        load_log(path, count, args.unit) for path, count in zip(args.logs, node_counts)
    ]
    data = pd.concat(frames, ignore_index=True)

    if args.combine:
        # Pool rows by node count; the file column is no longer meaningful.
        data["file"] = data["nodes"].map(
            lambda n: "all" if n is None else f"{n}-node runs"
        )

    summary = summarize(data, args.by_node)

    pd.set_option("display.max_rows", None)
    pd.set_option("display.width", None)
    pd.set_option("display.float_format", lambda v: f"{v:,.3f}")
    print(summary.to_string(index=False))

    if args.out:
        summary.to_csv(args.out, index=False)
        print(f"\nWrote summary to {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
