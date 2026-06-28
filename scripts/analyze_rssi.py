#!/usr/bin/env python3
"""Analyze RSSI calibration logs produced by mesh-ui.

Reads the combined CSV logs (one per run) written by `RssiCsvLogger`:

    unix_ms,kind,node_id,src,label,rssi,marker_index

and runs Part 1 of the tuning plan (calibration of the RSSI->distance model):

  - per-distance noise stats (mean / std / min / max) -> table + CSV
  - joint fit of A (rssi @ 1m) and N (path-loss exponent) via the linear
    model  rssi = A - 10*N*log10(d), fitted in RSSI-vs-log10(d) space
  - plots: calibration scatter + fit, per-distance RSSI boxplot, noise vs dist

The ground-truth distance is parsed from the `label` column (RSSI_LOG_LABEL),
e.g. `3m` -> 3.0. Anything before the distance token is treated as an
environment group, so labels like `inside-3m` / `outside-3m` are split into
`inside` / `outside` and fitted separately (N differs per environment).

Usage:
    python scripts/analyze_rssi.py                  # logs/ -> logs/analysis/
    python scripts/analyze_rssi.py --logs-dir logs --out-dir logs/analysis
    python scripts/analyze_rssi.py --by-src         # also split by TX node
"""

from __future__ import annotations

import argparse
import glob
import os
import re
import sys

import matplotlib

matplotlib.use("Agg")  # headless: write PNGs, never open a window
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

# Current firmware constants (esp32-firmware/build.rs: 10^((-56 - rssi)/25)).
# dist = 10^((A - rssi) / (10*N))  ->  A = -56 dBm, 10*N = 25  ->  N = 2.5.
FIRMWARE_A = -56.0
FIRMWARE_N = 2.5

LABEL_DIST_RE = re.compile(r"(\d+(?:\.\d+)?)\s*m\b", re.IGNORECASE)


def parse_label(label: str) -> tuple[str, float | None]:
    """Split a label into (environment, distance_m). Distance is the first
    `<number>m` token; the cleaned remainder is the environment group."""
    label = str(label)
    m = LABEL_DIST_RE.search(label)
    if not m:
        return label or "default", None
    dist = float(m.group(1))
    env = (label[: m.start()] + label[m.end() :]).strip(" -_/")
    return env or "default", dist


def load_logs(logs_dir: str, pattern: str) -> pd.DataFrame:
    paths = sorted(glob.glob(os.path.join(logs_dir, pattern)))
    if not paths:
        sys.exit(f"No log files matching {pattern!r} in {logs_dir!r}")

    frames = []
    for path in paths:
        df = pd.read_csv(path)
        df = df[df["kind"] == "sample"].copy()
        if df.empty:
            print(f"  skip (no samples): {os.path.basename(path)}")
            continue
        env_dist = df["label"].map(parse_label)
        df["env"] = env_dist.map(lambda t: t[0])
        df["dist"] = env_dist.map(lambda t: t[1])
        df["src_file"] = os.path.basename(path)
        n_unparsed = df["dist"].isna().sum()
        if n_unparsed:
            print(
                f"  skip {n_unparsed} rows with no distance in label "
                f"({df['label'].iloc[0]!r}) from {os.path.basename(path)}"
            )
        frames.append(df.dropna(subset=["dist"]))

    if not frames:
        sys.exit("No rows with a parseable distance label were found.")
    out = pd.concat(frames, ignore_index=True)
    out["rssi"] = pd.to_numeric(out["rssi"], errors="coerce")
    return out.dropna(subset=["rssi"])


def stats_table(df: pd.DataFrame, group_cols: list[str]) -> pd.DataFrame:
    g = df.groupby(group_cols)["rssi"]
    table = g.agg(
        n="count", mean="mean", std="std", min="min", max="max", median="median"
    ).reset_index()
    return table.sort_values(group_cols).round(2)


