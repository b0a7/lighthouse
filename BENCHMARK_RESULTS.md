# Stale-head HTTP API cost benchmark (#9530)

Evidence-only results for [sigp/lighthouse#9530](https://github.com/sigp/lighthouse/issues/9530).
**Not intended to be merged.**

## Reproduce

```bash
# Base everything on upstream unstable ef55e2fbf (same as the fix branches).
git fetch https://github.com/sigp/lighthouse ef55e2fbf
git checkout cursor/stale-head-benchmark

# Release build; http_api integration tests are compiled out in debug.
RUST_LOG=error \
STALE_HEAD_BENCH_VALIDATORS=32768 \
STALE_HEAD_BENCH_VARIANT=base \
STALE_HEAD_BENCH_FORKS=fulu,gloas \
cargo test -p http_api --release --test bn_http_api_tests \
  stale_head_http_api_benchmark -- --ignored --nocapture
```

To compare against the existing fix branches (do not modify those branches; cherry-pick locally):

```bash
# + proposer duties fix
git cherry-pick 658aa05d71d65548ff8402190eabbaa72f3ee27f

# + attestation_data fix
git cherry-pick b28a774745ee302c9252e09ee7d1878d499b6624
```

## Environment

| Item | Value |
|---|---|
| Benchmark branch | `cursor/stale-head-benchmark` @ `1682fddf29e87d4b6ef7d9e5995d7634a34e0b1c` |
| Unstable base | `ef55e2fbf` (`Unschedule Gloas on Hoodi`) |
| Proposer-duties fix | `658aa05d71d65548ff8402190eabbaa72f3ee27f` (from `cursor/proposer-duties-next-epoch-cache`) |
| Attestation-data fix | `b28a774745ee302c9252e09ee7d1878d499b6624` (from `cursor/attestation-data-stale-head-cache`) |
| CPU | 4× Intel Xeon (VM), `available_parallelism=4` |
| Memory | 15 GiB |
| OS | Linux 6.12 / Ubuntu 24.04 |
| Rust | `rustc 1.99.1` |
| Validators | **32768** (MainnetEthSpec; 64k hit the old 5s HTTP timeout on concurrent N=32 before the bench raised client timeouts to 120s) |
| Setup | 3 epochs of chain with 64 attesters/slot, then jump wall clock by N epochs with no new blocks |
| Repeat / concurrent | 5 sequential repeats; 8 parallel identical calls (warm after first) |
| Endpoints | `GET /eth/v1/validator/duties/proposer/{current+1}`, `GET /eth/v1/validator/attestation_data` at current slot |

## Findings (short)

Cost scales **roughly linearly with staleness N** on both Fulu and Gloas: first-call time ≈ `c × N` (Fulu ≈ 37 ms/epoch, Gloas ≈ 300 ms/epoch on this machine — Gloas is ~8× more expensive per epoch). Without fixes, every repeat and every concurrent caller re-pays that full advance (concurrent wall ≈ 4× first on 4 CPUs; concurrent CPU ≈ 8× first). With the matching fix, **repeat and concurrent drop to sub-millisecond** (roughly **10³–10⁴×** cheaper than the uncached path at N=32) while first-call cost is unchanged.

---

## Variant 1 — bench alone (`ef55e2fbf` + benchmark)

### stale-head benchmark variant=`base` validators=32768

| fork | endpoint | stale_epochs | first_ms | repeat_mean_ms (n=5) | repeat_total_ms | concurrent_wall_ms (n=8) | concurrent_cpu_ms |
|---|---|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 37.32 | 35.88 | 179.42 | 152.47 | 304.41 |
| fulu | attestation_data(current_slot) | 1 | 36.39 | 35.69 | 178.46 | 145.50 | 285.14 |
| fulu | proposer_duties(next_epoch) | 2 | 78.02 | 78.25 | 391.23 | 312.52 | 624.23 |
| fulu | attestation_data(current_slot) | 2 | 77.88 | 77.63 | 388.13 | 312.25 | 622.52 |
| fulu | proposer_duties(next_epoch) | 4 | 149.26 | 148.38 | 741.88 | 592.26 | 1184.42 |
| fulu | attestation_data(current_slot) | 4 | 148.50 | 148.35 | 741.73 | 593.00 | 1184.16 |
| fulu | proposer_duties(next_epoch) | 8 | 299.15 | 297.85 | 1489.26 | 1194.96 | 2383.55 |
| fulu | attestation_data(current_slot) | 8 | 298.14 | 296.94 | 1484.72 | 1192.33 | 2381.82 |
| fulu | proposer_duties(next_epoch) | 16 | 593.86 | 604.58 | 3022.88 | 2395.72 | 4784.95 |
| fulu | attestation_data(current_slot) | 16 | 595.87 | 596.73 | 2983.63 | 2396.96 | 4787.95 |
| fulu | proposer_duties(next_epoch) | 32 | 1190.66 | 1196.39 | 5981.96 | 4844.43 | 9662.30 |
| fulu | attestation_data(current_slot) | 32 | 1198.25 | 1197.63 | 5988.14 | 4795.33 | 9587.17 |
| gloas | proposer_duties(next_epoch) | 1 | 301.66 | 299.28 | 1496.38 | 1260.12 | 2456.77 |
| gloas | attestation_data(current_slot) | 1 | 299.62 | 301.40 | 1506.99 | 1193.52 | 2385.17 |
| gloas | proposer_duties(next_epoch) | 2 | 600.76 | 606.17 | 3030.86 | 2423.29 | 4819.85 |
| gloas | attestation_data(current_slot) | 2 | 605.09 | 610.15 | 3050.74 | 2428.99 | 4853.90 |
| gloas | proposer_duties(next_epoch) | 4 | 1219.29 | 1218.00 | 6089.98 | 4880.46 | 9760.37 |
| gloas | attestation_data(current_slot) | 4 | 1222.01 | 1225.26 | 6126.31 | 4901.28 | 9774.18 |
| gloas | proposer_duties(next_epoch) | 8 | 2426.27 | 2438.92 | 12194.62 | 9724.97 | 19398.83 |
| gloas | attestation_data(current_slot) | 8 | 2446.45 | 2438.08 | 12190.41 | 9859.70 | 19638.85 |
| gloas | proposer_duties(next_epoch) | 16 | 4886.46 | 4897.16 | 24485.79 | 19632.97 | 39239.60 |
| gloas | attestation_data(current_slot) | 16 | 4944.12 | 4978.75 | 24893.75 | 20014.40 | 39567.61 |
| gloas | proposer_duties(next_epoch) | 32 | 9714.72 | 9984.93 | 49924.66 | 39780.44 | 78992.98 |
| gloas | attestation_data(current_slot) | 32 | 9831.15 | 9965.87 | 49829.34 | 40216.79 | 79767.38 |

---

## Variant 2 — bench + proposer-duties fix

Cherry-pick: `658aa05d71d65548ff8402190eabbaa72f3ee27f`

### stale-head benchmark variant=`proposer-fix` validators=32768

| fork | endpoint | stale_epochs | first_ms | repeat_mean_ms (n=5) | repeat_total_ms | concurrent_wall_ms (n=8) | concurrent_cpu_ms |
|---|---|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 36.97 | 0.64 | 3.20 | 0.95 | 1.86 |
| fulu | attestation_data(current_slot) | 1 | 35.89 | 35.95 | 179.74 | 143.54 | 286.28 |
| fulu | proposer_duties(next_epoch) | 2 | 78.09 | 0.53 | 2.64 | 0.61 | 1.89 |
| fulu | attestation_data(current_slot) | 2 | 78.71 | 78.57 | 392.84 | 313.43 | 623.65 |
| fulu | proposer_duties(next_epoch) | 4 | 148.82 | 0.43 | 2.16 | 0.71 | 1.57 |
| fulu | attestation_data(current_slot) | 4 | 147.70 | 148.23 | 741.16 | 598.17 | 1193.11 |
| fulu | proposer_duties(next_epoch) | 8 | 297.48 | 0.47 | 2.37 | 0.55 | 1.60 |
| fulu | attestation_data(current_slot) | 8 | 297.34 | 300.24 | 1501.20 | 1199.66 | 2392.70 |
| fulu | proposer_duties(next_epoch) | 16 | 596.35 | 0.40 | 1.99 | 0.65 | 2.02 |
| fulu | attestation_data(current_slot) | 16 | 602.26 | 597.89 | 2989.45 | 2397.59 | 4782.73 |
| fulu | proposer_duties(next_epoch) | 32 | 1181.39 | 0.61 | 3.03 | 0.66 | 1.67 |
| fulu | attestation_data(current_slot) | 32 | 1190.08 | 1185.18 | 5925.89 | 4754.30 | 9497.00 |
| gloas | proposer_duties(next_epoch) | 1 | 299.60 | 0.69 | 3.44 | 1.39 | 2.73 |
| gloas | attestation_data(current_slot) | 1 | 300.26 | 300.21 | 1501.03 | 1207.88 | 2403.67 |
| gloas | proposer_duties(next_epoch) | 2 | 588.48 | 0.71 | 3.55 | 0.87 | 2.03 |
| gloas | attestation_data(current_slot) | 2 | 603.27 | 598.62 | 2993.12 | 2388.48 | 4767.94 |
| gloas | proposer_duties(next_epoch) | 4 | 1206.89 | 0.71 | 3.55 | 0.65 | 1.99 |
| gloas | attestation_data(current_slot) | 4 | 1223.46 | 1229.37 | 6146.86 | 4887.85 | 9751.83 |
| gloas | proposer_duties(next_epoch) | 8 | 2413.48 | 0.69 | 3.47 | 0.75 | 2.05 |
| gloas | attestation_data(current_slot) | 8 | 2455.73 | 2435.77 | 12178.85 | 9765.18 | 19505.42 |
| gloas | proposer_duties(next_epoch) | 16 | 4880.92 | 0.70 | 3.52 | 0.69 | 2.08 |
| gloas | attestation_data(current_slot) | 16 | 4981.95 | 4958.27 | 24791.33 | 19841.34 | 39615.98 |
| gloas | proposer_duties(next_epoch) | 32 | 9895.19 | 0.81 | 4.03 | 0.82 | 2.15 |
| gloas | attestation_data(current_slot) | 32 | 10107.02 | 10109.44 | 50547.21 | 40086.28 | 79543.96 |

---

## Variant 3 — bench + attestation-data fix

Cherry-pick: `b28a774745ee302c9252e09ee7d1878d499b6624`

### stale-head benchmark variant=`attestation-fix` validators=32768

| fork | endpoint | stale_epochs | first_ms | repeat_mean_ms (n=5) | repeat_total_ms | concurrent_wall_ms (n=8) | concurrent_cpu_ms |
|---|---|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 36.40 | 35.62 | 178.09 | 141.83 | 282.89 |
| fulu | attestation_data(current_slot) | 1 | 35.84 | 0.24 | 1.19 | 0.45 | 1.47 |
| fulu | proposer_duties(next_epoch) | 2 | 78.03 | 78.40 | 392.01 | 311.21 | 621.26 |
| fulu | attestation_data(current_slot) | 2 | 78.33 | 0.44 | 2.19 | 0.69 | 1.30 |
| fulu | proposer_duties(next_epoch) | 4 | 148.44 | 148.78 | 743.88 | 591.18 | 1181.87 |
| fulu | attestation_data(current_slot) | 4 | 148.61 | 0.71 | 3.55 | 0.51 | 1.40 |
| fulu | proposer_duties(next_epoch) | 8 | 296.35 | 296.95 | 1484.76 | 1193.42 | 2382.26 |
| fulu | attestation_data(current_slot) | 8 | 297.54 | 0.47 | 2.37 | 0.81 | 2.01 |
| fulu | proposer_duties(next_epoch) | 16 | 592.50 | 592.40 | 2962.02 | 2376.66 | 4751.90 |
| fulu | attestation_data(current_slot) | 16 | 591.66 | 0.50 | 2.52 | 0.45 | 1.49 |
| fulu | proposer_duties(next_epoch) | 32 | 1178.04 | 1192.77 | 5963.87 | 4811.64 | 9617.52 |
| fulu | attestation_data(current_slot) | 32 | 1187.38 | 0.55 | 2.77 | 0.62 | 1.46 |
| gloas | proposer_duties(next_epoch) | 1 | 297.87 | 294.62 | 1473.10 | 1199.43 | 2380.92 |
| gloas | attestation_data(current_slot) | 1 | 295.02 | 0.43 | 2.14 | 0.52 | 1.68 |
| gloas | proposer_duties(next_epoch) | 2 | 601.19 | 591.97 | 2959.87 | 2407.36 | 4782.92 |
| gloas | attestation_data(current_slot) | 2 | 597.79 | 0.69 | 3.44 | 0.50 | 1.61 |
| gloas | proposer_duties(next_epoch) | 4 | 1207.79 | 1226.35 | 6131.73 | 4869.40 | 9703.79 |
| gloas | attestation_data(current_slot) | 4 | 1203.03 | 0.71 | 3.53 | 0.59 | 1.80 |
| gloas | proposer_duties(next_epoch) | 8 | 2528.95 | 2418.64 | 12093.18 | 9722.74 | 19437.55 |
| gloas | attestation_data(current_slot) | 8 | 2420.03 | 0.69 | 3.45 | 0.52 | 1.77 |
| gloas | proposer_duties(next_epoch) | 16 | 4929.05 | 4892.66 | 24463.31 | 19657.73 | 39229.75 |
| gloas | attestation_data(current_slot) | 16 | 4819.05 | 0.61 | 3.06 | 0.57 | 1.65 |
| gloas | proposer_duties(next_epoch) | 32 | 9725.98 | 10071.68 | 50358.38 | 39546.23 | 78922.32 |
| gloas | attestation_data(current_slot) | 32 | 9785.52 | 0.90 | 4.51 | 0.94 | 2.11 |

---

## Variant 4 — bench + both fixes

Cherry-picks: `658aa05d7` then `b28a77474`

### stale-head benchmark variant=`both-fixes` validators=32768

| fork | endpoint | stale_epochs | first_ms | repeat_mean_ms (n=5) | repeat_total_ms | concurrent_wall_ms (n=8) | concurrent_cpu_ms |
|---|---|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 37.16 | 0.61 | 3.05 | 0.90 | 2.58 |
| fulu | attestation_data(current_slot) | 1 | 36.11 | 0.53 | 2.63 | 0.41 | 1.23 |
| fulu | proposer_duties(next_epoch) | 2 | 78.89 | 0.32 | 1.61 | 0.51 | 1.53 |
| fulu | attestation_data(current_slot) | 2 | 80.76 | 0.38 | 1.91 | 0.45 | 1.40 |
| fulu | proposer_duties(next_epoch) | 4 | 151.05 | 0.31 | 1.56 | 0.46 | 1.49 |
| fulu | attestation_data(current_slot) | 4 | 150.63 | 0.21 | 1.06 | 0.40 | 1.17 |
| fulu | proposer_duties(next_epoch) | 8 | 301.11 | 0.60 | 3.00 | 0.88 | 1.95 |
| fulu | attestation_data(current_slot) | 8 | 301.26 | 0.47 | 2.33 | 0.81 | 1.52 |
| fulu | proposer_duties(next_epoch) | 16 | 602.70 | 0.52 | 2.60 | 0.64 | 1.87 |
| fulu | attestation_data(current_slot) | 16 | 601.80 | 0.44 | 2.22 | 0.41 | 1.33 |
| fulu | proposer_duties(next_epoch) | 32 | 1194.27 | 0.76 | 3.80 | 0.63 | 1.84 |
| fulu | attestation_data(current_slot) | 32 | 1193.71 | 0.46 | 2.32 | 0.46 | 1.47 |
| gloas | proposer_duties(next_epoch) | 1 | 297.04 | 0.45 | 2.23 | 0.93 | 2.53 |
| gloas | attestation_data(current_slot) | 1 | 294.33 | 0.53 | 2.66 | 0.84 | 1.77 |
| gloas | proposer_duties(next_epoch) | 2 | 605.51 | 0.61 | 3.05 | 0.48 | 1.53 |
| gloas | attestation_data(current_slot) | 2 | 596.15 | 0.54 | 2.69 | 0.60 | 1.53 |
| gloas | proposer_duties(next_epoch) | 4 | 1235.38 | 0.73 | 3.63 | 0.68 | 2.09 |
| gloas | attestation_data(current_slot) | 4 | 1212.28 | 0.28 | 1.40 | 1.06 | 1.69 |
| gloas | proposer_duties(next_epoch) | 8 | 2422.73 | 0.68 | 3.38 | 1.47 | 1.91 |
| gloas | attestation_data(current_slot) | 8 | 2403.30 | 0.60 | 2.99 | 0.47 | 1.59 |
| gloas | proposer_duties(next_epoch) | 16 | 4900.84 | 0.66 | 3.31 | 0.65 | 1.75 |
| gloas | attestation_data(current_slot) | 16 | 4806.46 | 0.74 | 3.71 | 0.58 | 1.68 |
| gloas | proposer_duties(next_epoch) | 32 | 9813.34 | 0.80 | 3.99 | 0.99 | 2.18 |
| gloas | attestation_data(current_slot) | 32 | 9684.46 | 0.83 | 4.13 | 0.59 | 1.70 |

---

## Highlight: N=32 Gloas (worst row)

| Variant | Endpoint | first_ms | repeat_mean_ms | concurrent_wall_ms | concurrent_cpu_ms |
|---|---|---:|---:|---:|---:|
| base | proposer_duties | 9714.72 | 9984.93 | 39780.44 | 78992.98 |
| base | attestation_data | 9831.15 | 9965.87 | 40216.79 | 79767.38 |
| proposer-fix | proposer_duties | 9895.19 | **0.81** | **0.82** | **2.15** |
| proposer-fix | attestation_data | 10107.02 | 10109.44 | 40086.28 | 79543.96 |
| attestation-fix | proposer_duties | 9725.98 | 10071.68 | 39546.23 | 78922.32 |
| attestation-fix | attestation_data | 9785.52 | **0.90** | **0.94** | **2.11** |
| both-fixes | proposer_duties | 9813.34 | **0.80** | **0.99** | **2.18** |
| both-fixes | attestation_data | 9684.46 | **0.83** | **0.59** | **1.70** |

---

## Graphs

See [`benchmark-graphs/README.md`](benchmark-graphs/README.md) for the full set of plots (Fulu and Gloas, base scaling and fix-vs-base).

![Fulu base cost vs staleness](benchmark-graphs/fulu_base_cost_vs_staleness.png)

![Fulu fix vs base (log)](benchmark-graphs/fulu_fix_vs_base_log.png)
