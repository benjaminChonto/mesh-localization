#!/usr/bin/env python3
"""Tune the smoothing / spike filter (Part 2 of the RSSI tuning plan).

Faithfully replays the firmware filter from `esp32-firmware/src/state.rs`:

    raw ──> spike-clamp (ref = window mean) ──> EMA(alpha) ──> distance

and sweeps (alpha, window, threshold) over:

  2a static jitter  -- from the labelled static logs (precision at rest)
  2b spike threshold -- lower bound from static noise (~3 sigma); upper bound
                        from the largest legit motion delta (needs a staircase)
  2c responsiveness  -- a SYNTHETIC staircase trajectory built from the
                        calibration model + bootstrapped static residuals
                        (captures both noise and real spikes), scored by
                        tracking RMSE and settling time
  2d real staircase  -- optional: overlay raw/filtered on a real run and pull
                        the motion-delta upper bound from its marker segments

Reuses calibration loading/fit from analyze_rssi.py (same directory).

Usage:
    python scripts/tune_filter.py                       # logs/ -> logs/tuning/
    python scripts/tune_filter.py --env inside
    python scripts/tune_filter.py --A -56 --N 2.5       # skip the fit
    python scripts/tune_filter.py --staircase logs/rssi-walk-….csv
    python scripts/tune_filter.py --push-raw            # compare the old behavior
"""

from __future__ import annotations

import argparse
import os
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from analyze_rssi import fit_path_loss, load_logs, parse_label  # noqa: E402

# Firmware defaults (state.rs).
DEF_ALPHA = 0.3
DEF_WINDOW = 8
DEF_THRESHOLD = 12.0
DT = 0.05  # seconds between samples (~20/s)


class RssiFilter:
    """Exact replica of State::update in esp32-firmware/src/state.rs.

    The firmware now pushes the de-spiked value into the window
    (`push_filtered=True`, the default). `push_filtered=False` reproduces the
    old behaviour (push raw) so the two can be compared.
    """

    def __init__(self, alpha, window_size, threshold, push_filtered=True):
        self.alpha = alpha
        self.window_size = max(1, int(window_size))
        self.threshold = threshold
        self.push_filtered = push_filtered
        self.buf: list[float] = []
        self.sum = 0.0
        self.ema: float | None = None

    def update(self, rssi: float) -> float:
        raw = float(rssi)
        if self.ema is None:  # State::new: seed window+ema with first sample
            self.buf = [raw]
            self.sum = raw
            self.ema = raw
            return self.ema

        win_mean = self.sum / len(self.buf)
        if abs(raw - win_mean) > self.threshold:  # spike: clamp toward mean
            sign = 1.0 if raw > win_mean else -1.0
            filtered = win_mean + sign * self.threshold
        else:
            filtered = raw

        push = filtered if self.push_filtered else raw  # state.rs pushes filtered
        if len(self.buf) < self.window_size:
            self.buf.append(push)
            self.sum += push
        else:
            self.sum += push - self.buf.pop(0)
            self.buf.append(push)

        self.ema = self.alpha * filtered + (1.0 - self.alpha) * self.ema
        return self.ema

    def run(self, rssi_seq: np.ndarray) -> np.ndarray:
        return np.array([self.update(r) for r in rssi_seq])


def rssi_to_dist(rssi, A, N):
    return np.power(10.0, (A - np.asarray(rssi, float)) / (10.0 * N))


# --------------------------------------------------------------------------
# 2a — static jitter
# --------------------------------------------------------------------------
def static_jitter(df, A, N, alphas, warmup, out_dir):
    """std (in metres) of the filtered estimate at rest, vs alpha. Spike clamp
    disabled (huge threshold) so this is pure smoothing. Window is also swept to
    show it barely moves static jitter (it is a spike reference, not a smoother).
    """
    groups = [
        sub.sort_values("unix_ms")["rssi"].to_numpy()
        for _, sub in df.groupby("dist")
        if len(sub) > warmup + 5
    ]
    rows = []
    for a in alphas:
        for w in (1, DEF_WINDOW, 32):
            stds = []
            for seq in groups:
                ema = RssiFilter(a, w, 1e9).run(seq)[warmup:]
                stds.append(np.std(rssi_to_dist(ema, A, N)))
            rows.append({"alpha": a, "window": w, "n_eff": (2 - a) / a,
                         "jitter_m": float(np.mean(stds))})
    tab = pd.DataFrame(rows)

    fig, ax = plt.subplots(figsize=(8, 5))
    for w in (1, DEF_WINDOW, 32):
        s = tab[tab.window == w]
        ax.plot(s.alpha, s.jitter_m, "o-", label=f"window={w}")
    ax.set_xlabel("EMA alpha (1 = no smoothing)")
    ax.set_ylabel("estimate jitter at rest (m, std)")
    ax.set_title("Static jitter vs smoothing — diminishing returns")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "static-jitter.png"), dpi=120)
    plt.close(fig)
    return tab