def fit_path_loss(dist: np.ndarray, rssi: np.ndarray) -> dict:
    """Fit rssi = A - 10*N*log10(d). Returns A, N, R^2 (on raw samples)."""
    x = np.log10(dist)
    slope, intercept = np.polyfit(x, rssi, 1)
    pred = slope * x + intercept
    ss_res = np.sum((rssi - pred) ** 2)
    ss_tot = np.sum((rssi - rssi.mean()) ** 2)
    r2 = 1.0 - ss_res / ss_tot if ss_tot > 0 else float("nan")
    return {"A": intercept, "N": -slope / 10.0, "r2": r2}


def n_accuracy_sweep(env: str, sub: pd.DataFrame, fit: dict, out_dir: str,
                     n_grid: np.ndarray | None = None) -> dict:
    """Sweep N and, for each, pick the A that minimizes DISTANCE error, then
    plot distance-RMSE vs N. Answers "which N gives the most accurate distance"
    directly in metres (the regression N minimizes RSSI error, which can differ).

    For a fixed N the estimate is d_est = C * 10^(-mean_rssi/(10N)) where
    C = 10^(A/(10N)); the optimal scale C (hence A) has a closed form per N.
    """
    if n_grid is None:
        n_grid = np.linspace(1.5, 5.0, 71)
    means = sub.groupby("dist")["rssi"].mean()
    d = means.index.to_numpy(float)
    r = means.to_numpy(float)

    rmse = np.empty_like(n_grid)
    for i, N in enumerate(n_grid):
        g = np.power(10.0, -r / (10.0 * N))
        C = np.sum(g * d) / np.sum(g * g)          # optimal scale -> optimal A
        rmse[i] = np.sqrt(np.mean((C * g - d) ** 2))
    bi = int(np.argmin(rmse))
    best_N = float(n_grid[bi])
    g = np.power(10.0, -r / (10.0 * best_N))
    best_A = 10.0 * best_N * np.log10(np.sum(g * d) / np.sum(g * g))

    safe = re.sub(r"[^0-9a-zA-Z._-]+", "_", env)
    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(n_grid, rmse, "-")
    ax.axvline(best_N, color="red", ls="-",
               label=f"distance-optimal N={best_N:.2f} (RMSE={rmse[bi]:.2f}m)")
    ax.axvline(fit["N"], color="blue", ls="--", label=f"regression N={fit['N']:.2f}")
    ax.axvline(FIRMWARE_N, color="green", ls=":", label=f"firmware N={FIRMWARE_N}")
    ax.set_xlabel("path-loss exponent N")
    ax.set_ylabel("distance RMSE over per-dist means (m)")
    ax.set_title(f"Distance accuracy vs N — env={env}")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, f"n-sweep-{safe}.png"), dpi=120)
    plt.close(fig)
    return {"N": best_N, "A": best_A, "rmse": float(rmse[bi])}


