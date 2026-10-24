import re, numpy as np, matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter
from pathlib import Path

_HERE = Path(__file__).resolve().parent
SRC = _HERE.parent / "BENCHMARK_RESULTS.md"
OUT = _HERE
FOOT = "Source: b0a7/lighthouse @ cursor/stale-head-benchmark (9469ef6), BENCHMARK_RESULTS.md | 32768 validators, 4 vCPU Xeon VM"
plt.rcParams.update({"figure.facecolor":"white","axes.facecolor":"white","savefig.facecolor":"white",
    "font.size":10,"axes.grid":True,"grid.alpha":0.3})

# parse variant tables
data = {}  # (variant, fork, ep, N) -> dict
variant = None
for line in open(SRC):
    m = re.match(r"### stale-head benchmark variant=`([^`]+)`", line)
    if m: variant = m.group(1); continue
    if line.startswith("## Highlight"): variant = None
    if variant and line.startswith("| fulu") or variant and line.startswith("| gloas"):
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        ep = "proposer_duties" if c[1].startswith("proposer") else "attestation_data"
        v = list(map(float, c[3:]))
        data[(variant, c[0], ep, int(c[2]))] = dict(first=v[0], repeat=v[1], repeat_total=v[2], wall=v[3], cpu=v[4])
print(len(data), "rows")
VARS = ["base","proposer-fix","attestation-fix","both-fixes"]; FORKS=["fulu","gloas"]; EPS=["proposer_duties","attestation_data"]
NS = sorted({k[3] for k in data})
assert len(data)==4*2*2*len(NS)
EPLBL = {"proposer_duties":"proposer duties (next epoch)","attestation_data":"attestation_data (current slot)"}
MET = {"first":"first call","repeat":"repeat mean (n=5)","wall":"concurrent wall (8 parallel)","cpu":"concurrent CPU (8 parallel)"}
VCOL = {"base":"#d62728","proposer-fix":"#1f77b4","attestation-fix":"#2ca02c","both-fixes":"#9467bd"}
fmt = lambda x,_: (f"{x:g}" if x<1 else f"{x:,.0f}")

def foot(fig): fig.text(0.5, 0.01, FOOT, ha="center", va="bottom", fontsize=8, color="#555")
files=[]
def save(fig, name):
    p=f"{OUT}/{name}"; fig.savefig(p, dpi=150); plt.close(fig); files.append(p)

# 1. base scaling per fork
for fork in FORKS:
    fig, axes = plt.subplots(1,2,figsize=(8,4.667), sharey=True)
    for ax, ep in zip(axes, EPS):
        for met, mk, ls, col, ms in zip(["first","repeat","wall","cpu"], "osD^", ["--","-","-","-"], ["black","#ff7f0e","#2ca02c","#d62728"], [9,5,6,6]):
            y=[data[("base",fork,ep,n)][met] for n in NS]
            ax.plot(NS, y, marker=mk, ls=ls, color=col, ms=ms, mfc="none" if met=="first" else col, label=MET[met], zorder=3 if met=="first" else 2)
        f=[data[("base",fork,ep,n)]["first"] for n in NS]
        slope=np.polyfit(NS,f,1)[0]
        ax.set_title(f"{EPLBL[ep]}\nfirst-call slope ≈ {slope:.0f} ms/epoch", fontsize=10)
        ax.set_xlabel("N (epochs stale)"); ax.set_xticks([1,4,8,12,16,20,24,28,32])
        ax.yaxis.set_major_formatter(FuncFormatter(fmt))
    axes[0].set_ylabel("latency / CPU time (ms)"); axes[0].legend(fontsize=8, loc="upper left")
    fig.suptitle(f"{fork.capitalize()} — base (no fix): HTTP API cost vs stale-head epochs\n32768 validators, 4 vCPU", fontsize=11)
    fig.tight_layout(rect=(0,0.04,1,1)); foot(fig); save(fig, f"{fork}_base_cost_vs_staleness.png")