# --------------------------------------------------------------------------
# synthetic staircase trajectory from the calibration model
# --------------------------------------------------------------------------
def build_residual_pools(df, A, N):
    """Per-distance residual arrays (raw - model). Bootstrapping these injects
    BOTH the static noise and the real spikes that occurred at that distance."""
    pools = {}
    for d, sub in df.groupby("dist"):
        model = A - 10 * N * np.log10(d)
        pools[d] = (sub["rssi"].to_numpy() - model)
    return pools


def synth_trajectory(df, A, N, hold_s, seed):
    rng = np.random.default_rng(seed)
    dists = sorted(df["dist"].unique())
    pools = build_residual_pools(df, A, N)
    ladder = dists + dists[-2::-1]  # up then back down to test both directions
    hold_n = max(1, int(hold_s / DT))

    true_d, obs_rssi = [], []
    pool_dists = np.array(dists)
    for d in ladder:
        near = pool_dists[np.argmin(np.abs(pool_dists - d))]
        res = pools[near]
        model = A - 10 * N * np.log10(d)
        for _ in range(hold_n):
            true_d.append(d)
            obs_rssi.append(model + rng.choice(res))
    return np.array(true_d), np.array(obs_rssi)


def settling_times(true_d, est_d, tol_frac=0.15):
    """Samples to settle within tol_frac of each new plateau after a step."""
    steps = np.flatnonzero(np.diff(true_d) != 0) + 1
    out = []
    for k, s in enumerate(steps):
        end = steps[k + 1] if k + 1 < len(steps) else len(true_d)
        target = true_d[s]
        band = max(0.5, tol_frac * target)
        seg = est_d[s:end]
        within = np.abs(seg - target) <= band
        # first index from which it stays within band for the rest of the plateau
        settle = len(seg)
        for i in range(len(seg)):
            if within[i:].all():
                settle = i
                break
        out.append(settle * DT)
    return float(np.median(out)) if out else float("nan")


# --------------------------------------------------------------------------
# 2c — responsiveness sweep (RMSE heatmap over alpha x window)
# --------------------------------------------------------------------------
def responsiveness(true_d, obs_rssi, A, N, alphas, windows, threshold,
                   push_filtered, warmup, out_dir):
    rmse = np.full((len(alphas), len(windows)), np.nan)
    settle = np.full_like(rmse, np.nan)
    for i, a in enumerate(alphas):
        for j, w in enumerate(windows):
            ema = RssiFilter(a, w, threshold, push_filtered).run(obs_rssi)
            est = rssi_to_dist(ema, A, N)
            err = est[warmup:] - true_d[warmup:]
            rmse[i, j] = np.sqrt(np.mean(err ** 2))
            settle[i, j] = settling_times(true_d, est)

    bi, bj = np.unravel_index(np.nanargmin(rmse), rmse.shape)
    best = {"alpha": alphas[bi], "window": windows[bj],
            "rmse": rmse[bi, bj], "settle_s": settle[bi, bj]}

    fig, ax = plt.subplots(figsize=(8, 5))
    im = ax.imshow(rmse, origin="lower", aspect="auto", cmap="viridis")
    ax.set_xticks(range(len(windows)), windows)
    ax.set_yticks(range(len(alphas)), [f"{a:g}" for a in alphas])
    ax.set_xlabel("window size")
    ax.set_ylabel("EMA alpha")
    ax.set_title(f"Tracking RMSE (m) — threshold={threshold:g}\n"
                 f"best: alpha={best['alpha']:g}, window={best['window']}, "
                 f"RMSE={best['rmse']:.2f}m")
    ax.scatter([bj], [bi], marker="*", s=200, color="red")
    for i in range(len(alphas)):
        for j in range(len(windows)):
            ax.text(j, i, f"{rmse[i, j]:.1f}", ha="center", va="center",
                    color="w", fontsize=7)
    fig.colorbar(im, ax=ax, label="RMSE (m)")
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "responsiveness-rmse.png"), dpi=120)
    plt.close(fig)
    return best


