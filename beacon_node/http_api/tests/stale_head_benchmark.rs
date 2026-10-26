//! Stale-head HTTP API cost benchmark for https://github.com/sigp/lighthouse/issues/9530.
//!
//! Evidence-only: not intended to be merged. Lives on
//! `cursor/stale-head-benchmark-advanced-state` (fix + this harness). Measures how
//! expensive validator API paths become when the head lags the wall clock, including
//! an epoch-by-epoch walk and `process_epoch` sample deltas.
//!
//! Call order for each jump depth N is intentional: next-epoch proposer duties first,
//! then attestation_data, then current-epoch attester duties. With exact-slot advanced-
//! state retention that primes committee shufflings on the proposer advance, the
//! subsequent attester request must not replay empty epochs from the head.
//!
//! Reproduce (release; tests are compiled out in debug):
//! ```text
//! cargo test -p http_api --release --test bn_http_api_tests \
//!   stale_head_http_api_benchmark -- --ignored --nocapture
//!
//! cargo test -p http_api --release --test bn_http_api_tests \
//!   stale_head_incremental_epoch_benchmark -- --ignored --nocapture
//! ```
//!
//! Optional env:
//! - `STALE_HEAD_BENCH_VALIDATORS` (default 32768)
//! - `STALE_HEAD_BENCH_VARIANT` (label printed in the table, e.g. `base`)
//! - `STALE_HEAD_BENCH_FORKS` (`fulu`, `gloas`, or `fulu,gloas`; default `fulu,gloas`)

use beacon_chain::test_utils::{
    AttestationStrategy, BlockStrategy, LightClientStrategy, SyncCommitteeStrategy,
};
use bls::Signature;
use eth2::types::{BuilderConfig, SkipRandaoVerification, StateId};
use eth2::{BeaconNodeHttpClient, Timeouts};
use futures::future::join_all;
use http_api::test_utils::InteractiveTester;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use types::{Epoch, EthSpec, ForkName, Slot, Spec};

type E = Spec;

const STALE_EPOCHS: &[u64] = &[1, 2, 4, 8, 16, 32];
const INCREMENTAL_EPOCHS: u64 = 32;
const REPEAT_CALLS: usize = 5;
const CONCURRENT_CALLS: usize = 8;
const SETUP_EPOCHS: u64 = 3;
/// 32k is near the top of the practical range on a 4-CPU / ~15GiB VM; 64k hit the default
/// 5s HTTP client timeout on concurrent N=32 bursts before timeouts were raised.
const DEFAULT_VALIDATOR_COUNT: usize = 32_768;
const ATTESTERS_FOR_SETUP: usize = 64;
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

struct Row {
    fork: &'static str,
    endpoint: &'static str,
    stale_epochs: u64,
    first_wall_ms: f64,
    first_process_epoch: u64,
    repeat_mean_wall_ms: f64,
    repeat_total_wall_ms: f64,
    repeat_process_epoch_total: u64,
    concurrent_wall_ms: f64,
    concurrent_cpu_ms: f64,
    concurrent_process_epoch: u64,
}

struct IncrementalRow {
    fork: &'static str,
    endpoint: &'static str,
    stale_epochs: u64,
    first_wall_ms: f64,
    first_process_epoch: u64,
}

/// Sample count for the global `beacon_state_processing_process_epoch` histogram.
fn process_epoch_sample_count() -> u64 {
    metrics::gather()
        .iter()
        .find(|mf| mf.get_name() == "beacon_state_processing_process_epoch")
        .map(|mf| {
            mf.get_metric()
                .iter()
                .map(|m| m.get_histogram().get_sample_count())
                .sum()
        })
        .unwrap_or(0)
}

fn validator_count() -> usize {
    std::env::var("STALE_HEAD_BENCH_VALIDATORS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_VALIDATOR_COUNT)
}

fn variant_label() -> String {
    std::env::var("STALE_HEAD_BENCH_VARIANT").unwrap_or_else(|_| "unknown".to_string())
}

fn forks_to_run() -> Vec<(ForkName, &'static str)> {
    let raw = std::env::var("STALE_HEAD_BENCH_FORKS").unwrap_or_else(|_| "fulu,gloas".to_string());
    let mut out = Vec::new();
    for part in raw.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "fulu" => out.push((ForkName::Fulu, "fulu")),
            "gloas" => out.push((ForkName::Gloas, "gloas")),
            "" => {}
            other => panic!("unknown STALE_HEAD_BENCH_FORKS entry: {other}"),
        }
    }
    assert!(
        !out.is_empty(),
        "STALE_HEAD_BENCH_FORKS must list at least one fork"
    );
    out
}

