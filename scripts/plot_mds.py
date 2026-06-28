#!/usr/bin/env python3
"""Reproduce the MDS layouts logged by ``mesh-ui`` as a PNG.

``mesh-ui`` appends every MDS solution produced by node 0 to
``logs/mds-log-<timestamp>.csv`` in long format::

    unix_ms,frame,node_idx,x,y

This script turns one or more such logs into a scatter plot so you can compare
the algorithm's estimate against a real-world layout: arrange your physical
nodes in a known shape, run the mesh for a few seconds, then plot the log.

The firmware already Kabsch-aligns each MDS solution onto the previous one, so
consecutive frames share a stable orientation and averaging is a plain mean of
the raw coordinates -- no re-alignment needed. A ground-truth shape, however,
lives in an unrelated real-world frame, so the estimate is fitted onto it
before they are overlaid or compared.

Examples
--------
Plot the most recent frame of a single run::

    python scripts/plot_mds.py logs/mds-log-123.csv

Average every frame of a run (frames aligned to each other first)::

    python scripts/plot_mds.py logs/mds-log-123.csv --frame average

Average across several runs and compare to a real-world layout, reporting the
RMS positioning error after alignment::

    python scripts/plot_mds.py logs/*.csv --frame average \\
        --truth truth/square.csv --out square.png
"""

from __future__ import annotations

import argparse
import glob
import sys
from pathlib import Path

import numpy as np

import matplotlib

matplotlib.use("Agg")  # headless: write a PNG, never open a window
import matplotlib.pyplot as plt


def load_frames(paths: list[str]) -> list[np.ndarray]:
    """Load every MDS frame from the given CSV logs.

    Each log is ``unix_ms,frame,node_idx,x,y``. Returns a list of ``(N, 2)``
    arrays, one per frame, with rows ordered by ``node_idx``. Frames whose node
    set differs from the first frame's are skipped, since alignment and
    averaging need a consistent node ordering.
    """
    frames: list[np.ndarray] = []
    reference_nodes: np.ndarray | None = None

    for path in paths:
        data = np.loadtxt(path, delimiter=",", skiprows=1, ndmin=2)
        if data.size == 0:
            continue
        frame_col, node_col = data[:, 1], data[:, 2]
        for fid in np.unique(frame_col):
            rows = data[frame_col == fid]
            rows = rows[rows[:, 2].argsort()]  # order by node_idx
            nodes = rows[:, 2].astype(int)
            coords = rows[:, 3:5]

            if reference_nodes is None:
                reference_nodes = nodes
            elif not np.array_equal(nodes, reference_nodes):
                continue

            frames.append(coords)

    return frames


def fit_onto(source: np.ndarray, target: np.ndarray, scale: bool = False) -> np.ndarray:
    """Best-fit ``source`` onto ``target`` to bring it into the target's frame.

    Rotation + reflection + translation by default; pass ``scale=True`` to also
    rescale. Used only to align the MDS estimate onto a ground-truth layout --
    frame-to-frame orientation is already handled by the firmware's Kabsch step.

    Reflection is allowed because the estimate's chirality is arbitrary relative
    to an externally-defined truth, so a mirrored-but-correct shape still counts
    as a match. Scaling is off by default to keep the comparison metric: the
    firmware uses Kabsch (no scaling), so the estimate is already in real units.
    """
    src_mean = source.mean(axis=0)
    tgt_mean = target.mean(axis=0)
    src_c = source - src_mean
    tgt_c = target - tgt_mean

    u, s, vt = np.linalg.svd(tgt_c.T @ src_c)
    rotation = u @ vt

    factor = 1.0
    if scale:
        src_norm = (src_c**2).sum()
        factor = s.sum() / src_norm if src_norm > 0 else 1.0

    return (factor * src_c @ rotation.T) + tgt_mean


def average_frames(frames: list[np.ndarray]) -> np.ndarray:
    """Mean node positions across all frames.

    The firmware's Kabsch step already keeps consecutive solutions in a stable
    orientation, so the frames are directly comparable -- this is a plain mean,
    no per-frame re-alignment.
    """
    return np.array(frames).mean(axis=0)


def load_truth(path: str) -> np.ndarray:
    """Load a ground-truth layout (``node_idx,x,y``) as an ``(N, 2)`` array."""
    data = np.loadtxt(path, delimiter=",", skiprows=1, ndmin=2)
    data = data[data[:, 0].argsort()]  # order by node_idx
    return data[:, 1:3]