def best_timeseries(true_d, obs_rssi, A, N, best, threshold, push_filtered, out_dir):
    ema = RssiFilter(best["alpha"], best["window"], threshold, push_filtered).run(obs_rssi)
    est = rssi_to_dist(ema, A, N)
    raw_d = rssi_to_dist(obs_rssi, A, N)
    t = np.arange(len(true_d)) * DT
    fig, ax = plt.subplots(figsize=(10, 5))
    ax.plot(t, raw_d, color="0.7", lw=0.6, label="raw (unfiltered)")
    ax.plot(t, true_d, "k--", lw=1.5, label="true distance")
    ax.plot(t, est, "r-", lw=1.2,
            label=f"filtered (alpha={best['alpha']:g}, win={best['window']})")
    ax.set_xlabel("time (s)")
    ax.set_ylabel("distance (m)")
    ax.set_ylim(0, np.nanmax(true_d) * 1.6)
    ax.set_title("Synthetic staircase — best filter tracking")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "best-tracking.png"), dpi=120)
    plt.close(fig)


# --------------------------------------------------------------------------
# 2b — threshold sweep
# --------------------------------------------------------------------------
def threshold_sweep(true_d, obs_rssi, A, N, best, thresholds, sigma3,
                    motion_delta, push_filtered, warmup, out_dir):
    rmses = []
    for th in thresholds:
        ema = RssiFilter(best["alpha"], best["window"], th, push_filtered).run(obs_rssi)
        est = rssi_to_dist(ema, A, N)
        err = est[warmup:] - true_d[warmup:]
        rmses.append(np.sqrt(np.mean(err ** 2)))
    fig, ax = plt.subplots(figsize=(8, 5))
    ax.plot(thresholds, rmses, "o-")
    ax.axvline(sigma3, color="green", ls="--", label=f"~3σ static floor = {sigma3:.1f} dB")
    if motion_delta is not None:
        ax.axvline(motion_delta, color="red", ls="--",
                   label=f"max motion delta = {motion_delta:.1f} dB")
    ax.axvline(DEF_THRESHOLD, color="0.5", ls=":", label=f"firmware = {DEF_THRESHOLD:g} dB")
    ax.set_xlabel("spike threshold (dB)")
    ax.set_ylabel("tracking RMSE (m)")
    ax.set_title("Spike threshold sweep (sweep between the dashed bounds)")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "threshold-sweep.png"), dpi=120)
    plt.close(fig)


