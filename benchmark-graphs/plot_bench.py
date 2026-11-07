#!/usr/bin/env python3
"""Plot stale-head full-stack vs baseline graphs from BENCHMARK_RESULTS.md tables."""

from __future__ import annotations

import re
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.ticker import FuncFormatter

_HERE = Path(__file__).resolve().parent
SRC = _HERE.parent / "BENCHMARK_RESULTS.md"
OUT = _HERE
FOOT = (
    "Source: b0a7/lighthouse @ cursor/stale-head-benchmark-stack-0c9d | "
    "32768 validators, 4 vCPU, fix tip 6663e7c5b vs base 25b9b7541"
)

plt.rcParams.update(
    {
        "figure.facecolor": "white",
        "axes.facecolor": "white",
        "savefig.facecolor": "white",
        "font.size": 10,
        "axes.grid": True,
        "grid.alpha": 0.3,
    }
)

# (variant, fork, endpoint, N) -> metrics
jump: dict[tuple[str, str, str, int], dict] = {}
incr: dict[tuple[str, str, str, int], dict] = {}

variant = None
mode = None
for line in SRC.read_text().splitlines():
    if line.startswith("### Fix jump"):
        variant, mode = "fix", "jump"
        continue
    if line.startswith("### Baseline jump"):
        variant, mode = "base", "jump"
        continue
    if line.startswith("### Fix incremental"):
        variant, mode = "fix", "incr"
        continue
    if line.startswith("### Baseline incremental"):
        variant, mode = "base", "incr"
        continue
    if line.startswith("## ") or line.startswith("# "):
        # leaving a section; keep variant until next ### header
        pass
    if not variant or not line.startswith("|"):
        continue
    if "fork" in line or line.startswith("|---"):
        continue
    cols = [c.strip() for c in line.strip().strip("|").split("|")]
    if mode == "jump" and len(cols) >= 11:
        fork, ep, n = cols[0], cols[1], int(cols[2])
        jump[(variant, fork, ep, n)] = {
            "first": float(cols[3]),
            "first_pe": int(cols[4]),
            "repeat": float(cols[5]),
            "wall": float(cols[8]),
            "cpu": float(cols[9]),
        }
    elif mode == "incr" and len(cols) >= 5:
        fork, ep, n = cols[0], cols[1], int(cols[2])
        incr[(variant, fork, ep, n)] = {
            "first": float(cols[3]),
            "first_pe": int(cols[4]),
        }

assert jump and incr, (len(jump), len(incr))
print(f"parsed jump={len(jump)} incr={len(incr)}")

NS = [1, 2, 4, 8, 16, 32]
SHORT = {
    "proposer_duties(next_epoch)": "proposer",
    "attestation_data(current_slot)": "attestation_data",
    "attester_duties(current_epoch)": "attester",
    "ptc_duties(current_epoch)": "ptc",
    "block_production(v3)": "block_prod",
    "block_production(v4)": "block_prod",
}
COL = {"base": "#d62728", "fix": "#1f77b4"}
fmt = FuncFormatter(lambda x, _: f"{x:g}" if x < 1 else f"{x:,.0f}")


def foot(fig):
    fig.text(0.5, 0.01, FOOT, ha="center", va="bottom", fontsize=7.5, color="#555")


files: list[str] = []


def save(fig, name: str):
    path = OUT / name
    fig.savefig(path, dpi=150)
    plt.close(fig)
    files.append(str(path))
    print("wrote", path.name)