fn process_cpu_time() -> Duration {
    // Linux x86_64: struct rusage begins with two timeval (utime, stime), each two i64 fields.
    let mut buf = [0u8; 256];
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut u8) -> i32;
    }
    const RUSAGE_SELF: i32 = 0;
    let rc = unsafe { getrusage(RUSAGE_SELF, buf.as_mut_ptr()) };
    assert_eq!(rc, 0, "getrusage failed");

    let read_i64 = |off: usize| i64::from_ne_bytes(buf[off..off + 8].try_into().unwrap());
    let utime_sec = read_i64(0);
    let utime_usec = read_i64(8);
    let stime_sec = read_i64(16);
    let stime_usec = read_i64(24);

    let usec = utime_sec
        .saturating_mul(1_000_000)
        .saturating_add(utime_usec)
        .saturating_add(stime_sec.saturating_mul(1_000_000))
        .saturating_add(stime_usec);
    Duration::from_micros(usec.max(0) as u64)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

fn print_table(variant: &str, validator_count: usize, rows: &[Row]) {
    println!();
    println!("### stale-head benchmark variant=`{variant}` validators={validator_count}");
    println!(
        "| fork | endpoint | stale_epochs | first_ms | first_pe | repeat_mean_ms (n={REPEAT_CALLS}) | repeat_total_ms | repeat_pe_total | concurrent_wall_ms (n={CONCURRENT_CALLS}) | concurrent_cpu_ms | concurrent_pe |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for row in rows {
        println!(
            "| {} | {} | {} | {:.2} | {} | {:.2} | {:.2} | {} | {:.2} | {:.2} | {} |",
            row.fork,
            row.endpoint,
            row.stale_epochs,
            row.first_wall_ms,
            row.first_process_epoch,
            row.repeat_mean_wall_ms,
            row.repeat_total_wall_ms,
            row.repeat_process_epoch_total,
            row.concurrent_wall_ms,
            row.concurrent_cpu_ms,
            row.concurrent_process_epoch,
        );
    }
    println!();
}

fn print_incremental_table(variant: &str, validator_count: usize, rows: &[IncrementalRow]) {
    println!();
    println!(
        "### stale-head epoch-by-epoch incremental variant=`{variant}` validators={validator_count}"
    );
    println!("| fork | endpoint | stale_epochs | first_ms | first_pe |");
    println!("|---|---|---:|---:|---:|");
    for row in rows {
        println!(
            "| {} | {} | {} | {:.2} | {} |",
            row.fork, row.endpoint, row.stale_epochs, row.first_wall_ms, row.first_process_epoch
        );
    }
    println!();
}

async fn measure_wall<Fut, T>(fut: Fut) -> (T, Duration)
where
    Fut: Future<Output = T>,
{
    let wall_start = Instant::now();
    let value = fut.await;
    (value, wall_start.elapsed())
}

/// Returns (first_ms, first_pe, repeat_mean_ms, repeat_total_ms, repeat_pe_total,
/// concurrent_wall_ms, concurrent_cpu_ms, concurrent_pe).
async fn measure_endpoint_series<MakeFut, Fut, T>(
    make: MakeFut,
) -> (f64, u64, f64, f64, u64, f64, f64, u64)
where
    MakeFut: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let pe_before = process_epoch_sample_count();
    let (_first, first_wall) = measure_wall(make()).await;
    let first_pe = process_epoch_sample_count().saturating_sub(pe_before);

    let mut repeat_total = Duration::ZERO;
    let mut repeat_pe_total = 0_u64;
    for _ in 0..REPEAT_CALLS {
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(make()).await;
        repeat_pe_total =
            repeat_pe_total.saturating_add(process_epoch_sample_count().saturating_sub(pe_before));
        repeat_total += wall;
    }
    let repeat_mean = repeat_total / (REPEAT_CALLS as u32);

    let pe_before = process_epoch_sample_count();
    let cpu_before = process_cpu_time();
    let wall_start = Instant::now();
    let results = join_all((0..CONCURRENT_CALLS).map(|_| make())).await;
    let concurrent_wall = wall_start.elapsed();
    let concurrent_cpu = process_cpu_time().saturating_sub(cpu_before);
    let concurrent_pe = process_epoch_sample_count().saturating_sub(pe_before);
    assert_eq!(results.len(), CONCURRENT_CALLS);

    (
        ms(first_wall),
        first_pe,
        ms(repeat_mean),
        ms(repeat_total),
        repeat_pe_total,
        ms(concurrent_wall),
        ms(concurrent_cpu),
        concurrent_pe,
    )
}