# --------------------------------------------------------------------------
# 2d — real staircase run (optional)
# --------------------------------------------------------------------------
def real_staircase(path, A, N, best, threshold, push_filtered, out_dir):
    df = pd.read_csv(path)
    samples = df[df["kind"] == "sample"].sort_values("unix_ms")
    markers = df[df["kind"] == "marker"]
    t0 = samples["unix_ms"].iloc[0]
    t = (samples["unix_ms"].to_numpy() - t0) / 1000.0
    rssi = samples["rssi"].to_numpy(float)
    ema = RssiFilter(best["alpha"], best["window"], threshold, push_filtered).run(rssi)

    fig, (a1, a2) = plt.subplots(2, 1, figsize=(11, 7), sharex=True)
    a1.plot(t, rssi, color="0.7", lw=0.6, label="raw RSSI")
    a1.plot(t, ema, "r-", lw=1.2, label="filtered RSSI")
    a2.plot(t, rssi_to_dist(rssi, A, N), color="0.7", lw=0.6, label="raw dist")
    a2.plot(t, rssi_to_dist(ema, A, N), "r-", lw=1.2, label="filtered dist")
    for mt in markers["unix_ms"]:
        for ax in (a1, a2):
            ax.axvline((mt - t0) / 1000.0, color="b", ls="--", alpha=0.5)
    a1.set_ylabel("RSSI (dBm)"); a1.legend(); a1.grid(True, alpha=0.3)
    a2.set_ylabel("distance (m)"); a2.set_xlabel("time (s)")
    a2.legend(); a2.grid(True, alpha=0.3)
    a1.set_title(f"Real staircase ({os.path.basename(path)}) — blue = waypoint markers")
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "real-staircase.png"), dpi=120)
    plt.close(fig)

    # Largest consecutive delta -> motion-delta upper bound for the threshold.
    max_delta = float(np.max(np.abs(np.diff(rssi)))) if len(rssi) > 1 else None
    return max_delta


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--logs-dir", default="logs")
    p.add_argument("--out-dir", default="logs/tuning")
    p.add_argument("--pattern", default="rssi-*.csv")
    p.add_argument("--env", default=None, help="environment to tune (default: most samples)")
    p.add_argument("--A", type=float, default=None, help="rssi@1m (default: fit from logs)")
    p.add_argument("--N", type=float, default=None, help="path-loss exponent (default: fit)")
    p.add_argument("--hold", type=float, default=8.0, help="synthetic plateau hold (s)")
    p.add_argument("--warmup", type=int, default=20, help="samples discarded before scoring")
    p.add_argument("--threshold", type=float, default=DEF_THRESHOLD)
    p.add_argument("--staircase", default=None, help="path to a real staircase run csv")
    p.add_argument("--push-raw", action="store_true",
                   help="reproduce the OLD behavior (push raw into the window) "
                        "instead of the current state.rs (push de-spiked value)")
    p.add_argument("--seed", type=int, default=0)
    args = p.parse_args()
    # Firmware now pushes the de-spiked value; --push-raw flips back to old.
    args.push_filtered = not args.push_raw

    os.makedirs(args.out_dir, exist_ok=True)
    df = load_logs(args.logs_dir, args.pattern)

    if args.env is None:
        args.env = df["env"].value_counts().idxmax()
    df = df[df["env"] == args.env]
    if df["dist"].nunique() < 2:
        sys.exit(f"env {args.env!r} has <2 distances; need a calibration sweep.")
    print(f"Tuning env={args.env!r}: {len(df)} samples, "
          f"dists={sorted(df['dist'].unique())}, push_filtered={args.push_filtered}\n")

    if args.A is None or args.N is None:
        fit = fit_path_loss(df["dist"].to_numpy(), df["rssi"].to_numpy())
        A, N = (args.A or fit["A"]), (args.N or fit["N"])
        print(f"Using fitted A={A:.2f}, N={N:.2f} (R²={fit['r2']:.3f})\n")
    else:
        A, N = args.A, args.N
        print(f"Using supplied A={A:.2f}, N={N:.2f}\n")

    alphas = [0.05, 0.1, 0.15, 0.2, 0.3, 0.5, 0.7, 1.0]
    windows = [1, 2, 4, 8, 16, 32]

    # 2a static jitter
    jit = static_jitter(df, A, N, alphas, args.warmup, args.out_dir)
    print("Static jitter (window=8), metres std:")
    print(jit[jit.window == DEF_WINDOW][["alpha", "n_eff", "jitter_m"]]
          .round(3).to_string(index=False))

    # 2c synthetic responsiveness
    true_d, obs = synth_trajectory(df, A, N, args.hold, args.seed)
    best = responsiveness(true_d, obs, A, N, alphas, windows, args.threshold,
                          args.push_filtered, args.warmup, args.out_dir)
    print(f"\nBest by tracking RMSE: alpha={best['alpha']:g}, "
          f"window={best['window']}, RMSE={best['rmse']:.2f}m, "
          f"median settle={best['settle_s']:.2f}s")
    best_timeseries(true_d, obs, A, N, best, args.threshold, args.push_filtered, args.out_dir)

    # 2d optional real staircase -> motion-delta upper bound
    motion_delta = None
    if args.staircase:
        motion_delta = real_staircase(args.staircase, A, N, best, args.threshold,
                                      args.push_filtered, args.out_dir)
        print(f"\nReal staircase max consecutive delta = {motion_delta:.1f} dB "
              "(threshold upper bound).")

    # 2b threshold sweep between the two bounds
    sigma3 = 3.0 * float(df.groupby("dist")["rssi"].std().median())
    ths = sorted(set(np.round(np.linspace(2, 30, 15), 1)) | {sigma3, DEF_THRESHOLD})
    threshold_sweep(true_d, obs, A, N, best, ths, sigma3, motion_delta,
                    args.push_filtered, args.warmup, args.out_dir)
    print(f"\nThreshold floor ~3σ = {sigma3:.1f} dB. "
          + ("Set threshold between this and the motion delta above."
             if motion_delta else
             "Provide --staircase to get the motion-delta upper bound."))
    print(f"\nPlots + tables written to {args.out_dir}/")


if __name__ == "__main__":
    main()
