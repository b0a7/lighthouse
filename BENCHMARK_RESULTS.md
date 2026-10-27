# Stale-head advanced-state benchmark (#9530)

Evidence-only results for [sigp/lighthouse#9530](https://github.com/sigp/lighthouse/issues/9530).
**Not intended to be merged.** Branch: `cursor/stale-head-benchmark-advanced-state`.

Refreshed onto fix tip `c4a7b63ef` (exact-slot advanced-state retention + committee-shuffling
prime after next-epoch proposer advances). Frozen public tip `cursor/stale-head-benchmark` @
`4504e7bdf` is untouched.

## Reproduce

```bash
git fetch origin cursor/stale-head-benchmark-advanced-state
git checkout cursor/stale-head-benchmark-advanced-state

RUST_LOG=error \
STALE_HEAD_BENCH_VALIDATORS=32768 \
STALE_HEAD_BENCH_VARIANT=advanced-state-c4a7b63 \
STALE_HEAD_BENCH_FORKS=fulu,gloas \
cargo test -p http_api --release --test bn_http_api_tests \
  stale_head_http_api_benchmark -- --ignored --nocapture

RUST_LOG=error \
STALE_HEAD_BENCH_VALIDATORS=32768 \
STALE_HEAD_BENCH_VARIANT=advanced-state-c4a7b63 \
STALE_HEAD_BENCH_FORKS=fulu,gloas \
cargo test -p http_api --release --test bn_http_api_tests \
  stale_head_incremental_epoch_benchmark -- --ignored --nocapture
```

Baseline comparison uses the same harness cherry-picked onto `ef55e2fbf` (parent of the fix
stack) without the advanced-state commits.

## Environment

| Item | Value |
|---|---|
| Benchmark branch tip (after this refresh) | see git log on `cursor/stale-head-benchmark-advanced-state` |
| Fix tip | `c4a7b63ef0b603858c870331f78ebacaa0cbed02` |
| Unstable base (baseline) | `ef55e2fbf` |
| CPU | 4× Intel Xeon (VM), `available_parallelism=4` |
| Memory | 15 GiB |
| OS | Linux 6.12 / Ubuntu 24.04 |
| Rust | `rustc 1.99.1` |
| Validators | **32768** (MainnetEthSpec) |
| Forks | Fulu and Gloas |
| Setup | 3 epochs of chain with 64 attesters/slot, then jump wall clock |
| Call order per N | next-epoch **proposer** → **attestation_data** → current-epoch **attester** (then PTC on Gloas, block production, states/validators) |
| Repeat / concurrent | 5 sequential repeats; 8 parallel identical calls (warm after first) |
| `first_pe` / `*_pe` | delta of `beacon_state_processing_process_epoch` histogram samples |

Timings are single-run wall-clock (no multi-trial median/p95). `repeat_mean_ms` is the mean of 5 sequential repeats.

## Headline before/after (N=32, after next-epoch proposer)

Restart-onto-old-head sequence: first next-epoch proposer call, then first current-epoch attester
call on the same stale head. `first_pe` is empty-epoch replay count.

| Fork | Endpoint | Baseline first_ms | Baseline first_pe | Fix first_ms | Fix first_pe | Fix repeat_mean_ms |
|---|---|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1191.31 | 32 | 600.04† | 16† | 1.03 |
| fulu | attestation_data(current_slot) | 1179.36 | 32 | 0.37 | 0 | 0.56 |
| fulu | attester_duties(current_epoch) **after proposer** | 1212.27 | 31 | **70.32** | **0** | 72.88 |
| gloas | proposer_duties(next_epoch) | 10015.38 | 32 | 4987.20† | 16† | 0.96 |
| gloas | attestation_data(current_slot) | 10159.48 | 32 | 0.42 | 0 | 0.38 |
| gloas | attester_duties(current_epoch) **after proposer** | 9680.87 | 31 | **68.58** | **0** | 67.64 |

† Jump-to-N reuses one harness across N∈{1,2,4,8,16,32}, so on the fix the retained advanced
state from the previous N means the N=32 proposer first call only pays the remaining epochs
(`first_pe≈16`). Baseline always advances from the head (`first_pe=32`). The attester-after-proposer
comparison is apples-to-apples within each variant: baseline still replays (`first_pe=31`);
`c4a7b63` serves attester duties from the primed shuffling cache (`first_pe=0`).

Versus the previous advanced-state tip before shuffling priming (Variant 7 @ `53a3609f`),
attester-after-proposer at Fulu N=32 dropped from ~636 ms to ~70 ms; Gloas N=32 from ~4699 ms
to ~69 ms — matching the unit test move from `process_epoch` delta 11 → 0.

### Epoch-by-epoch walk (attester after proposer each step)

| Fork | Step N | Baseline attester first_ms (pe) | Fix attester first_ms (pe) |
|---|---:|---:|---:|
| fulu | 1 | 73.72 (0) | 73.87 (0) |
| fulu | 8 | 333.13 (7) | 67.39 (0) |
| fulu | 16 | 625.26 (15) | 67.49 (0) |
| fulu | 32 | 1214.68 (31) | 66.66 (0) |
| gloas | 1 | 75.03 (0) | 71.75 (0) |
| gloas | 8 | 2238.20 (7) | 68.26 (0) |
| gloas | 16 | 4805.14 (15) | 67.77 (0) |
| gloas | 32 | 10875.21 (31) | 69.08 (0) |

### What is still expensive / not slower

- **Gloas PTC duties** still scale with N on the fix (N=32 first ≈ 4.8 s, `first_pe=15`; repeats
  also re-pay). Baseline was worse (~9.8 s / pe=31). Not a regression vs unstable; still unfixed.
- **Block production first call** still pays a partial advance proportional to how far the
  retained state lags the wall clock; repeats stay ~260 ms. Not slower than baseline.
- Nothing measured was slower on `c4a7b63` than on `ef55e2fbf` for the same harness.

## Findings (short)

Cost still scales roughly linearly with staleness on the unfixed baseline. With
`c4a7b63`, next-epoch proposer and attestation_data repeats/concurrent stay sub-ms after the
first advance; current-epoch attester duties after that proposer advance no longer replay
(`first_pe=0`, ~70 ms flat at every N). The epoch-by-epoch walk stays flat on the fix and
linear on baseline.

---

## Variant A — baseline (`ef55e2fbf` + harness only)

### stale-head benchmark variant=`baseline-ef55e2fbf` validators=32768
| fork | endpoint | stale_epochs | first_ms | first_pe | repeat_mean_ms (n=5) | repeat_total_ms | repeat_pe_total | concurrent_wall_ms (n=8) | concurrent_cpu_ms | concurrent_pe |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 37.49 | 1 | 35.50 | 177.49 | 5 | 142.46 | 285.05 | 8 |
| fulu | attestation_data(current_slot) | 1 | 35.91 | 1 | 35.94 | 179.68 | 5 | 142.15 | 283.09 | 8 |
| fulu | attester_duties(current_epoch) | 1 | 74.21 | 0 | 72.13 | 360.63 | 0 | 289.38 | 578.72 | 0 |
| fulu | block_production(v3) | 1 | 316.73 | 1 | 312.67 | 1563.37 | 5 | 1247.75 | 507.04 | 8 |
| fulu | states/{slot}/validators(past_head) | 1 | 75.45 | 0 | 73.92 | 369.59 | 0 | 380.31 | 649.28 | 0 |
| fulu | proposer_duties(next_epoch) | 2 | 86.19 | 2 | 78.26 | 391.30 | 10 | 310.72 | 620.12 | 16 |
| fulu | attestation_data(current_slot) | 2 | 78.26 | 2 | 77.94 | 389.71 | 10 | 311.55 | 622.21 | 16 |
| fulu | attester_duties(current_epoch) | 2 | 103.95 | 1 | 70.53 | 352.64 | 0 | 281.18 | 564.84 | 0 |
| fulu | block_production(v3) | 2 | 374.15 | 2 | 369.25 | 1846.23 | 10 | 1499.32 | 1001.92 | 16 |
| fulu | states/{slot}/validators(past_head) | 2 | 74.33 | 0 | 73.76 | 368.80 | 0 | 367.05 | 625.99 | 0 |
| fulu | proposer_duties(next_epoch) | 4 | 155.48 | 4 | 148.26 | 741.28 | 20 | 590.88 | 1181.81 | 32 |
| fulu | attestation_data(current_slot) | 4 | 147.29 | 4 | 148.06 | 740.29 | 20 | 590.26 | 1179.81 | 32 |
| fulu | attester_duties(current_epoch) | 4 | 181.97 | 3 | 69.33 | 346.66 | 0 | 281.05 | 572.53 | 0 |
| fulu | block_production(v3) | 4 | 472.45 | 4 | 468.23 | 2341.16 | 20 | 1890.42 | 1839.24 | 32 |
| fulu | states/{slot}/validators(past_head) | 4 | 71.79 | 0 | 72.57 | 362.87 | 0 | 379.79 | 637.61 | 0 |
| fulu | proposer_duties(next_epoch) | 8 | 304.89 | 8 | 298.64 | 1493.22 | 40 | 1188.56 | 2373.23 | 64 |
| fulu | attestation_data(current_slot) | 8 | 296.37 | 8 | 297.36 | 1486.82 | 40 | 1187.90 | 2373.03 | 64 |
| fulu | attester_duties(current_epoch) | 8 | 337.97 | 7 | 70.40 | 351.98 | 0 | 278.36 | 566.04 | 0 |
| fulu | block_production(v3) | 8 | 680.62 | 8 | 676.07 | 3380.35 | 40 | 2776.76 | 3641.33 | 64 |
| fulu | states/{slot}/validators(past_head) | 8 | 72.73 | 0 | 74.02 | 370.11 | 0 | 369.80 | 623.73 | 0 |
| fulu | proposer_duties(next_epoch) | 16 | 600.57 | 16 | 593.33 | 2966.67 | 80 | 2365.63 | 4730.07 | 128 |
| fulu | attestation_data(current_slot) | 16 | 592.26 | 16 | 591.98 | 2959.91 | 80 | 2369.68 | 4739.12 | 128 |
| fulu | attester_duties(current_epoch) | 16 | 626.50 | 15 | 71.01 | 355.06 | 0 | 277.66 | 564.63 | 0 |
| fulu | block_production(v3) | 16 | 1101.78 | 16 | 1093.94 | 5469.68 | 80 | 4466.54 | 7248.57 | 128 |
| fulu | states/{slot}/validators(past_head) | 16 | 73.28 | 0 | 73.26 | 366.29 | 0 | 391.06 | 673.56 | 0 |
| fulu | proposer_duties(next_epoch) | 32 | 1191.31 | 32 | 1179.99 | 5899.97 | 160 | 4704.18 | 9406.55 | 256 |
| fulu | attestation_data(current_slot) | 32 | 1179.36 | 32 | 1191.46 | 5957.32 | 160 | 4725.32 | 9441.88 | 256 |
| fulu | attester_duties(current_epoch) | 32 | 1212.27 | 31 | 71.61 | 358.04 | 0 | 280.64 | 581.92 | 0 |
| fulu | block_production(v3) | 32 | 1927.81 | 32 | 1915.50 | 9577.50 | 160 | 7723.05 | 14410.48 | 256 |
| fulu | states/{slot}/validators(past_head) | 32 | 73.89 | 0 | 74.14 | 370.72 | 0 | 383.72 | 634.67 | 0 |
| gloas | proposer_duties(next_epoch) | 1 | 301.56 | 1 | 298.59 | 1492.94 | 5 | 1199.55 | 2384.29 | 8 |
| gloas | attestation_data(current_slot) | 1 | 303.53 | 1 | 305.20 | 1525.98 | 5 | 1216.20 | 2431.34 | 8 |
| gloas | attester_duties(current_epoch) | 1 | 76.69 | 0 | 68.96 | 344.80 | 0 | 284.30 | 569.18 | 0 |
| gloas | ptc_duties(current_epoch) | 1 | 158.54 | 0 | 156.64 | 783.22 | 0 | 600.90 | 1253.43 | 0 |
| gloas | block_production(v4) | 1 | 580.51 | 1 | 576.09 | 2880.45 | 5 | 2333.38 | 2651.08 | 8 |
| gloas | states/{slot}/validators(past_head) | 1 | 74.97 | 0 | 73.30 | 366.52 | 0 | 388.87 | 644.74 | 0 |
| gloas | proposer_duties(next_epoch) | 2 | 610.63 | 2 | 603.02 | 3015.08 | 10 | 2390.38 | 4780.28 | 16 |
| gloas | attestation_data(current_slot) | 2 | 608.97 | 2 | 619.61 | 3098.04 | 10 | 2423.80 | 4841.51 | 16 |
| gloas | attester_duties(current_epoch) | 2 | 367.09 | 1 | 68.71 | 343.57 | 0 | 276.48 | 560.41 | 0 |
| gloas | ptc_duties(current_epoch) | 2 | 454.23 | 1 | 447.34 | 2236.68 | 5 | 1759.99 | 3574.48 | 8 |
| gloas | block_production(v4) | 2 | 898.98 | 2 | 897.51 | 4487.55 | 10 | 3582.70 | 5198.78 | 16 |
| gloas | states/{slot}/validators(past_head) | 2 | 76.13 | 0 | 75.25 | 376.23 | 0 | 374.24 | 639.83 | 0 |
| gloas | proposer_duties(next_epoch) | 4 | 1219.30 | 4 | 1220.07 | 6100.36 | 20 | 4867.99 | 9734.41 | 32 |
| gloas | attestation_data(current_slot) | 4 | 1247.30 | 4 | 1236.52 | 6182.61 | 20 | 4935.25 | 9870.34 | 32 |
| gloas | attester_duties(current_epoch) | 4 | 974.96 | 3 | 67.77 | 338.85 | 0 | 270.60 | 550.69 | 0 |
| gloas | ptc_duties(current_epoch) | 4 | 1070.92 | 3 | 1051.51 | 5257.56 | 15 | 4224.34 | 8485.15 | 24 |
| gloas | block_production(v4) | 4 | 1559.60 | 4 | 1543.95 | 7719.77 | 20 | 6206.61 | 10465.21 | 32 |
| gloas | states/{slot}/validators(past_head) | 4 | 78.18 | 0 | 72.92 | 364.61 | 0 | 364.09 | 635.55 | 0 |
| gloas | proposer_duties(next_epoch) | 8 | 2441.78 | 8 | 2475.99 | 12379.96 | 40 | 10012.44 | 19975.61 | 64 |
| gloas | attestation_data(current_slot) | 8 | 2505.70 | 8 | 2504.98 | 12524.91 | 40 | 9932.47 | 19820.86 | 64 |
| gloas | attester_duties(current_epoch) | 8 | 2237.18 | 7 | 69.79 | 348.94 | 0 | 278.22 | 561.77 | 0 |
| gloas | ptc_duties(current_epoch) | 8 | 2339.78 | 7 | 2283.72 | 11418.58 | 35 | 9102.83 | 18216.68 | 56 |
| gloas | block_production(v4) | 8 | 2813.89 | 8 | 2803.41 | 14017.07 | 40 | 11313.22 | 20771.57 | 64 |
| gloas | states/{slot}/validators(past_head) | 8 | 76.32 | 0 | 73.35 | 366.73 | 0 | 366.81 | 638.74 | 0 |
| gloas | proposer_duties(next_epoch) | 16 | 4913.59 | 16 | 5018.62 | 25093.09 | 80 | 19761.86 | 39352.46 | 128 |
| gloas | attestation_data(current_slot) | 16 | 4958.25 | 16 | 4987.30 | 24936.52 | 80 | 20517.82 | 40736.19 | 128 |
| gloas | attester_duties(current_epoch) | 16 | 4656.03 | 15 | 68.35 | 341.77 | 0 | 276.71 | 551.73 | 0 |
| gloas | ptc_duties(current_epoch) | 16 | 4727.13 | 15 | 4852.01 | 24260.04 | 75 | 18903.46 | 37865.39 | 120 |
| gloas | block_production(v4) | 16 | 5379.96 | 16 | 5443.54 | 27217.70 | 80 | 21547.36 | 41672.12 | 128 |
| gloas | states/{slot}/validators(past_head) | 16 | 77.65 | 0 | 73.59 | 367.93 | 0 | 365.63 | 626.40 | 0 |
| gloas | proposer_duties(next_epoch) | 32 | 10015.38 | 32 | 9962.13 | 49810.64 | 160 | 40133.29 | 79472.17 | 256 |
| gloas | attestation_data(current_slot) | 32 | 10159.48 | 32 | 10181.89 | 50909.47 | 160 | 40562.57 | 80772.83 | 256 |
| gloas | attester_duties(current_epoch) | 32 | 9680.87 | 31 | 70.79 | 353.94 | 0 | 275.82 | 563.61 | 0 |
| gloas | ptc_duties(current_epoch) | 32 | 9763.26 | 31 | 9955.94 | 49779.70 | 155 | 39627.09 | 79117.76 | 248 |
| gloas | block_production(v4) | 32 | 10831.77 | 32 | 10816.59 | 54082.93 | 160 | 44409.00 | 87804.68 | 256 |
| gloas | states/{slot}/validators(past_head) | 32 | 103.50 | 0 | 78.37 | 391.87 | 0 | 375.96 | 651.60 | 0 |

### stale-head epoch-by-epoch incremental variant=`baseline-ef55e2fbf` validators=32768
| fork | endpoint | stale_epochs | first_ms | first_pe |
|---|---|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 37.56 | 1 |
| fulu | attestation_data(current_slot) | 1 | 36.71 | 1 |
| fulu | attester_duties(current_epoch) | 1 | 73.72 | 0 |
| fulu | block_production(v3) | 1 | 316.46 | 1 |
| fulu | states/{slot}/validators(past_head) | 1 | 89.65 | 0 |
| fulu | proposer_duties(next_epoch) | 2 | 82.76 | 2 |
| fulu | attestation_data(current_slot) | 2 | 78.91 | 2 |
| fulu | attester_duties(current_epoch) | 2 | 106.28 | 1 |
| fulu | block_production(v3) | 2 | 369.48 | 2 |
| fulu | states/{slot}/validators(past_head) | 2 | 79.77 | 0 |
| fulu | proposer_duties(next_epoch) | 3 | 116.86 | 3 |
| fulu | attestation_data(current_slot) | 3 | 116.36 | 3 |
| fulu | attester_duties(current_epoch) | 3 | 148.90 | 2 |
| fulu | block_production(v3) | 3 | 420.69 | 3 |
| fulu | states/{slot}/validators(past_head) | 3 | 72.38 | 0 |
| fulu | proposer_duties(next_epoch) | 4 | 152.73 | 4 |
| fulu | attestation_data(current_slot) | 4 | 148.56 | 4 |
| fulu | attester_duties(current_epoch) | 4 | 184.21 | 3 |
| fulu | block_production(v3) | 4 | 469.39 | 4 |
| fulu | states/{slot}/validators(past_head) | 4 | 72.25 | 0 |
| fulu | proposer_duties(next_epoch) | 5 | 183.15 | 5 |
| fulu | attestation_data(current_slot) | 5 | 181.08 | 5 |
| fulu | attester_duties(current_epoch) | 5 | 215.31 | 4 |
| fulu | block_production(v3) | 5 | 520.20 | 5 |
| fulu | states/{slot}/validators(past_head) | 5 | 73.16 | 0 |
| fulu | proposer_duties(next_epoch) | 6 | 224.31 | 6 |
| fulu | attestation_data(current_slot) | 6 | 220.81 | 6 |
| fulu | attester_duties(current_epoch) | 6 | 248.91 | 5 |
| fulu | block_production(v3) | 6 | 574.99 | 6 |
| fulu | states/{slot}/validators(past_head) | 6 | 80.59 | 0 |
| fulu | proposer_duties(next_epoch) | 7 | 271.05 | 7 |
| fulu | attestation_data(current_slot) | 7 | 268.06 | 7 |
| fulu | attester_duties(current_epoch) | 7 | 290.43 | 6 |
| fulu | block_production(v3) | 7 | 634.60 | 7 |
| fulu | states/{slot}/validators(past_head) | 7 | 79.03 | 0 |
| fulu | proposer_duties(next_epoch) | 8 | 304.33 | 8 |
| fulu | attestation_data(current_slot) | 8 | 296.32 | 8 |
| fulu | attester_duties(current_epoch) | 8 | 333.13 | 7 |
| fulu | block_production(v3) | 8 | 679.02 | 8 |
| fulu | states/{slot}/validators(past_head) | 8 | 72.95 | 0 |
| fulu | proposer_duties(next_epoch) | 9 | 374.85 | 9 |
| fulu | attestation_data(current_slot) | 9 | 332.48 | 9 |
| fulu | attester_duties(current_epoch) | 9 | 363.12 | 8 |
| fulu | block_production(v3) | 9 | 725.86 | 9 |
| fulu | states/{slot}/validators(past_head) | 9 | 73.94 | 0 |
| fulu | proposer_duties(next_epoch) | 10 | 374.09 | 10 |
| fulu | attestation_data(current_slot) | 10 | 370.29 | 10 |
| fulu | attester_duties(current_epoch) | 10 | 401.60 | 9 |
| fulu | block_production(v3) | 10 | 780.45 | 10 |
| fulu | states/{slot}/validators(past_head) | 10 | 73.16 | 0 |
| fulu | proposer_duties(next_epoch) | 11 | 412.27 | 11 |
| fulu | attestation_data(current_slot) | 11 | 407.19 | 11 |
| fulu | attester_duties(current_epoch) | 11 | 435.85 | 10 |
| fulu | block_production(v3) | 11 | 829.39 | 11 |
| fulu | states/{slot}/validators(past_head) | 11 | 74.96 | 0 |
| fulu | proposer_duties(next_epoch) | 12 | 452.98 | 12 |
| fulu | attestation_data(current_slot) | 12 | 446.05 | 12 |
| fulu | attester_duties(current_epoch) | 12 | 474.46 | 11 |
| fulu | block_production(v3) | 12 | 880.93 | 12 |
| fulu | states/{slot}/validators(past_head) | 12 | 72.09 | 0 |
| fulu | proposer_duties(next_epoch) | 13 | 485.73 | 13 |
| fulu | attestation_data(current_slot) | 13 | 485.87 | 13 |
| fulu | attester_duties(current_epoch) | 13 | 515.38 | 12 |
| fulu | block_production(v3) | 13 | 939.49 | 13 |
| fulu | states/{slot}/validators(past_head) | 13 | 75.14 | 0 |
| fulu | proposer_duties(next_epoch) | 14 | 525.05 | 14 |
| fulu | attestation_data(current_slot) | 14 | 521.51 | 14 |
| fulu | attester_duties(current_epoch) | 14 | 548.03 | 13 |
| fulu | block_production(v3) | 14 | 984.18 | 14 |
| fulu | states/{slot}/validators(past_head) | 14 | 72.92 | 0 |
| fulu | proposer_duties(next_epoch) | 15 | 565.09 | 15 |
| fulu | attestation_data(current_slot) | 15 | 558.78 | 15 |
| fulu | attester_duties(current_epoch) | 15 | 589.55 | 14 |
| fulu | block_production(v3) | 15 | 1048.21 | 15 |
| fulu | states/{slot}/validators(past_head) | 15 | 71.85 | 0 |
| fulu | proposer_duties(next_epoch) | 16 | 597.04 | 16 |
| fulu | attestation_data(current_slot) | 16 | 593.75 | 16 |
| fulu | attester_duties(current_epoch) | 16 | 625.26 | 15 |
| fulu | block_production(v3) | 16 | 1100.02 | 16 |
| fulu | states/{slot}/validators(past_head) | 16 | 73.11 | 0 |
| fulu | proposer_duties(next_epoch) | 17 | 643.94 | 17 |
| fulu | attestation_data(current_slot) | 17 | 642.11 | 17 |
| fulu | attester_duties(current_epoch) | 17 | 661.62 | 16 |
| fulu | block_production(v3) | 17 | 1154.91 | 17 |
| fulu | states/{slot}/validators(past_head) | 17 | 73.11 | 0 |
| fulu | proposer_duties(next_epoch) | 18 | 674.95 | 18 |
| fulu | attestation_data(current_slot) | 18 | 670.92 | 18 |
| fulu | attester_duties(current_epoch) | 18 | 705.01 | 17 |
| fulu | block_production(v3) | 18 | 1196.41 | 18 |
| fulu | states/{slot}/validators(past_head) | 18 | 73.24 | 0 |
| fulu | proposer_duties(next_epoch) | 19 | 707.13 | 19 |
| fulu | attestation_data(current_slot) | 19 | 706.09 | 19 |
| fulu | attester_duties(current_epoch) | 19 | 736.00 | 18 |
| fulu | block_production(v3) | 19 | 1250.64 | 19 |
| fulu | states/{slot}/validators(past_head) | 19 | 72.98 | 0 |
| fulu | proposer_duties(next_epoch) | 20 | 743.53 | 20 |
| fulu | attestation_data(current_slot) | 20 | 740.86 | 20 |
| fulu | attester_duties(current_epoch) | 20 | 773.05 | 19 |
| fulu | block_production(v3) | 20 | 1295.37 | 20 |
| fulu | states/{slot}/validators(past_head) | 20 | 71.97 | 0 |
| fulu | proposer_duties(next_epoch) | 21 | 774.58 | 21 |
| fulu | attestation_data(current_slot) | 21 | 767.19 | 21 |
| fulu | attester_duties(current_epoch) | 21 | 803.59 | 20 |
| fulu | block_production(v3) | 21 | 1333.32 | 21 |
| fulu | states/{slot}/validators(past_head) | 21 | 73.19 | 0 |
| fulu | proposer_duties(next_epoch) | 22 | 801.69 | 22 |
| fulu | attestation_data(current_slot) | 22 | 802.46 | 22 |
| fulu | attester_duties(current_epoch) | 22 | 829.91 | 21 |
| fulu | block_production(v3) | 22 | 1378.30 | 22 |
| fulu | states/{slot}/validators(past_head) | 22 | 71.94 | 0 |
| fulu | proposer_duties(next_epoch) | 23 | 829.12 | 23 |
| fulu | attestation_data(current_slot) | 23 | 825.02 | 23 |
| fulu | attester_duties(current_epoch) | 23 | 870.94 | 22 |
| fulu | block_production(v3) | 23 | 1430.58 | 23 |
| fulu | states/{slot}/validators(past_head) | 23 | 71.39 | 0 |
| fulu | proposer_duties(next_epoch) | 24 | 868.70 | 24 |
| fulu | attestation_data(current_slot) | 24 | 868.92 | 24 |
| fulu | attester_duties(current_epoch) | 24 | 896.53 | 23 |
| fulu | block_production(v3) | 24 | 1480.62 | 24 |
| fulu | states/{slot}/validators(past_head) | 24 | 73.53 | 0 |
| fulu | proposer_duties(next_epoch) | 25 | 904.19 | 25 |
| fulu | attestation_data(current_slot) | 25 | 903.43 | 25 |
| fulu | attester_duties(current_epoch) | 25 | 929.44 | 24 |
| fulu | block_production(v3) | 25 | 1526.86 | 25 |
| fulu | states/{slot}/validators(past_head) | 25 | 72.56 | 0 |
| fulu | proposer_duties(next_epoch) | 26 | 945.93 | 26 |
| fulu | attestation_data(current_slot) | 26 | 942.83 | 26 |
| fulu | attester_duties(current_epoch) | 26 | 963.29 | 25 |
| fulu | block_production(v3) | 26 | 1579.43 | 26 |
| fulu | states/{slot}/validators(past_head) | 26 | 74.68 | 0 |
| fulu | proposer_duties(next_epoch) | 27 | 979.32 | 27 |
| fulu | attestation_data(current_slot) | 27 | 982.14 | 27 |
| fulu | attester_duties(current_epoch) | 27 | 1001.97 | 26 |
| fulu | block_production(v3) | 27 | 1641.52 | 27 |
| fulu | states/{slot}/validators(past_head) | 27 | 73.10 | 0 |
| fulu | proposer_duties(next_epoch) | 28 | 1024.38 | 28 |
| fulu | attestation_data(current_slot) | 28 | 1023.46 | 28 |
| fulu | attester_duties(current_epoch) | 28 | 1037.70 | 27 |
| fulu | block_production(v3) | 28 | 1690.22 | 28 |
| fulu | states/{slot}/validators(past_head) | 28 | 72.56 | 0 |
| fulu | proposer_duties(next_epoch) | 29 | 1069.26 | 29 |
| fulu | attestation_data(current_slot) | 29 | 1064.45 | 29 |
| fulu | attester_duties(current_epoch) | 29 | 1081.83 | 28 |
| fulu | block_production(v3) | 29 | 1779.98 | 29 |
| fulu | states/{slot}/validators(past_head) | 29 | 73.78 | 0 |
| fulu | proposer_duties(next_epoch) | 30 | 1115.63 | 30 |
| fulu | attestation_data(current_slot) | 30 | 1112.84 | 30 |
| fulu | attester_duties(current_epoch) | 30 | 1121.27 | 29 |
| fulu | block_production(v3) | 30 | 1813.42 | 30 |
| fulu | states/{slot}/validators(past_head) | 30 | 72.38 | 0 |
| fulu | proposer_duties(next_epoch) | 31 | 1149.07 | 31 |
| fulu | attestation_data(current_slot) | 31 | 1143.35 | 31 |
| fulu | attester_duties(current_epoch) | 31 | 1174.56 | 30 |
| fulu | block_production(v3) | 31 | 1929.23 | 31 |
| fulu | states/{slot}/validators(past_head) | 31 | 71.72 | 0 |
| fulu | proposer_duties(next_epoch) | 32 | 1191.74 | 32 |
| fulu | attestation_data(current_slot) | 32 | 1283.31 | 32 |
| fulu | attester_duties(current_epoch) | 32 | 1214.68 | 31 |
| fulu | block_production(v3) | 32 | 1907.77 | 32 |
| fulu | states/{slot}/validators(past_head) | 32 | 73.42 | 0 |
| gloas | proposer_duties(next_epoch) | 1 | 412.59 | 1 |
| gloas | attestation_data(current_slot) | 1 | 308.95 | 1 |
| gloas | attester_duties(current_epoch) | 1 | 75.03 | 0 |
| gloas | ptc_duties(current_epoch) | 1 | 157.66 | 0 |
| gloas | block_production(v4) | 1 | 586.37 | 1 |
| gloas | states/{slot}/validators(past_head) | 1 | 83.71 | 0 |
| gloas | proposer_duties(next_epoch) | 2 | 604.12 | 2 |
| gloas | attestation_data(current_slot) | 2 | 608.67 | 2 |
| gloas | attester_duties(current_epoch) | 2 | 366.11 | 1 |
| gloas | ptc_duties(current_epoch) | 2 | 447.66 | 1 |
| gloas | block_production(v4) | 2 | 896.00 | 2 |
| gloas | states/{slot}/validators(past_head) | 2 | 73.48 | 0 |
| gloas | proposer_duties(next_epoch) | 3 | 913.31 | 3 |
| gloas | attestation_data(current_slot) | 3 | 931.71 | 3 |
| gloas | attester_duties(current_epoch) | 3 | 667.43 | 2 |
| gloas | ptc_duties(current_epoch) | 3 | 753.92 | 2 |
| gloas | block_production(v4) | 3 | 1217.37 | 3 |
| gloas | states/{slot}/validators(past_head) | 3 | 79.45 | 0 |
| gloas | proposer_duties(next_epoch) | 4 | 1265.92 | 4 |
| gloas | attestation_data(current_slot) | 4 | 1244.20 | 4 |
| gloas | attester_duties(current_epoch) | 4 | 979.72 | 3 |
| gloas | ptc_duties(current_epoch) | 4 | 1054.19 | 3 |
| gloas | block_production(v4) | 4 | 1553.51 | 4 |
| gloas | states/{slot}/validators(past_head) | 4 | 71.96 | 0 |
| gloas | proposer_duties(next_epoch) | 5 | 1538.64 | 5 |
| gloas | attestation_data(current_slot) | 5 | 1570.47 | 5 |
| gloas | attester_duties(current_epoch) | 5 | 1286.18 | 4 |
| gloas | ptc_duties(current_epoch) | 5 | 1379.85 | 4 |
| gloas | block_production(v4) | 5 | 1871.14 | 5 |
| gloas | states/{slot}/validators(past_head) | 5 | 72.71 | 0 |
| gloas | proposer_duties(next_epoch) | 6 | 1847.88 | 6 |
| gloas | attestation_data(current_slot) | 6 | 1915.33 | 6 |
| gloas | attester_duties(current_epoch) | 6 | 1620.26 | 5 |
| gloas | ptc_duties(current_epoch) | 6 | 1689.47 | 5 |
| gloas | block_production(v4) | 6 | 2213.42 | 6 |
| gloas | states/{slot}/validators(past_head) | 6 | 72.37 | 0 |
| gloas | proposer_duties(next_epoch) | 7 | 2173.93 | 7 |
| gloas | attestation_data(current_slot) | 7 | 2254.31 | 7 |
| gloas | attester_duties(current_epoch) | 7 | 1945.12 | 6 |
| gloas | ptc_duties(current_epoch) | 7 | 2045.23 | 6 |
| gloas | block_production(v4) | 7 | 2563.85 | 7 |
| gloas | states/{slot}/validators(past_head) | 7 | 96.58 | 0 |
| gloas | proposer_duties(next_epoch) | 8 | 2622.61 | 8 |
| gloas | attestation_data(current_slot) | 8 | 2547.39 | 8 |
| gloas | attester_duties(current_epoch) | 8 | 2238.20 | 7 |
| gloas | ptc_duties(current_epoch) | 8 | 2338.70 | 7 |
| gloas | block_production(v4) | 8 | 2955.44 | 8 |
| gloas | states/{slot}/validators(past_head) | 8 | 74.03 | 0 |
| gloas | proposer_duties(next_epoch) | 9 | 2830.49 | 9 |
| gloas | attestation_data(current_slot) | 9 | 2839.34 | 9 |
| gloas | attester_duties(current_epoch) | 9 | 2605.09 | 8 |
| gloas | ptc_duties(current_epoch) | 9 | 2678.79 | 8 |
| gloas | block_production(v4) | 9 | 3237.41 | 9 |
| gloas | states/{slot}/validators(past_head) | 9 | 73.16 | 0 |
| gloas | proposer_duties(next_epoch) | 10 | 3144.72 | 10 |
| gloas | attestation_data(current_slot) | 10 | 3205.10 | 10 |
| gloas | attester_duties(current_epoch) | 10 | 2865.78 | 9 |
| gloas | ptc_duties(current_epoch) | 10 | 2967.66 | 9 |
| gloas | block_production(v4) | 10 | 3500.08 | 10 |
| gloas | states/{slot}/validators(past_head) | 10 | 73.38 | 0 |
| gloas | proposer_duties(next_epoch) | 11 | 3474.38 | 11 |
| gloas | attestation_data(current_slot) | 11 | 3540.36 | 11 |
| gloas | attester_duties(current_epoch) | 11 | 3226.77 | 10 |
| gloas | ptc_duties(current_epoch) | 11 | 3328.76 | 10 |
| gloas | block_production(v4) | 11 | 3825.96 | 11 |
| gloas | states/{slot}/validators(past_head) | 11 | 73.75 | 0 |
| gloas | proposer_duties(next_epoch) | 12 | 3763.53 | 12 |
| gloas | attestation_data(current_slot) | 12 | 3786.49 | 12 |
| gloas | attester_duties(current_epoch) | 12 | 3482.74 | 11 |
| gloas | ptc_duties(current_epoch) | 12 | 3659.72 | 11 |
| gloas | block_production(v4) | 12 | 4159.97 | 12 |
| gloas | states/{slot}/validators(past_head) | 12 | 73.17 | 0 |
| gloas | proposer_duties(next_epoch) | 13 | 4096.74 | 13 |
| gloas | attestation_data(current_slot) | 13 | 4175.31 | 13 |
| gloas | attester_duties(current_epoch) | 13 | 3816.49 | 12 |
| gloas | ptc_duties(current_epoch) | 13 | 3903.34 | 12 |
| gloas | block_production(v4) | 13 | 4481.51 | 13 |
| gloas | states/{slot}/validators(past_head) | 13 | 71.27 | 0 |
| gloas | proposer_duties(next_epoch) | 14 | 4408.61 | 14 |
| gloas | attestation_data(current_slot) | 14 | 4546.12 | 14 |
| gloas | attester_duties(current_epoch) | 14 | 4129.06 | 13 |
| gloas | ptc_duties(current_epoch) | 14 | 4200.73 | 13 |
| gloas | block_production(v4) | 14 | 4781.66 | 14 |
| gloas | states/{slot}/validators(past_head) | 14 | 71.39 | 0 |
| gloas | proposer_duties(next_epoch) | 15 | 4750.63 | 15 |
| gloas | attestation_data(current_slot) | 15 | 4828.32 | 15 |
| gloas | attester_duties(current_epoch) | 15 | 4432.12 | 14 |
| gloas | ptc_duties(current_epoch) | 15 | 4642.10 | 14 |
| gloas | block_production(v4) | 15 | 5121.74 | 15 |
| gloas | states/{slot}/validators(past_head) | 15 | 72.56 | 0 |
| gloas | proposer_duties(next_epoch) | 16 | 5009.92 | 16 |
| gloas | attestation_data(current_slot) | 16 | 5077.91 | 16 |
| gloas | attester_duties(current_epoch) | 16 | 4805.14 | 15 |
| gloas | ptc_duties(current_epoch) | 16 | 4859.41 | 15 |
| gloas | block_production(v4) | 16 | 5457.26 | 16 |
| gloas | states/{slot}/validators(past_head) | 16 | 75.70 | 0 |
| gloas | proposer_duties(next_epoch) | 17 | 5482.97 | 17 |
| gloas | attestation_data(current_slot) | 17 | 5500.82 | 17 |
| gloas | attester_duties(current_epoch) | 17 | 5138.63 | 16 |
| gloas | ptc_duties(current_epoch) | 17 | 5168.89 | 16 |
| gloas | block_production(v4) | 17 | 5775.53 | 17 |
| gloas | states/{slot}/validators(past_head) | 17 | 74.58 | 0 |
| gloas | proposer_duties(next_epoch) | 18 | 5728.36 | 18 |
| gloas | attestation_data(current_slot) | 18 | 5741.39 | 18 |
| gloas | attester_duties(current_epoch) | 18 | 5429.03 | 17 |
| gloas | ptc_duties(current_epoch) | 18 | 5513.02 | 17 |
| gloas | block_production(v4) | 18 | 6100.77 | 18 |
| gloas | states/{slot}/validators(past_head) | 18 | 72.95 | 0 |
| gloas | proposer_duties(next_epoch) | 19 | 5995.33 | 19 |
| gloas | attestation_data(current_slot) | 19 | 6082.73 | 19 |
| gloas | attester_duties(current_epoch) | 19 | 5864.60 | 18 |
| gloas | ptc_duties(current_epoch) | 19 | 5796.20 | 18 |
| gloas | block_production(v4) | 19 | 6386.78 | 19 |
| gloas | states/{slot}/validators(past_head) | 19 | 74.00 | 0 |
| gloas | proposer_duties(next_epoch) | 20 | 6362.74 | 20 |
| gloas | attestation_data(current_slot) | 20 | 6551.42 | 20 |
| gloas | attester_duties(current_epoch) | 20 | 6129.35 | 19 |
| gloas | ptc_duties(current_epoch) | 20 | 6106.24 | 19 |
| gloas | block_production(v4) | 20 | 6745.37 | 20 |
| gloas | states/{slot}/validators(past_head) | 20 | 75.46 | 0 |
| gloas | proposer_duties(next_epoch) | 21 | 6716.13 | 21 |
| gloas | attestation_data(current_slot) | 21 | 6769.85 | 21 |
| gloas | attester_duties(current_epoch) | 21 | 6367.79 | 20 |
| gloas | ptc_duties(current_epoch) | 21 | 6685.80 | 20 |
| gloas | block_production(v4) | 21 | 7061.32 | 21 |
| gloas | states/{slot}/validators(past_head) | 21 | 75.04 | 0 |
| gloas | proposer_duties(next_epoch) | 22 | 7058.96 | 22 |
| gloas | attestation_data(current_slot) | 22 | 7026.63 | 22 |
| gloas | attester_duties(current_epoch) | 22 | 6682.93 | 21 |
| gloas | ptc_duties(current_epoch) | 22 | 6777.28 | 21 |
| gloas | block_production(v4) | 22 | 7393.31 | 22 |
| gloas | states/{slot}/validators(past_head) | 22 | 72.75 | 0 |
| gloas | proposer_duties(next_epoch) | 23 | 7245.56 | 23 |
| gloas | attestation_data(current_slot) | 23 | 7552.22 | 23 |
| gloas | attester_duties(current_epoch) | 23 | 6958.27 | 22 |
| gloas | ptc_duties(current_epoch) | 23 | 7106.53 | 22 |
| gloas | block_production(v4) | 23 | 7692.96 | 23 |
| gloas | states/{slot}/validators(past_head) | 23 | 73.87 | 0 |
| gloas | proposer_duties(next_epoch) | 24 | 7741.50 | 24 |
| gloas | attestation_data(current_slot) | 24 | 7819.04 | 24 |
| gloas | attester_duties(current_epoch) | 24 | 7290.07 | 23 |
| gloas | ptc_duties(current_epoch) | 24 | 7539.05 | 23 |
| gloas | block_production(v4) | 24 | 8024.92 | 24 |
| gloas | states/{slot}/validators(past_head) | 24 | 73.22 | 0 |
| gloas | proposer_duties(next_epoch) | 25 | 7919.41 | 25 |
| gloas | attestation_data(current_slot) | 25 | 8015.03 | 25 |
| gloas | attester_duties(current_epoch) | 25 | 7712.15 | 24 |
| gloas | ptc_duties(current_epoch) | 25 | 7762.46 | 24 |
| gloas | block_production(v4) | 25 | 8354.88 | 25 |
| gloas | states/{slot}/validators(past_head) | 25 | 73.85 | 0 |
| gloas | proposer_duties(next_epoch) | 26 | 8230.93 | 26 |
| gloas | attestation_data(current_slot) | 26 | 8325.83 | 26 |
| gloas | attester_duties(current_epoch) | 26 | 7913.97 | 25 |
| gloas | ptc_duties(current_epoch) | 26 | 8135.81 | 25 |
| gloas | block_production(v4) | 26 | 8641.16 | 26 |
| gloas | states/{slot}/validators(past_head) | 26 | 72.93 | 0 |
| gloas | proposer_duties(next_epoch) | 27 | 8709.52 | 27 |
| gloas | attestation_data(current_slot) | 27 | 8976.67 | 27 |
| gloas | attester_duties(current_epoch) | 27 | 8327.16 | 26 |
| gloas | ptc_duties(current_epoch) | 27 | 8257.01 | 26 |
| gloas | block_production(v4) | 27 | 8975.51 | 27 |
| gloas | states/{slot}/validators(past_head) | 27 | 73.69 | 0 |
| gloas | proposer_duties(next_epoch) | 28 | 8826.94 | 28 |
| gloas | attestation_data(current_slot) | 28 | 9137.24 | 28 |
| gloas | attester_duties(current_epoch) | 28 | 8725.72 | 27 |
| gloas | ptc_duties(current_epoch) | 28 | 8686.87 | 27 |
| gloas | block_production(v4) | 28 | 9292.62 | 28 |
| gloas | states/{slot}/validators(past_head) | 28 | 73.99 | 0 |
| gloas | proposer_duties(next_epoch) | 29 | 9208.03 | 29 |
| gloas | attestation_data(current_slot) | 29 | 9531.47 | 29 |
| gloas | attester_duties(current_epoch) | 29 | 8956.38 | 28 |
| gloas | ptc_duties(current_epoch) | 29 | 8971.14 | 28 |
| gloas | block_production(v4) | 29 | 9633.47 | 29 |
| gloas | states/{slot}/validators(past_head) | 29 | 72.49 | 0 |
| gloas | proposer_duties(next_epoch) | 30 | 9669.97 | 30 |
| gloas | attestation_data(current_slot) | 30 | 9742.36 | 30 |
| gloas | attester_duties(current_epoch) | 30 | 9286.66 | 29 |
| gloas | ptc_duties(current_epoch) | 30 | 9279.93 | 29 |
| gloas | block_production(v4) | 30 | 9966.65 | 30 |
| gloas | states/{slot}/validators(past_head) | 30 | 72.84 | 0 |
| gloas | proposer_duties(next_epoch) | 31 | 9888.83 | 31 |
| gloas | attestation_data(current_slot) | 31 | 10006.62 | 31 |
| gloas | attester_duties(current_epoch) | 31 | 10522.24 | 30 |
| gloas | ptc_duties(current_epoch) | 31 | 12139.56 | 30 |
| gloas | block_production(v4) | 31 | 12004.92 | 31 |
| gloas | states/{slot}/validators(past_head) | 31 | 86.70 | 0 |
| gloas | proposer_duties(next_epoch) | 32 | 11918.28 | 32 |
| gloas | attestation_data(current_slot) | 32 | 12020.29 | 32 |
| gloas | attester_duties(current_epoch) | 32 | 10875.21 | 31 |
| gloas | ptc_duties(current_epoch) | 32 | 10141.26 | 31 |
| gloas | block_production(v4) | 32 | 11220.09 | 32 |
| gloas | states/{slot}/validators(past_head) | 32 | 79.36 | 0 |

---

## Variant B — fix (`c4a7b63` + harness)

### stale-head benchmark variant=`advanced-state-c4a7b63` validators=32768
| fork | endpoint | stale_epochs | first_ms | first_pe | repeat_mean_ms (n=5) | repeat_total_ms | repeat_pe_total | concurrent_wall_ms (n=8) | concurrent_cpu_ms | concurrent_pe |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 38.34 | 1 | 0.78 | 3.90 | 0 | 1.64 | 3.38 | 0 |
| fulu | attestation_data(current_slot) | 1 | 0.50 | 0 | 0.35 | 1.74 | 0 | 0.78 | 2.14 | 0 |
| fulu | attester_duties(current_epoch) | 1 | 79.68 | 0 | 73.65 | 368.23 | 0 | 292.79 | 572.78 | 0 |
| fulu | block_production(v3) | 1 | 315.79 | 1 | 260.26 | 1301.31 | 0 | 1038.48 | 74.00 | 0 |
| fulu | states/{slot}/validators(past_head) | 1 | 85.81 | 0 | 71.28 | 356.42 | 0 | 359.36 | 600.00 | 0 |
| fulu | proposer_duties(next_epoch) | 2 | 50.23 | 1 | 0.97 | 4.87 | 0 | 1.50 | 3.52 | 0 |
| fulu | attestation_data(current_slot) | 2 | 0.76 | 0 | 0.45 | 2.26 | 0 | 0.73 | 2.01 | 0 |
| fulu | attester_duties(current_epoch) | 2 | 70.78 | 0 | 73.90 | 369.48 | 0 | 285.80 | 558.42 | 0 |
| fulu | block_production(v3) | 2 | 325.47 | 1 | 262.14 | 1310.68 | 0 | 1042.89 | 79.96 | 0 |
| fulu | states/{slot}/validators(past_head) | 2 | 77.99 | 0 | 71.38 | 356.88 | 0 | 366.77 | 605.59 | 0 |
| fulu | proposer_duties(next_epoch) | 4 | 81.83 | 2 | 0.90 | 4.52 | 0 | 2.16 | 4.30 | 0 |
| fulu | attestation_data(current_slot) | 4 | 0.97 | 0 | 0.45 | 2.24 | 0 | 0.88 | 2.39 | 0 |
| fulu | attester_duties(current_epoch) | 4 | 74.96 | 0 | 73.90 | 369.51 | 0 | 291.58 | 569.30 | 0 |
| fulu | block_production(v3) | 4 | 369.45 | 2 | 262.19 | 1310.95 | 0 | 1039.95 | 77.41 | 0 |
| fulu | states/{slot}/validators(past_head) | 4 | 83.79 | 0 | 70.80 | 353.98 | 0 | 367.77 | 608.84 | 0 |
| fulu | proposer_duties(next_epoch) | 8 | 163.01 | 4 | 0.82 | 4.12 | 0 | 0.99 | 2.60 | 0 |
| fulu | attestation_data(current_slot) | 8 | 0.46 | 0 | 0.43 | 2.14 | 0 | 0.85 | 2.35 | 0 |
| fulu | attester_duties(current_epoch) | 8 | 70.76 | 0 | 73.46 | 367.31 | 0 | 286.01 | 560.16 | 0 |
| fulu | block_production(v3) | 8 | 481.31 | 4 | 261.95 | 1309.76 | 0 | 1042.12 | 79.64 | 0 |
| fulu | states/{slot}/validators(past_head) | 8 | 82.45 | 0 | 72.38 | 361.89 | 0 | 366.43 | 590.54 | 0 |
| fulu | proposer_duties(next_epoch) | 16 | 304.13 | 8 | 0.94 | 4.72 | 0 | 1.82 | 3.60 | 0 |
| fulu | attestation_data(current_slot) | 16 | 0.77 | 0 | 0.47 | 2.37 | 0 | 0.75 | 2.16 | 0 |
| fulu | attester_duties(current_epoch) | 16 | 71.10 | 0 | 72.06 | 360.30 | 0 | 285.83 | 555.84 | 0 |
| fulu | block_production(v3) | 16 | 683.14 | 8 | 265.46 | 1327.30 | 0 | 1050.50 | 103.79 | 0 |
| fulu | states/{slot}/validators(past_head) | 16 | 77.74 | 0 | 70.20 | 350.98 | 0 | 360.19 | 588.12 | 0 |
| fulu | proposer_duties(next_epoch) | 32 | 600.04 | 16 | 1.03 | 5.15 | 0 | 1.89 | 2.74 | 0 |
| fulu | attestation_data(current_slot) | 32 | 0.37 | 0 | 0.56 | 2.78 | 0 | 1.24 | 3.01 | 0 |
| fulu | attester_duties(current_epoch) | 32 | 70.32 | 0 | 72.88 | 364.41 | 0 | 285.79 | 553.33 | 0 |
| fulu | block_production(v3) | 32 | 1095.05 | 16 | 265.26 | 1326.28 | 0 | 1052.24 | 107.38 | 0 |
| fulu | states/{slot}/validators(past_head) | 32 | 76.96 | 0 | 69.58 | 347.90 | 0 | 362.82 | 594.09 | 0 |
| gloas | proposer_duties(next_epoch) | 1 | 300.44 | 1 | 0.66 | 3.30 | 0 | 1.49 | 3.42 | 0 |
| gloas | attestation_data(current_slot) | 1 | 0.53 | 0 | 0.35 | 1.77 | 0 | 0.90 | 2.38 | 0 |
| gloas | attester_duties(current_epoch) | 1 | 72.54 | 0 | 67.91 | 339.54 | 0 | 283.08 | 571.82 | 0 |
| gloas | ptc_duties(current_epoch) | 1 | 167.03 | 0 | 159.26 | 796.30 | 0 | 630.87 | 1289.37 | 0 |
| gloas | block_production(v4) | 1 | 579.24 | 1 | 261.23 | 1306.14 | 0 | 1037.99 | 70.82 | 0 |
| gloas | states/{slot}/validators(past_head) | 1 | 74.68 | 0 | 70.18 | 350.88 | 0 | 370.82 | 611.36 | 0 |
| gloas | proposer_duties(next_epoch) | 2 | 310.47 | 1 | 0.96 | 4.82 | 0 | 1.06 | 2.94 | 0 |
| gloas | attestation_data(current_slot) | 2 | 0.40 | 0 | 0.34 | 1.71 | 0 | 1.10 | 2.73 | 0 |
| gloas | attester_duties(current_epoch) | 2 | 73.48 | 0 | 69.87 | 349.37 | 0 | 282.03 | 547.27 | 0 |
| gloas | ptc_duties(current_epoch) | 2 | 162.23 | 0 | 156.55 | 782.77 | 0 | 599.91 | 1257.73 | 0 |
| gloas | block_production(v4) | 2 | 598.62 | 1 | 261.15 | 1305.77 | 0 | 1036.05 | 67.22 | 0 |
| gloas | states/{slot}/validators(past_head) | 2 | 73.83 | 0 | 71.13 | 355.66 | 0 | 357.48 | 595.13 | 0 |
| gloas | proposer_duties(next_epoch) | 4 | 626.66 | 2 | 1.00 | 4.98 | 0 | 1.42 | 3.95 | 0 |
| gloas | attestation_data(current_slot) | 4 | 0.55 | 0 | 0.38 | 1.88 | 0 | 0.98 | 2.43 | 0 |
| gloas | attester_duties(current_epoch) | 4 | 66.52 | 0 | 67.31 | 336.53 | 0 | 276.27 | 550.28 | 0 |
| gloas | ptc_duties(current_epoch) | 4 | 477.84 | 1 | 470.09 | 2350.45 | 5 | 1843.03 | 2484.61 | 4 |
| gloas | block_production(v4) | 4 | 921.99 | 2 | 260.73 | 1303.64 | 0 | 1038.05 | 69.51 | 0 |
| gloas | states/{slot}/validators(past_head) | 4 | 79.65 | 0 | 71.58 | 357.89 | 0 | 366.02 | 601.97 | 0 |
| gloas | proposer_duties(next_epoch) | 8 | 1224.68 | 4 | 1.00 | 4.99 | 0 | 1.19 | 3.02 | 0 |
| gloas | attestation_data(current_slot) | 8 | 0.40 | 0 | 0.45 | 2.26 | 0 | 0.96 | 2.33 | 0 |
| gloas | attester_duties(current_epoch) | 8 | 67.89 | 0 | 68.47 | 342.37 | 0 | 279.92 | 548.70 | 0 |
| gloas | ptc_duties(current_epoch) | 8 | 1073.18 | 3 | 1067.87 | 5339.34 | 15 | 4237.49 | 4866.91 | 12 |
| gloas | block_production(v4) | 8 | 1554.94 | 4 | 260.64 | 1303.19 | 0 | 1037.28 | 69.16 | 0 |
| gloas | states/{slot}/validators(past_head) | 8 | 74.34 | 0 | 71.83 | 359.15 | 0 | 358.82 | 604.52 | 0 |
| gloas | proposer_duties(next_epoch) | 16 | 2566.66 | 8 | 1.07 | 5.33 | 0 | 1.43 | 3.55 | 0 |
| gloas | attestation_data(current_slot) | 16 | 0.39 | 0 | 0.47 | 2.37 | 0 | 0.95 | 2.70 | 0 |
| gloas | attester_duties(current_epoch) | 16 | 67.93 | 0 | 67.57 | 337.83 | 0 | 283.31 | 655.47 | 0 |
| gloas | ptc_duties(current_epoch) | 16 | 2320.39 | 7 | 2326.71 | 11633.56 | 35 | 9351.17 | 10011.06 | 28 |
| gloas | block_production(v4) | 16 | 2988.02 | 8 | 263.86 | 1319.29 | 0 | 1046.17 | 87.40 | 0 |
| gloas | states/{slot}/validators(past_head) | 16 | 74.40 | 0 | 71.65 | 358.24 | 0 | 364.38 | 603.42 | 0 |
| gloas | proposer_duties(next_epoch) | 32 | 4987.20 | 16 | 0.96 | 4.78 | 0 | 1.93 | 4.30 | 0 |
| gloas | attestation_data(current_slot) | 32 | 0.42 | 0 | 0.38 | 1.92 | 0 | 0.99 | 2.45 | 0 |
| gloas | attester_duties(current_epoch) | 32 | 68.58 | 0 | 67.64 | 338.22 | 0 | 280.43 | 555.05 | 0 |
| gloas | ptc_duties(current_epoch) | 32 | 4788.94 | 15 | 4960.58 | 24802.89 | 75 | 19237.63 | 19863.80 | 60 |
| gloas | block_production(v4) | 32 | 5405.11 | 16 | 265.58 | 1327.90 | 0 | 1052.04 | 98.36 | 0 |
| gloas | states/{slot}/validators(past_head) | 32 | 77.56 | 0 | 71.60 | 357.98 | 0 | 379.40 | 623.43 | 0 |

### stale-head epoch-by-epoch incremental variant=`advanced-state-c4a7b63` validators=32768
| fork | endpoint | stale_epochs | first_ms | first_pe |
|---|---|---:|---:|---:|
| fulu | proposer_duties(next_epoch) | 1 | 38.85 | 1 |
| fulu | attestation_data(current_slot) | 1 | 0.91 | 0 |
| fulu | attester_duties(current_epoch) | 1 | 73.87 | 0 |
| fulu | block_production(v3) | 1 | 314.55 | 1 |
| fulu | states/{slot}/validators(past_head) | 1 | 77.34 | 0 |
| fulu | proposer_duties(next_epoch) | 2 | 47.59 | 1 |
| fulu | attestation_data(current_slot) | 2 | 0.82 | 0 |
| fulu | attester_duties(current_epoch) | 2 | 69.28 | 0 |
| fulu | block_production(v3) | 2 | 321.02 | 1 |
| fulu | states/{slot}/validators(past_head) | 2 | 71.50 | 0 |
| fulu | proposer_duties(next_epoch) | 3 | 39.99 | 1 |
| fulu | attestation_data(current_slot) | 3 | 0.94 | 0 |
| fulu | attester_duties(current_epoch) | 3 | 68.22 | 0 |
| fulu | block_production(v3) | 3 | 316.80 | 1 |
| fulu | states/{slot}/validators(past_head) | 3 | 71.58 | 0 |
| fulu | proposer_duties(next_epoch) | 4 | 37.20 | 1 |
| fulu | attestation_data(current_slot) | 4 | 0.78 | 0 |
| fulu | attester_duties(current_epoch) | 4 | 68.67 | 0 |
| fulu | block_production(v3) | 4 | 312.85 | 1 |
| fulu | states/{slot}/validators(past_head) | 4 | 77.34 | 0 |
| fulu | proposer_duties(next_epoch) | 5 | 36.12 | 1 |
| fulu | attestation_data(current_slot) | 5 | 0.88 | 0 |
| fulu | attester_duties(current_epoch) | 5 | 67.12 | 0 |
| fulu | block_production(v3) | 5 | 317.53 | 1 |
| fulu | states/{slot}/validators(past_head) | 5 | 71.45 | 0 |
| fulu | proposer_duties(next_epoch) | 6 | 44.43 | 1 |
| fulu | attestation_data(current_slot) | 6 | 0.63 | 0 |
| fulu | attester_duties(current_epoch) | 6 | 66.96 | 0 |
| fulu | block_production(v3) | 6 | 324.50 | 1 |
| fulu | states/{slot}/validators(past_head) | 6 | 68.81 | 0 |
| fulu | proposer_duties(next_epoch) | 7 | 49.46 | 1 |
| fulu | attestation_data(current_slot) | 7 | 1.19 | 0 |
| fulu | attester_duties(current_epoch) | 7 | 68.51 | 0 |
| fulu | block_production(v3) | 7 | 325.90 | 1 |
| fulu | states/{slot}/validators(past_head) | 7 | 68.31 | 0 |
| fulu | proposer_duties(next_epoch) | 8 | 35.69 | 1 |
| fulu | attestation_data(current_slot) | 8 | 0.52 | 0 |
| fulu | attester_duties(current_epoch) | 8 | 67.39 | 0 |
| fulu | block_production(v3) | 8 | 317.44 | 1 |
| fulu | states/{slot}/validators(past_head) | 8 | 69.27 | 0 |
| fulu | proposer_duties(next_epoch) | 9 | 39.37 | 1 |
| fulu | attestation_data(current_slot) | 9 | 0.57 | 0 |
| fulu | attester_duties(current_epoch) | 9 | 67.73 | 0 |
| fulu | block_production(v3) | 9 | 315.20 | 1 |
| fulu | states/{slot}/validators(past_head) | 9 | 71.85 | 0 |
| fulu | proposer_duties(next_epoch) | 10 | 39.30 | 1 |
| fulu | attestation_data(current_slot) | 10 | 1.14 | 0 |
| fulu | attester_duties(current_epoch) | 10 | 68.37 | 0 |
| fulu | block_production(v3) | 10 | 315.73 | 1 |
| fulu | states/{slot}/validators(past_head) | 10 | 68.92 | 0 |
| fulu | proposer_duties(next_epoch) | 11 | 41.65 | 1 |
| fulu | attestation_data(current_slot) | 11 | 0.62 | 0 |
| fulu | attester_duties(current_epoch) | 11 | 67.97 | 0 |
| fulu | block_production(v3) | 11 | 318.68 | 1 |
| fulu | states/{slot}/validators(past_head) | 11 | 68.79 | 0 |
| fulu | proposer_duties(next_epoch) | 12 | 44.70 | 1 |
| fulu | attestation_data(current_slot) | 12 | 1.02 | 0 |
| fulu | attester_duties(current_epoch) | 12 | 68.42 | 0 |
| fulu | block_production(v3) | 12 | 319.35 | 1 |
| fulu | states/{slot}/validators(past_head) | 12 | 70.23 | 0 |
| fulu | proposer_duties(next_epoch) | 13 | 41.81 | 1 |
| fulu | attestation_data(current_slot) | 13 | 0.63 | 0 |
| fulu | attester_duties(current_epoch) | 13 | 67.79 | 0 |
| fulu | block_production(v3) | 13 | 321.79 | 1 |
| fulu | states/{slot}/validators(past_head) | 13 | 70.01 | 0 |
| fulu | proposer_duties(next_epoch) | 14 | 43.57 | 1 |
| fulu | attestation_data(current_slot) | 14 | 0.56 | 0 |
| fulu | attester_duties(current_epoch) | 14 | 67.84 | 0 |
| fulu | block_production(v3) | 14 | 319.83 | 1 |
| fulu | states/{slot}/validators(past_head) | 14 | 69.30 | 0 |
| fulu | proposer_duties(next_epoch) | 15 | 40.27 | 1 |
| fulu | attestation_data(current_slot) | 15 | 0.56 | 0 |
| fulu | attester_duties(current_epoch) | 15 | 67.69 | 0 |
| fulu | block_production(v3) | 15 | 320.37 | 1 |
| fulu | states/{slot}/validators(past_head) | 15 | 69.04 | 0 |
| fulu | proposer_duties(next_epoch) | 16 | 38.97 | 1 |
| fulu | attestation_data(current_slot) | 16 | 0.55 | 0 |
| fulu | attester_duties(current_epoch) | 16 | 67.49 | 0 |
| fulu | block_production(v3) | 16 | 318.68 | 1 |
| fulu | states/{slot}/validators(past_head) | 16 | 68.76 | 0 |
| fulu | proposer_duties(next_epoch) | 17 | 52.48 | 1 |
| fulu | attestation_data(current_slot) | 17 | 1.34 | 0 |
| fulu | attester_duties(current_epoch) | 17 | 67.81 | 0 |
| fulu | block_production(v3) | 17 | 327.68 | 1 |
| fulu | states/{slot}/validators(past_head) | 17 | 69.37 | 0 |
| fulu | proposer_duties(next_epoch) | 18 | 33.14 | 1 |
| fulu | attestation_data(current_slot) | 18 | 0.69 | 0 |
| fulu | attester_duties(current_epoch) | 18 | 67.58 | 0 |
| fulu | block_production(v3) | 18 | 313.79 | 1 |
| fulu | states/{slot}/validators(past_head) | 18 | 69.02 | 0 |
| fulu | proposer_duties(next_epoch) | 19 | 39.54 | 1 |
| fulu | attestation_data(current_slot) | 19 | 0.53 | 0 |
| fulu | attester_duties(current_epoch) | 19 | 67.46 | 0 |
| fulu | block_production(v3) | 19 | 318.93 | 1 |
| fulu | states/{slot}/validators(past_head) | 19 | 69.65 | 0 |
| fulu | proposer_duties(next_epoch) | 20 | 37.51 | 1 |
| fulu | attestation_data(current_slot) | 20 | 1.15 | 0 |
| fulu | attester_duties(current_epoch) | 20 | 66.42 | 0 |
| fulu | block_production(v3) | 20 | 317.99 | 1 |
| fulu | states/{slot}/validators(past_head) | 20 | 69.25 | 0 |
| fulu | proposer_duties(next_epoch) | 21 | 33.09 | 1 |
| fulu | attestation_data(current_slot) | 21 | 0.75 | 0 |
| fulu | attester_duties(current_epoch) | 21 | 68.57 | 0 |
| fulu | block_production(v3) | 21 | 309.80 | 1 |
| fulu | states/{slot}/validators(past_head) | 21 | 69.41 | 0 |
| fulu | proposer_duties(next_epoch) | 22 | 35.34 | 1 |
| fulu | attestation_data(current_slot) | 22 | 1.00 | 0 |
| fulu | attester_duties(current_epoch) | 22 | 66.55 | 0 |
| fulu | block_production(v3) | 22 | 310.35 | 1 |
| fulu | states/{slot}/validators(past_head) | 22 | 68.87 | 0 |
| fulu | proposer_duties(next_epoch) | 23 | 31.62 | 1 |
| fulu | attestation_data(current_slot) | 23 | 1.01 | 0 |
| fulu | attester_duties(current_epoch) | 23 | 67.54 | 0 |
| fulu | block_production(v3) | 23 | 314.54 | 1 |
| fulu | states/{slot}/validators(past_head) | 23 | 69.79 | 0 |
| fulu | proposer_duties(next_epoch) | 24 | 42.51 | 1 |
| fulu | attestation_data(current_slot) | 24 | 0.49 | 0 |
| fulu | attester_duties(current_epoch) | 24 | 67.31 | 0 |
| fulu | block_production(v3) | 24 | 321.79 | 1 |
| fulu | states/{slot}/validators(past_head) | 24 | 69.84 | 0 |
| fulu | proposer_duties(next_epoch) | 25 | 36.85 | 1 |
| fulu | attestation_data(current_slot) | 25 | 0.55 | 0 |
| fulu | attester_duties(current_epoch) | 25 | 67.64 | 0 |
| fulu | block_production(v3) | 25 | 314.35 | 1 |
| fulu | states/{slot}/validators(past_head) | 25 | 69.78 | 0 |
| fulu | proposer_duties(next_epoch) | 26 | 46.23 | 1 |
| fulu | attestation_data(current_slot) | 26 | 0.59 | 0 |
| fulu | attester_duties(current_epoch) | 26 | 67.48 | 0 |
| fulu | block_production(v3) | 26 | 326.62 | 1 |
| fulu | states/{slot}/validators(past_head) | 26 | 69.36 | 0 |
| fulu | proposer_duties(next_epoch) | 27 | 39.29 | 1 |
| fulu | attestation_data(current_slot) | 27 | 1.21 | 0 |
| fulu | attester_duties(current_epoch) | 27 | 67.27 | 0 |
| fulu | block_production(v3) | 27 | 318.50 | 1 |
| fulu | states/{slot}/validators(past_head) | 27 | 69.26 | 0 |
| fulu | proposer_duties(next_epoch) | 28 | 45.09 | 1 |
| fulu | attestation_data(current_slot) | 28 | 1.17 | 0 |
| fulu | attester_duties(current_epoch) | 28 | 68.44 | 0 |
| fulu | block_production(v3) | 28 | 319.80 | 1 |
| fulu | states/{slot}/validators(past_head) | 28 | 68.49 | 0 |
| fulu | proposer_duties(next_epoch) | 29 | 52.82 | 1 |
| fulu | attestation_data(current_slot) | 29 | 0.79 | 0 |
| fulu | attester_duties(current_epoch) | 29 | 68.27 | 0 |
| fulu | block_production(v3) | 29 | 330.47 | 1 |
| fulu | states/{slot}/validators(past_head) | 29 | 68.94 | 0 |
| fulu | proposer_duties(next_epoch) | 30 | 53.76 | 1 |
| fulu | attestation_data(current_slot) | 30 | 0.71 | 0 |
| fulu | attester_duties(current_epoch) | 30 | 67.55 | 0 |
| fulu | block_production(v3) | 30 | 334.24 | 1 |
| fulu | states/{slot}/validators(past_head) | 30 | 70.17 | 0 |
| fulu | proposer_duties(next_epoch) | 31 | 38.91 | 1 |
| fulu | attestation_data(current_slot) | 31 | 0.94 | 0 |
| fulu | attester_duties(current_epoch) | 31 | 67.13 | 0 |
| fulu | block_production(v3) | 31 | 314.04 | 1 |
| fulu | states/{slot}/validators(past_head) | 31 | 68.45 | 0 |
| fulu | proposer_duties(next_epoch) | 32 | 38.07 | 1 |
| fulu | attestation_data(current_slot) | 32 | 0.56 | 0 |
| fulu | attester_duties(current_epoch) | 32 | 66.66 | 0 |
| fulu | block_production(v3) | 32 | 312.06 | 1 |
| fulu | states/{slot}/validators(past_head) | 32 | 68.03 | 0 |
| gloas | proposer_duties(next_epoch) | 1 | 306.28 | 1 |
| gloas | attestation_data(current_slot) | 1 | 0.67 | 0 |
| gloas | attester_duties(current_epoch) | 1 | 71.75 | 0 |
| gloas | ptc_duties(current_epoch) | 1 | 160.35 | 0 |
| gloas | block_production(v4) | 1 | 587.19 | 1 |
| gloas | states/{slot}/validators(past_head) | 1 | 74.45 | 0 |
| gloas | proposer_duties(next_epoch) | 2 | 303.36 | 1 |
| gloas | attestation_data(current_slot) | 2 | 0.67 | 0 |
| gloas | attester_duties(current_epoch) | 2 | 67.49 | 0 |
| gloas | ptc_duties(current_epoch) | 2 | 153.87 | 0 |
| gloas | block_production(v4) | 2 | 585.46 | 1 |
| gloas | states/{slot}/validators(past_head) | 2 | 72.77 | 0 |
| gloas | proposer_duties(next_epoch) | 3 | 313.55 | 1 |
| gloas | attestation_data(current_slot) | 3 | 1.47 | 0 |
| gloas | attester_duties(current_epoch) | 3 | 69.22 | 0 |
| gloas | ptc_duties(current_epoch) | 3 | 155.17 | 0 |
| gloas | block_production(v4) | 3 | 591.30 | 1 |
| gloas | states/{slot}/validators(past_head) | 3 | 70.42 | 0 |
| gloas | proposer_duties(next_epoch) | 4 | 317.55 | 1 |
| gloas | attestation_data(current_slot) | 4 | 1.27 | 0 |
| gloas | attester_duties(current_epoch) | 4 | 69.06 | 0 |
| gloas | ptc_duties(current_epoch) | 4 | 154.18 | 0 |
| gloas | block_production(v4) | 4 | 599.88 | 1 |
| gloas | states/{slot}/validators(past_head) | 4 | 71.55 | 0 |
| gloas | proposer_duties(next_epoch) | 5 | 311.18 | 1 |
| gloas | attestation_data(current_slot) | 5 | 0.81 | 0 |
| gloas | attester_duties(current_epoch) | 5 | 67.89 | 0 |
| gloas | ptc_duties(current_epoch) | 5 | 162.27 | 0 |
| gloas | block_production(v4) | 5 | 588.25 | 1 |
| gloas | states/{slot}/validators(past_head) | 5 | 70.56 | 0 |
| gloas | proposer_duties(next_epoch) | 6 | 314.95 | 1 |
| gloas | attestation_data(current_slot) | 6 | 0.51 | 0 |
| gloas | attester_duties(current_epoch) | 6 | 68.60 | 0 |
| gloas | ptc_duties(current_epoch) | 6 | 160.07 | 0 |
| gloas | block_production(v4) | 6 | 586.14 | 1 |
| gloas | states/{slot}/validators(past_head) | 6 | 70.76 | 0 |
| gloas | proposer_duties(next_epoch) | 7 | 310.98 | 1 |
| gloas | attestation_data(current_slot) | 7 | 0.89 | 0 |
| gloas | attester_duties(current_epoch) | 7 | 68.03 | 0 |
| gloas | ptc_duties(current_epoch) | 7 | 155.74 | 0 |
| gloas | block_production(v4) | 7 | 582.48 | 1 |
| gloas | states/{slot}/validators(past_head) | 7 | 71.02 | 0 |
| gloas | proposer_duties(next_epoch) | 8 | 314.32 | 1 |
| gloas | attestation_data(current_slot) | 8 | 0.82 | 0 |
| gloas | attester_duties(current_epoch) | 8 | 68.26 | 0 |
| gloas | ptc_duties(current_epoch) | 8 | 164.09 | 0 |
| gloas | block_production(v4) | 8 | 590.84 | 1 |
| gloas | states/{slot}/validators(past_head) | 8 | 71.03 | 0 |
| gloas | proposer_duties(next_epoch) | 9 | 325.86 | 1 |
| gloas | attestation_data(current_slot) | 9 | 1.15 | 0 |
| gloas | attester_duties(current_epoch) | 9 | 68.86 | 0 |
| gloas | ptc_duties(current_epoch) | 9 | 155.22 | 0 |
| gloas | block_production(v4) | 9 | 606.27 | 1 |
| gloas | states/{slot}/validators(past_head) | 9 | 70.41 | 0 |
| gloas | proposer_duties(next_epoch) | 10 | 318.18 | 1 |
| gloas | attestation_data(current_slot) | 10 | 0.98 | 0 |
| gloas | attester_duties(current_epoch) | 10 | 68.89 | 0 |
| gloas | ptc_duties(current_epoch) | 10 | 161.73 | 0 |
| gloas | block_production(v4) | 10 | 597.00 | 1 |
| gloas | states/{slot}/validators(past_head) | 10 | 70.34 | 0 |
| gloas | proposer_duties(next_epoch) | 11 | 317.04 | 1 |
| gloas | attestation_data(current_slot) | 11 | 1.15 | 0 |
| gloas | attester_duties(current_epoch) | 11 | 69.50 | 0 |
| gloas | ptc_duties(current_epoch) | 11 | 155.06 | 0 |
| gloas | block_production(v4) | 11 | 599.97 | 1 |
| gloas | states/{slot}/validators(past_head) | 11 | 70.41 | 0 |
| gloas | proposer_duties(next_epoch) | 12 | 309.86 | 1 |
| gloas | attestation_data(current_slot) | 12 | 0.91 | 0 |
| gloas | attester_duties(current_epoch) | 12 | 69.03 | 0 |
| gloas | ptc_duties(current_epoch) | 12 | 154.76 | 0 |
| gloas | block_production(v4) | 12 | 599.09 | 1 |
| gloas | states/{slot}/validators(past_head) | 12 | 69.79 | 0 |
| gloas | proposer_duties(next_epoch) | 13 | 312.11 | 1 |
| gloas | attestation_data(current_slot) | 13 | 1.21 | 0 |
| gloas | attester_duties(current_epoch) | 13 | 67.96 | 0 |
| gloas | ptc_duties(current_epoch) | 13 | 155.66 | 0 |
| gloas | block_production(v4) | 13 | 591.29 | 1 |
| gloas | states/{slot}/validators(past_head) | 13 | 70.72 | 0 |
| gloas | proposer_duties(next_epoch) | 14 | 311.22 | 1 |
| gloas | attestation_data(current_slot) | 14 | 0.67 | 0 |
| gloas | attester_duties(current_epoch) | 14 | 68.84 | 0 |
| gloas | ptc_duties(current_epoch) | 14 | 164.20 | 0 |
| gloas | block_production(v4) | 14 | 592.13 | 1 |
| gloas | states/{slot}/validators(past_head) | 14 | 70.82 | 0 |
| gloas | proposer_duties(next_epoch) | 15 | 325.78 | 1 |
| gloas | attestation_data(current_slot) | 15 | 0.98 | 0 |
| gloas | attester_duties(current_epoch) | 15 | 68.85 | 0 |
| gloas | ptc_duties(current_epoch) | 15 | 156.11 | 0 |
| gloas | block_production(v4) | 15 | 607.56 | 1 |
| gloas | states/{slot}/validators(past_head) | 15 | 70.23 | 0 |
| gloas | proposer_duties(next_epoch) | 16 | 302.10 | 1 |
| gloas | attestation_data(current_slot) | 16 | 0.98 | 0 |
| gloas | attester_duties(current_epoch) | 16 | 67.77 | 0 |
| gloas | ptc_duties(current_epoch) | 16 | 158.82 | 0 |
| gloas | block_production(v4) | 16 | 581.55 | 1 |
| gloas | states/{slot}/validators(past_head) | 16 | 70.25 | 0 |
| gloas | proposer_duties(next_epoch) | 17 | 317.70 | 1 |
| gloas | attestation_data(current_slot) | 17 | 0.93 | 0 |
| gloas | attester_duties(current_epoch) | 17 | 68.54 | 0 |
| gloas | ptc_duties(current_epoch) | 17 | 159.74 | 0 |
| gloas | block_production(v4) | 17 | 594.87 | 1 |
| gloas | states/{slot}/validators(past_head) | 17 | 70.94 | 0 |
| gloas | proposer_duties(next_epoch) | 18 | 313.25 | 1 |
| gloas | attestation_data(current_slot) | 18 | 1.05 | 0 |
| gloas | attester_duties(current_epoch) | 18 | 68.63 | 0 |
| gloas | ptc_duties(current_epoch) | 18 | 161.99 | 0 |
| gloas | block_production(v4) | 18 | 596.04 | 1 |
| gloas | states/{slot}/validators(past_head) | 18 | 69.92 | 0 |
| gloas | proposer_duties(next_epoch) | 19 | 300.34 | 1 |
| gloas | attestation_data(current_slot) | 19 | 0.74 | 0 |
| gloas | attester_duties(current_epoch) | 19 | 68.71 | 0 |
| gloas | ptc_duties(current_epoch) | 19 | 155.79 | 0 |
| gloas | block_production(v4) | 19 | 581.27 | 1 |
| gloas | states/{slot}/validators(past_head) | 19 | 70.09 | 0 |
| gloas | proposer_duties(next_epoch) | 20 | 309.78 | 1 |
| gloas | attestation_data(current_slot) | 20 | 0.80 | 0 |
| gloas | attester_duties(current_epoch) | 20 | 67.28 | 0 |
| gloas | ptc_duties(current_epoch) | 20 | 156.31 | 0 |
| gloas | block_production(v4) | 20 | 600.33 | 1 |
| gloas | states/{slot}/validators(past_head) | 20 | 69.64 | 0 |
| gloas | proposer_duties(next_epoch) | 21 | 334.96 | 1 |
| gloas | attestation_data(current_slot) | 21 | 0.91 | 0 |
| gloas | attester_duties(current_epoch) | 21 | 67.16 | 0 |
| gloas | ptc_duties(current_epoch) | 21 | 155.08 | 0 |
| gloas | block_production(v4) | 21 | 602.98 | 1 |
| gloas | states/{slot}/validators(past_head) | 21 | 71.42 | 0 |
| gloas | proposer_duties(next_epoch) | 22 | 314.07 | 1 |
| gloas | attestation_data(current_slot) | 22 | 0.80 | 0 |
| gloas | attester_duties(current_epoch) | 22 | 67.94 | 0 |
| gloas | ptc_duties(current_epoch) | 22 | 163.98 | 0 |
| gloas | block_production(v4) | 22 | 596.91 | 1 |
| gloas | states/{slot}/validators(past_head) | 22 | 69.89 | 0 |
| gloas | proposer_duties(next_epoch) | 23 | 309.99 | 1 |
| gloas | attestation_data(current_slot) | 23 | 0.66 | 0 |
| gloas | attester_duties(current_epoch) | 23 | 67.99 | 0 |
| gloas | ptc_duties(current_epoch) | 23 | 156.04 | 0 |
| gloas | block_production(v4) | 23 | 593.24 | 1 |
| gloas | states/{slot}/validators(past_head) | 23 | 70.18 | 0 |
| gloas | proposer_duties(next_epoch) | 24 | 306.04 | 1 |
| gloas | attestation_data(current_slot) | 24 | 0.92 | 0 |
| gloas | attester_duties(current_epoch) | 24 | 68.66 | 0 |
| gloas | ptc_duties(current_epoch) | 24 | 156.83 | 0 |
| gloas | block_production(v4) | 24 | 586.44 | 1 |
| gloas | states/{slot}/validators(past_head) | 24 | 71.45 | 0 |
| gloas | proposer_duties(next_epoch) | 25 | 310.68 | 1 |
| gloas | attestation_data(current_slot) | 25 | 0.94 | 0 |
| gloas | attester_duties(current_epoch) | 25 | 67.86 | 0 |
| gloas | ptc_duties(current_epoch) | 25 | 164.07 | 0 |
| gloas | block_production(v4) | 25 | 590.85 | 1 |
| gloas | states/{slot}/validators(past_head) | 25 | 70.79 | 0 |
| gloas | proposer_duties(next_epoch) | 26 | 311.54 | 1 |
| gloas | attestation_data(current_slot) | 26 | 0.67 | 0 |
| gloas | attester_duties(current_epoch) | 26 | 67.25 | 0 |
| gloas | ptc_duties(current_epoch) | 26 | 157.22 | 0 |
| gloas | block_production(v4) | 26 | 588.60 | 1 |
| gloas | states/{slot}/validators(past_head) | 26 | 69.36 | 0 |
| gloas | proposer_duties(next_epoch) | 27 | 308.49 | 1 |
| gloas | attestation_data(current_slot) | 27 | 0.86 | 0 |
| gloas | attester_duties(current_epoch) | 27 | 68.59 | 0 |
| gloas | ptc_duties(current_epoch) | 27 | 164.31 | 0 |
| gloas | block_production(v4) | 27 | 596.38 | 1 |
| gloas | states/{slot}/validators(past_head) | 27 | 70.26 | 0 |
| gloas | proposer_duties(next_epoch) | 28 | 316.25 | 1 |
| gloas | attestation_data(current_slot) | 28 | 0.88 | 0 |
| gloas | attester_duties(current_epoch) | 28 | 68.26 | 0 |
| gloas | ptc_duties(current_epoch) | 28 | 155.92 | 0 |
| gloas | block_production(v4) | 28 | 598.02 | 1 |
| gloas | states/{slot}/validators(past_head) | 28 | 71.11 | 0 |
| gloas | proposer_duties(next_epoch) | 29 | 340.30 | 1 |
| gloas | attestation_data(current_slot) | 29 | 0.82 | 0 |
| gloas | attester_duties(current_epoch) | 29 | 68.50 | 0 |
| gloas | ptc_duties(current_epoch) | 29 | 156.82 | 0 |
| gloas | block_production(v4) | 29 | 599.87 | 1 |
| gloas | states/{slot}/validators(past_head) | 29 | 71.00 | 0 |
| gloas | proposer_duties(next_epoch) | 30 | 313.38 | 1 |
| gloas | attestation_data(current_slot) | 30 | 1.00 | 0 |
| gloas | attester_duties(current_epoch) | 30 | 67.31 | 0 |
| gloas | ptc_duties(current_epoch) | 30 | 155.54 | 0 |
| gloas | block_production(v4) | 30 | 596.76 | 1 |
| gloas | states/{slot}/validators(past_head) | 30 | 70.78 | 0 |
| gloas | proposer_duties(next_epoch) | 31 | 306.89 | 1 |
| gloas | attestation_data(current_slot) | 31 | 1.24 | 0 |
| gloas | attester_duties(current_epoch) | 31 | 68.52 | 0 |
| gloas | ptc_duties(current_epoch) | 31 | 154.88 | 0 |
| gloas | block_production(v4) | 31 | 583.70 | 1 |
| gloas | states/{slot}/validators(past_head) | 31 | 70.71 | 0 |
| gloas | proposer_duties(next_epoch) | 32 | 325.18 | 1 |
| gloas | attestation_data(current_slot) | 32 | 1.17 | 0 |
| gloas | attester_duties(current_epoch) | 32 | 69.08 | 0 |
| gloas | ptc_duties(current_epoch) | 32 | 156.65 | 0 |
| gloas | block_production(v4) | 32 | 607.10 | 1 |
| gloas | states/{slot}/validators(past_head) | 32 | 70.50 | 0 |