fn push_series_row(
    rows: &mut Vec<Row>,
    fork: &'static str,
    endpoint: &'static str,
    stale_epochs: u64,
    series: (f64, u64, f64, f64, u64, f64, f64, u64),
) {
    let (first, first_pe, repeat_mean, repeat_total, repeat_pe, conc_wall, conc_cpu, conc_pe) =
        series;
    rows.push(Row {
        fork,
        endpoint,
        stale_epochs,
        first_wall_ms: first,
        first_process_epoch: first_pe,
        repeat_mean_wall_ms: repeat_mean,
        repeat_total_wall_ms: repeat_total,
        repeat_process_epoch_total: repeat_pe,
        concurrent_wall_ms: conc_wall,
        concurrent_cpu_ms: conc_cpu,
        concurrent_process_epoch: conc_pe,
    });
}

async fn produce_block(
    client: &BeaconNodeHttpClient,
    fork: ForkName,
    slot: Slot,
) -> Result<(), String> {
    let randao = Signature::infinity().expect("infinity signature").into();
    if fork.gloas_enabled() {
        client
            .post_validator_blocks_v4_modular::<E>(
                slot,
                &randao,
                None,
                SkipRandaoVerification::Yes,
                false,
                &BuilderConfig::empty(),
                None,
                ForkName::Gloas,
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("v4 produce: {e:?}"))
    } else {
        client
            .get_validator_blocks_v3_modular::<E>(
                slot,
                &randao,
                None,
                SkipRandaoVerification::Yes,
                None,
                None,
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("v3 produce: {e:?}"))
    }
}

async fn setup_tester(
    fork: ForkName,
    fork_label: &'static str,
    validator_count: usize,
) -> (InteractiveTester<E>, BeaconNodeHttpClient, Slot, Epoch) {
    let mut spec = fork.make_genesis_spec(E::default_spec());
    // Keep aggregator targets high so attestation production stays cheap in setup.
    spec.target_aggregators_per_committee = u64::MAX / 2;

    eprintln!("==> building InteractiveTester fork={fork_label} validators={validator_count}");
    let setup_start = Instant::now();
    let tester = InteractiveTester::<E>::new(Some(spec), validator_count).await;
    // InteractiveTester defaults to 5s HTTP timeouts; stale concurrent bursts need more headroom.
    let client = BeaconNodeHttpClient::new(
        tester.client.server().clone(),
        Timeouts::set_all(HTTP_TIMEOUT),
    );
    let harness = &tester.harness;
    let mock_el = harness.mock_execution_layer.as_ref().unwrap();
    mock_el.server.all_payloads_valid();

    let slots_per_epoch = E::slots_per_epoch();
    let num_slots = SETUP_EPOCHS * slots_per_epoch;
    let setup_attesters: Vec<usize> = (0..ATTESTERS_FOR_SETUP.min(validator_count)).collect();

    harness.advance_slot();
    harness
        .extend_chain_with_sync(
            num_slots as usize,
            BlockStrategy::OnCanonicalHead,
            AttestationStrategy::SomeValidators(setup_attesters),
            SyncCommitteeStrategy::NoValidators,
            LightClientStrategy::Disabled,
        )
        .await;

    let head_slot = harness.head_slot();
    let head_epoch = head_slot.epoch(slots_per_epoch);
    assert_eq!(head_epoch, Epoch::new(SETUP_EPOCHS));
    eprintln!(
        "==> setup done in {:.1}s (head_slot={head_slot})",
        setup_start.elapsed().as_secs_f64()
    );

    (tester, client, head_slot, head_epoch)
}