# 2. fix vs base per fork, log y
for fork in FORKS:
    fig, axes = plt.subplots(2,3,figsize=(8,4.667*1.25), sharey=True)
    for i, ep in enumerate(EPS):
        for j, met in enumerate(["repeat","wall","cpu"]):
            ax=axes[i,j]
            for v, mk in zip(VARS,"osD^"):
                kw = dict(lw=5, alpha=0.35, ms=0) if v=="base" else dict(lw=1.4, ms=4)
                ax.plot(NS,[data[(v,fork,ep,n)][met] for n in NS], marker=mk, color=VCOL[v], label=v, **kw)
            ax.set_yscale("log"); ax.set_xscale("log", base=2); ax.set_xticks(NS); ax.set_xticklabels(NS)
            ax.yaxis.set_major_formatter(FuncFormatter(fmt)); ax.set_ylim(0.1, 2e5)
            if i==0: ax.set_title(MET[met], fontsize=10)
            if j==0: ax.set_ylabel(f"{ep}\nms (log)", fontsize=9)
            if i==1: ax.set_xlabel("epochs stale (N)")
    axes[0,0].legend(fontsize=7, loc="upper left", ncol=2)
    fig.suptitle(f"{fork.capitalize()} — fix vs base: repeat & concurrent cost (log scale), 32768 validators, 4 vCPU\n(wide translucent red = base; the non-matching fix overlaps base, the matching fix overlaps both-fixes)", fontsize=11)
    fig.tight_layout(rect=(0,0.04,1,1)); foot(fig); save(fig, f"{fork}_fix_vs_base_log.png")

# 3. headline Gloas N=max
N=max(NS); fork="gloas"
groups=[(ep,met) for ep in EPS for met in ["repeat","wall","cpu"]]
match={"proposer_duties":"proposer-fix","attestation_data":"attestation-fix"}
fig, ax = plt.subplots(figsize=(8,4.667))
x=np.arange(len(groups)); w=0.27
series=[("base",lambda ep:"base"),("matching fix",lambda ep:match[ep]),("both-fixes",lambda ep:"both-fixes")]
cols=["#d62728","#1f77b4","#9467bd"]
for k,(lbl,fn) in enumerate(series):
    vals=[data[(fn(ep),fork,ep,N)][met] for ep,met in groups]
    bars=ax.bar(x+(k-1)*w, vals, w, label=lbl, color=cols[k])
    for b,v in zip(bars,vals):
        ax.text(b.get_x()+b.get_width()/2, v*1.15, f"{v:,.0f}" if v>=100 else f"{v:.2f}", ha="center", va="bottom", fontsize=7, rotation=0)
ax.set_yscale("log"); ax.set_ylim(0.1, 3e6); ax.yaxis.set_major_formatter(FuncFormatter(fmt))
ax.set_xticks(x); ax.set_xticklabels({"repeat":"repeat mean\n(n=5)","wall":"concurrent wall\n(8 parallel)","cpu":"concurrent CPU\n(8 parallel)"}[m] for ep,m in groups); ax.tick_params(axis="x", labelsize=8)
for gx, ep in [(1,"proposer_duties (next epoch)"),(4,"attestation_data (current slot)")]:
    ax.text(gx, 1e6, ep, ha="center", fontsize=9.5, weight="bold")
ax.set_ylabel("ms (log scale)"); ax.legend(fontsize=8, loc="upper center", bbox_to_anchor=(0.5,0.9), ncol=3)
ax.axvline(2.5, color="#999", lw=0.8)
fig.suptitle(f"Gloas, N={N} epochs stale (worst case): base vs fix\n32768 validators, 4 vCPU — matching fix = proposer-fix / attestation-fix for its endpoint", fontsize=10.5)
fig.tight_layout(rect=(0,0.04,1,1)); foot(fig); save(fig, f"gloas_N{N}_headline_base_vs_fix.png")

# cross-check headline vs Highlight table
hl={}
for line in open(SRC).read().split("## Highlight")[1].splitlines():
    if line.startswith("| ") and not line.startswith("| Variant") :
        c=[x.strip().strip("*") for x in line.strip().strip("|").split("|")]
        hl[(c[0],c[1])]=list(map(float,c[2:]))
for (v,ep),vals in hl.items():
    d=data[(v,"gloas",ep,32)]; assert [d["first"],d["repeat"],d["wall"],d["cpu"]]==vals,(v,ep)
print("highlight table consistent")
print("\n".join(files))