# --- 1. Gloas N=32 headline bars ---
N = 32
fork = "gloas"
groups = [
    ("proposer_duties(next_epoch)", "repeat", "proposer\nrepeat"),
    ("proposer_duties(next_epoch)", "wall", "proposer\nconcurrent"),
    ("attestation_data(current_slot)", "repeat", "attestation_data\nrepeat"),
    ("attestation_data(current_slot)", "wall", "attestation_data\nconcurrent"),
    ("attester_duties(current_epoch)", "first_pe", "attester\nfirst_pe"),
]
fig, ax = plt.subplots(figsize=(9, 4.8))
x = np.arange(len(groups))
w = 0.36
for k, variant in enumerate(["base", "fix"]):
    vals = []
    for ep, met, _ in groups:
        row = jump[(variant, fork, ep, N)]
        vals.append(row["first_pe"] if met == "first_pe" else row[met])
    bars = ax.bar(x + (k - 0.5) * w, vals, w, label=variant, color=COL[variant])
    for b, v, (_, met, _) in zip(bars, vals, groups):
        label = f"{v:g}" if met == "first_pe" else (f"{v:,.0f}" if v >= 10 else f"{v:.2f}")
        ax.text(
            b.get_x() + b.get_width() / 2,
            max(v, 0.15) * 1.2,
            label,
            ha="center",
            va="bottom",
            fontsize=7.5,
        )
ax.set_yscale("log")
ax.set_ylim(0.1, 5e5)
ax.yaxis.set_major_formatter(fmt)
ax.set_xticks(x)
ax.set_xticklabels([g[2] for g in groups], fontsize=8.5)
ax.set_ylabel("ms (log)  ·  first_pe is epoch count")
ax.legend(fontsize=9)
ax.axvline(3.5, color="#999", lw=0.8, ls="--")
fig.suptitle(
    f"Gloas, N={N}: base vs full stack\n"
    "32768 validators — attester first_pe is empty-epoch replay after next-epoch proposer",
    fontsize=11,
)
fig.tight_layout(rect=(0, 0.05, 1, 1))
foot(fig)
save(fig, "gloas_N32_headline_base_vs_fix.png")


# --- 2/3. Base cost vs staleness per fork ---
def plot_base_scaling(fork: str, endpoints: list[str], title_ep: dict[str, str]):
    fig, axes = plt.subplots(1, len(endpoints), figsize=(4.2 * len(endpoints), 4.6), sharey=True)
    if len(endpoints) == 1:
        axes = [axes]
    for ax, ep in zip(axes, endpoints):
        for met, mk, ls, col in zip(
            ["first", "repeat", "wall"],
            "osD",
            ["--", "-", "-"],
            ["black", "#ff7f0e", "#2ca02c"],
        ):
            y = [jump[("base", fork, ep, n)][met] for n in NS]
            ax.plot(
                NS,
                y,
                marker=mk,
                ls=ls,
                color=col,
                ms=6,
                mfc="none" if met == "first" else col,
                label={"first": "first call", "repeat": "repeat mean (n=5)", "wall": "concurrent wall (8)"}[
                    met
                ],
            )
        first = [jump[("base", fork, ep, n)]["first"] for n in NS]
        slope = np.polyfit(NS, first, 1)[0]
        ax.set_title(f"{title_ep[ep]}\n≈ {slope:.0f} ms/epoch (first)", fontsize=10)
        ax.set_xlabel("N (epochs stale)")
        ax.set_xticks(NS)
        ax.yaxis.set_major_formatter(fmt)
    axes[0].set_ylabel("latency (ms)")
    axes[0].legend(fontsize=8, loc="upper left")
    fig.suptitle(
        f"{fork.capitalize()} — baseline (no cache): cost grows with stale-head depth\n32768 validators, 4 vCPU",
        fontsize=11,
    )
    fig.tight_layout(rect=(0, 0.05, 1, 1))
    foot(fig)
    save(fig, f"{fork}_base_cost_vs_staleness.png")


plot_base_scaling(
    "gloas",
    [
        "proposer_duties(next_epoch)",
        "attestation_data(current_slot)",
        "attester_duties(current_epoch)",
    ],
    {
        "proposer_duties(next_epoch)": "proposer duties",
        "attestation_data(current_slot)": "attestation_data",
        "attester_duties(current_epoch)": "attester duties",
    },
)
plot_base_scaling(
    "fulu",
    [
        "proposer_duties(next_epoch)",
        "attestation_data(current_slot)",
        "attester_duties(current_epoch)",
    ],
    {
        "proposer_duties(next_epoch)": "proposer duties",
        "attestation_data(current_slot)": "attestation_data",
        "attester_duties(current_epoch)": "attester duties",
    },
)