async fn run_fork_jump_to_n(
    fork: ForkName,
    fork_label: &'static str,
    validator_count: usize,
) -> Vec<Row> {
    let (tester, client, head_slot, head_epoch) =
        setup_tester(fork, fork_label, validator_count).await;
    let harness = &tester.harness;
    let slots_per_epoch = E::slots_per_epoch();
    let attester_indices: Arc<Vec<u64>> = Arc::new((0..validator_count as u64).collect());

    let mut rows = Vec::new();

    for &stale_epochs in STALE_EPOCHS {
        let future_slot = head_slot + Slot::new(stale_epochs * slots_per_epoch);
        harness.set_current_slot(future_slot);

        let current_epoch = harness.chain.epoch().unwrap();
        assert_eq!(current_epoch, head_epoch + stale_epochs);
        let next_epoch = current_epoch + 1;

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring proposer duties");
        push_series_row(
            &mut rows,
            fork_label,
            "proposer_duties(next_epoch)",
            stale_epochs,
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    client
                        .get_validator_duties_proposer(next_epoch)
                        .await
                        .expect("proposer duties")
                }
            })
            .await,
        );

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring attestation_data");
        let slot = future_slot;
        push_series_row(
            &mut rows,
            fork_label,
            "attestation_data(current_slot)",
            stale_epochs,
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    client
                        .get_validator_attestation_data(slot, 0)
                        .await
                        .expect("attestation_data")
                }
            })
            .await,
        );

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring attester duties");
        let indices = attester_indices.clone();
        push_series_row(
            &mut rows,
            fork_label,
            "attester_duties(current_epoch)",
            stale_epochs,
            measure_endpoint_series(|| {
                let client = client.clone();
                let indices = indices.clone();
                async move {
                    client
                        .post_validator_duties_attester(current_epoch, indices.as_slice())
                        .await
                        .expect("attester duties")
                }
            })
            .await,
        );

        if fork.gloas_enabled() {
            eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring ptc duties");
            let indices = attester_indices.clone();
            push_series_row(
                &mut rows,
                fork_label,
                "ptc_duties(current_epoch)",
                stale_epochs,
                measure_endpoint_series(|| {
                    let client = client.clone();
                    let indices = indices.clone();
                    async move {
                        client
                            .post_validator_duties_ptc(current_epoch, indices.as_slice())
                            .await
                            .expect("ptc duties")
                    }
                })
                .await,
            );
        }

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring block production");
        push_series_row(
            &mut rows,
            fork_label,
            if fork.gloas_enabled() {
                "block_production(v4)"
            } else {
                "block_production(v3)"
            },
            stale_epochs,
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    produce_block(&client, fork, slot)
                        .await
                        .expect("block production")
                }
            })
            .await,
        );

        eprintln!(
            "--> fork={fork_label} stale_epochs={stale_epochs} measuring states/{{slot}}/validators"
        );
        push_series_row(
            &mut rows,
            fork_label,
            "states/{slot}/validators(past_head)",
            stale_epochs,
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    client
                        .post_beacon_states_validators(StateId::Slot(slot), None, None)
                        .await
                        .expect("states validators http")
                        .expect("states validators present")
                }
            })
            .await,
        );
    }

    rows
}