def rms_error(estimate: np.ndarray, truth: np.ndarray, scale: bool) -> float:
    """RMS per-node distance after fitting ``estimate`` onto ``truth``."""
    fitted = fit_onto(estimate, truth, scale=scale)
    return float(np.sqrt(((fitted - truth) ** 2).sum(axis=1).mean()))


def annotate(ax: plt.Axes, coords: np.ndarray, color: str) -> None:
    """Label each plotted node with its index."""
    for idx, (x, y) in enumerate(coords):
        ax.annotate(str(idx), (x, y), textcoords="offset points", xytext=(5, 5), color=color)


def plot(
    coords: np.ndarray,
    truth: np.ndarray | None,
    title: str,
    out: Path,
    scale: bool = False,
) -> None:
    fig, ax = plt.subplots(figsize=(7, 7))

    if truth is not None:
        # Bring the estimate into the truth's frame so the two are comparable.
        coords = fit_onto(coords, truth, scale=scale)
        ax.scatter(truth[:, 0], truth[:, 1], c="tab:green", marker="s", s=80, label="ground truth")
        annotate(ax, truth, "tab:green")
        for est, gt in zip(coords, truth):
            ax.plot([est[0], gt[0]], [est[1], gt[1]], c="0.7", lw=1, zorder=0)

    ax.scatter(coords[:, 0], coords[:, 1], c="tab:blue", s=80, label="MDS estimate")
    annotate(ax, coords, "tab:blue")

    ax.set_title(title)
    ax.set_xlabel("x")
    ax.set_ylabel("y")
    ax.set_aspect("equal", adjustable="datalim")
    ax.grid(True, ls=":", alpha=0.5)
    ax.legend()
    fig.tight_layout()
    fig.savefig(out, dpi=150)
    print(f"Wrote {out}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Render MDS logs from mesh-ui as a PNG.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "logs",
        nargs="+",
        help="MDS CSV log file(s); globs are expanded. Multiple files are pooled.",
    )
    parser.add_argument(
        "--frame",
        default="last",
        help="'last', 'first', 'average', or a 0-based frame index (default: last). "
        "'average' aligns frames to each other before averaging.",
    )
    parser.add_argument(
        "--truth",
        help="Optional ground-truth layout CSV (node_idx,x,y) to overlay and "
        "measure error against.",
    )
    parser.add_argument(
        "--scale",
        action="store_true",
        help="Also rescale the estimate when fitting onto --truth. Off by "
        "default, since the firmware's Kabsch keeps the estimate in real units; "
        "enable this if your MDS output is in arbitrary units.",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("mds-plot.png"),
        help="Output PNG path (default: mds-plot.png).",
    )
    args = parser.parse_args()

    paths: list[str] = []
    for pattern in args.logs:
        matched = sorted(glob.glob(pattern))
        paths.extend(matched if matched else [pattern])

    frames = load_frames(paths)
    if not frames:
        print("No MDS frames found in the given log(s).", file=sys.stderr)
        return 1

    if args.frame == "average":
        coords = average_frames(frames)
        title = f"MDS average of {len(frames)} frame(s)"
    elif args.frame == "first":
        coords, title = frames[0], "MDS frame 0"
    elif args.frame == "last":
        coords, title = frames[-1], f"MDS frame {len(frames) - 1} (last)"
    else:
        try:
            i = int(args.frame)
        except ValueError:
            print(f"--frame must be last/first/average or an integer, got {args.frame!r}", file=sys.stderr)
            return 2
        if not -len(frames) <= i < len(frames):
            print(f"frame index {i} out of range (have {len(frames)} frames)", file=sys.stderr)
            return 2
        coords, title = frames[i], f"MDS frame {i % len(frames)}"

    truth = load_truth(args.truth) if args.truth else None
    if truth is not None:
        if truth.shape[0] != coords.shape[0]:
            print(
                f"truth has {truth.shape[0]} nodes but the log has {coords.shape[0]}; "
                "they must match.",
                file=sys.stderr,
            )
            return 3
        err = rms_error(coords, truth, args.scale)
        title += f"\nRMS error vs truth: {err:.3f}"
        print(f"RMS positioning error after alignment: {err:.4f}")

    plot(coords, truth, title, args.out, args.scale)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