# --- 4/5. Fix vs base log plots ---
def plot_fix_vs_base(fork: str, endpoints: list[str]):
    metrics = ["repeat", "wall"]
    fig, axes = plt.subplots(
        len(endpoints), len(metrics), figsize=(8.2, 2.6 * len(endpoints)), sharex=True, sharey=True
    )
    if len(endpoints) == 1:
        axes = np.array([axes])
    for i, ep in enumerate(endpoints):
        for j, met in enumerate(metrics):
            ax = axes[i, j]
            for variant, mk in [("base", "o"), ("fix", "s")]:
                kw = dict(lw=4, alpha=0.35, ms=0) if variant == "base" else dict(lw=1.6, ms=5)
                ax.plot(
                    NS,
                    [jump[(variant, fork, ep, n)][met] for n in NS],
                    marker=mk,
                    color=COL[variant],
                    label=variant,
                    **kw,
                )
            ax.set_yscale("log")
            ax.set_xscale("log", base=2)
            ax.set_xticks(NS)
            ax.set_xticklabels(NS)
            ax.yaxis.set_major_formatter(fmt)
            ax.set_ylim(0.1, 5e5)
            if i == 0:
                ax.set_title(
                    {"repeat": "repeat mean (n=5)", "wall": "concurrent wall (8)"}[met],
                    fontsize=10,
                )
            if j == 0:
                ax.set_ylabel(f"{SHORT[ep]}\nms (log)", fontsize=9)
            if i == len(endpoints) - 1:
                ax.set_xlabel("epochs stale (N)")
    axes[0, 0].legend(fontsize=8, loc="upper left")
    fig.suptitle(
        f"{fork.capitalize()} — full stack vs baseline (log scale)\n"
        "wide translucent red = baseline; blue = stack tip 6663e7c5b",
        fontsize=11,
    )
    fig.tight_layout(rect=(0, 0.04, 1, 1))
    foot(fig)
    save(fig, f"{fork}_fix_vs_base_log.png")


plot_fix_vs_base(
    "gloas",
    [
        "proposer_duties(next_epoch)",
        "attestation_data(current_slot)",
        "attester_duties(current_epoch)",
        "ptc_duties(current_epoch)",
    ],
)
plot_fix_vs_base(
    "fulu",
    [
        "proposer_duties(next_epoch)",
        "attestation_data(current_slot)",
        "attester_duties(current_epoch)",
    ],
)


# --- 6. Incremental walk (Gloas proposer + attestation + attester) ---
fig, axes = plt.subplots(1, 3, figsize=(10.5, 4.2), sharex=True)
eps = [
    ("proposer_duties(next_epoch)", "proposer duties"),
    ("attestation_data(current_slot)", "attestation_data"),
    ("attester_duties(current_epoch)", "attester duties"),
]
xs = list(range(1, 33))
for ax, (ep, title) in zip(axes, eps):
    for variant in ["base", "fix"]:
        ys = [incr[(variant, "gloas", ep, n)]["first"] for n in xs]
        ax.plot(xs, ys, color=COL[variant], label=variant, lw=1.5 if variant == "fix" else 2.2, alpha=0.9)
    ax.set_title(title, fontsize=10)
    ax.set_xlabel("N (epochs stale)")
    ax.yaxis.set_major_formatter(fmt)
axes[0].set_ylabel("first-call ms after +1 epoch")
axes[0].legend(fontsize=8)
fig.suptitle(
    "Gloas — epoch-by-epoch walk: first-call cost after each +1 epoch step\n"
    "baseline grows with N; full stack stays flat (~1 epoch of work)",
    fontsize=11,
)
fig.tight_layout(rect=(0, 0.05, 1, 1))
foot(fig)
save(fig, "gloas_incremental_walk_base_vs_fix.png")

print("done:", len(files), "files")