def plot_env(env: str, sub: pd.DataFrame, fit: dict, out_dir: str) -> None:
    safe = re.sub(r"[^0-9a-zA-Z._-]+", "_", env)

    # 1. Calibration scatter (log-x) + per-distance means + fitted curve.
    means = sub.groupby("dist")["rssi"].mean()
    fig, ax = plt.subplots(figsize=(8, 5))
    ax.scatter(sub["dist"], sub["rssi"], s=6, alpha=0.15, label="raw samples")
    ax.scatter(means.index, means.values, color="black", zorder=5, label="per-dist mean")
    dgrid = np.linspace(sub["dist"].min(), sub["dist"].max(), 200)
    ax.plot(
        dgrid,
        fit["A"] - 10 * fit["N"] * np.log10(dgrid),
        "r-",
        label=f"fit: A={fit['A']:.1f}, N={fit['N']:.2f}",
    )
    ax.plot(
        dgrid,
        FIRMWARE_A - 10 * FIRMWARE_N * np.log10(dgrid),
        "g--",
        alpha=0.7,
        label=f"firmware: A={FIRMWARE_A:.0f}, N={FIRMWARE_N}",
    )
    ax.set_xscale("log")
    ax.set_xlabel("distance (m, log scale)")
    ax.set_ylabel("RSSI (dBm)")
    ax.set_title(f"RSSI vs distance — env={env}  (R²={fit['r2']:.3f})")
    ax.legend()
    ax.grid(True, which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, f"calibration-{safe}.png"), dpi=120)
    plt.close(fig)

    # 2. RSSI distribution per distance (spread = noise/spike size).
    dists = sorted(sub["dist"].unique())
    fig, ax = plt.subplots(figsize=(8, 5))
    # Set tick labels separately (boxplot's label kwarg was renamed across
    # matplotlib versions) so this works regardless of installed version.
    ax.boxplot([sub[sub["dist"] == d]["rssi"].values for d in dists], positions=range(len(dists)))
    ax.set_xticks(range(len(dists)))
    ax.set_xticklabels([str(d) for d in dists])
    ax.set_xlabel("distance (m)")
    ax.set_ylabel("RSSI (dBm)")
    ax.set_title(f"RSSI distribution per distance — env={env}")
    ax.grid(True, axis="y", alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, f"distribution-{safe}.png"), dpi=120)
    plt.close(fig)

    # 3. Noise (std) vs distance — informs the spike-threshold floor (~3 sigma).
    std = sub.groupby("dist")["rssi"].std()
    fig, ax = plt.subplots(figsize=(8, 5))
    ax.bar([str(d) for d in std.index], std.values)
    ax.set_xlabel("distance (m)")
    ax.set_ylabel("RSSI std (dB)")
    ax.set_title(f"Per-distance noise — env={env}")
    ax.grid(True, axis="y", alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, f"noise-{safe}.png"), dpi=120)
    plt.close(fig)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--logs-dir", default="logs")
    p.add_argument("--out-dir", default="logs/analysis")
    p.add_argument("--pattern", default="rssi-*.csv")
    p.add_argument("--by-src", action="store_true", help="also break the stats table down by TX node (src MAC)")
    args = p.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    df = load_logs(args.logs_dir, args.pattern)
    print(f"\nLoaded {len(df)} samples across {df['src_file'].nunique()} file(s), "
          f"envs={sorted(df['env'].unique())}, dists={sorted(df['dist'].unique())}\n")

    # Per-distance stats table (optionally per src link, to expose asymmetry).
    group_cols = ["env", "dist"] + (["src"] if args.by_src else [])
    table = stats_table(df, group_cols)
    table_path = os.path.join(args.out_dir, "rssi-stats.csv")
    table.to_csv(table_path, index=False)
    print(table.to_string(index=False))
    print(f"\n-> stats table: {table_path}")

    if df["src"].nunique() > 1 and not args.by_src:
        print(f"\nNote: {df['src'].nunique()} distinct src MACs present — RSSI is "
              "per-link asymmetric. Re-run with --by-src to separate them.")

    # Per-environment A/N fit + plots. Two N estimates per env:
    #  - regression N (least-squares in RSSI space)
    #  - distance-optimal N (minimises distance RMSE; the N sweep plot)
    print("\nPath-loss fit  (rssi = A - 10*N*log10(d)):")
    print(f"{'env':<12}{'A (rssi@1m)':>13}{'N (fit)':>9}{'R^2':>7}"
          f"{'N (best dist)':>15}{'A (best)':>10}{'dRMSE m':>9}{'n':>8}")
    for env, sub in df.groupby("env"):
        if sub["dist"].nunique() < 2:
            print(f"{env:<12}{'(need >=2 distances to fit)':>40}")
            continue
        fit = fit_path_loss(sub["dist"].to_numpy(), sub["rssi"].to_numpy())
        plot_env(env, sub, fit, args.out_dir)
        best = n_accuracy_sweep(env, sub, fit, args.out_dir)
        print(f"{env:<12}{fit['A']:>13.2f}{fit['N']:>9.2f}{fit['r2']:>7.3f}"
              f"{best['N']:>15.2f}{best['A']:>10.2f}{best['rmse']:>9.2f}{len(sub):>8}")

    print(f"\nFirmware currently uses A={FIRMWARE_A:.0f}, N={FIRMWARE_N} "
          "(esp32-firmware/build.rs). Compare against the fitted values above.")
    print(f"Plots written to {args.out_dir}/")


if __name__ == "__main__":
    main()