async fn run_fork_incremental(
    fork: ForkName,
    fork_label: &'static str,
    validator_count: usize,
) -> Vec<IncrementalRow> {
    let (tester, client, head_slot, head_epoch) =
        setup_tester(fork, fork_label, validator_count).await;
    let harness = &tester.harness;
    let slots_per_epoch = E::slots_per_epoch();
    let attester_indices: Vec<u64> = (0..validator_count as u64).collect();
    let produce_label = if fork.gloas_enabled() {
        "block_production(v4)"
    } else {
        "block_production(v3)"
    };

    let mut rows = Vec::new();

    for step in 1..=INCREMENTAL_EPOCHS {
        let future_slot = head_slot + Slot::new(step * slots_per_epoch);
        harness.set_current_slot(future_slot);
        let current_epoch = harness.chain.epoch().unwrap();
        assert_eq!(current_epoch, head_epoch + step);
        let next_epoch = current_epoch + 1;
        let slot = future_slot;

        let mut push_inc = |endpoint: &'static str, wall: Duration, pe: u64| {
            rows.push(IncrementalRow {
                fork: fork_label,
                endpoint,
                stale_epochs: step,
                first_wall_ms: ms(wall),
                first_process_epoch: pe,
            });
        };

        eprintln!("--> incremental fork={fork_label} stale_epochs={step} proposer duties");
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(async {
            client
                .get_validator_duties_proposer(next_epoch)
                .await
                .expect("proposer duties")
        })
        .await;
        push_inc(
            "proposer_duties(next_epoch)",
            wall,
            process_epoch_sample_count().saturating_sub(pe_before),
        );

        eprintln!("--> incremental fork={fork_label} stale_epochs={step} attestation_data");
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(async {
            client
                .get_validator_attestation_data(slot, 0)
                .await
                .expect("attestation_data")
        })
        .await;
        push_inc(
            "attestation_data(current_slot)",
            wall,
            process_epoch_sample_count().saturating_sub(pe_before),
        );

        eprintln!("--> incremental fork={fork_label} stale_epochs={step} attester duties");
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(async {
            client
                .post_validator_duties_attester(current_epoch, &attester_indices)
                .await
                .expect("attester duties")
        })
        .await;
        push_inc(
            "attester_duties(current_epoch)",
            wall,
            process_epoch_sample_count().saturating_sub(pe_before),
        );

        if fork.gloas_enabled() {
            eprintln!("--> incremental fork={fork_label} stale_epochs={step} ptc duties");
            let pe_before = process_epoch_sample_count();
            let (_v, wall) = measure_wall(async {
                client
                    .post_validator_duties_ptc(current_epoch, &attester_indices)
                    .await
                    .expect("ptc duties")
            })
            .await;
            push_inc(
                "ptc_duties(current_epoch)",
                wall,
                process_epoch_sample_count().saturating_sub(pe_before),
            );
        }

        eprintln!("--> incremental fork={fork_label} stale_epochs={step} block production");
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(async {
            produce_block(&client, fork, slot)
                .await
                .expect("block production")
        })
        .await;
        push_inc(
            produce_label,
            wall,
            process_epoch_sample_count().saturating_sub(pe_before),
        );

        eprintln!(
            "--> incremental fork={fork_label} stale_epochs={step} states/{{slot}}/validators"
        );
        let pe_before = process_epoch_sample_count();
        let (_v, wall) = measure_wall(async {
            client
                .post_beacon_states_validators(StateId::Slot(slot), None, None)
                .await
                .expect("states validators http")
                .expect("states validators present")
        })
        .await;
        push_inc(
            "states/{slot}/validators(past_head)",
            wall,
            process_epoch_sample_count().saturating_sub(pe_before),
        );
    }

    rows
}

/// Ignored cost benchmark: jump wall clock to each N ∈ {1,2,4,8,16,32} and measure
/// first / repeat / concurrent cost for every covered endpoint (+ states/validators).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "expensive stale-head benchmark for #9530 evidence; run with --ignored --nocapture"]
async fn stale_head_http_api_benchmark() {
    let validator_count = validator_count();
    let variant = variant_label();
    let nproc = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);

    println!("stale-head benchmark starting");
    println!("variant={variant}");
    println!("validators={validator_count}");
    println!("available_parallelism={nproc}");
    println!("repeat_calls={REPEAT_CALLS}");
    println!("concurrent_calls={CONCURRENT_CALLS}");
    println!("setup_epochs={SETUP_EPOCHS}");
    println!("stale_epochs={STALE_EPOCHS:?}");

    let mut all_rows = Vec::new();
    for (fork, label) in forks_to_run() {
        let rows = run_fork_jump_to_n(fork, label, validator_count).await;
        all_rows.extend(rows);
    }

    print_table(&variant, validator_count, &all_rows);
}

/// Ignored cost benchmark: advance the clock one epoch at a time and record the
/// first-call cost after each step. With an advanced-head-state cache this stays
/// ~flat; without it the cost grows with N.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "expensive stale-head incremental benchmark for #9530 evidence; run with --ignored --nocapture"]
async fn stale_head_incremental_epoch_benchmark() {
    let validator_count = validator_count();
    let variant = variant_label();
    let nproc = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);

    println!("stale-head epoch-by-epoch incremental benchmark starting");
    println!("variant={variant}");
    println!("validators={validator_count}");
    println!("available_parallelism={nproc}");
    println!("setup_epochs={SETUP_EPOCHS}");
    println!("incremental_epochs={INCREMENTAL_EPOCHS}");

    let mut all_rows = Vec::new();
    for (fork, label) in forks_to_run() {
        let rows = run_fork_incremental(fork, label, validator_count).await;
        all_rows.extend(rows);
    }

    print_incremental_table(&variant, validator_count, &all_rows);
}
