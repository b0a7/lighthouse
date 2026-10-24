//! Stale-head HTTP API cost benchmark for https://github.com/sigp/lighthouse/issues/9530.
//!
//! Evidence-only: not intended to be merged. Measures how expensive next-epoch
//! proposer-duties and `attestation_data` become when the head lags the wall clock.
//!
//! Reproduce (release; tests are compiled out in debug):
//! ```text
//! cargo test -p http_api --release --test bn_http_api_tests \
//!   stale_head_http_api_benchmark -- --ignored --nocapture
//! ```
//!
//! Optional env:
//! - `STALE_HEAD_BENCH_VALIDATORS` (default 16384)
//! - `STALE_HEAD_BENCH_VARIANT` (label printed in the table, e.g. `base`)
//! - `STALE_HEAD_BENCH_FORKS` (`fulu`, `gloas`, or `fulu,gloas`; default `fulu,gloas`)

use beacon_chain::test_utils::{
    AttestationStrategy, BlockStrategy, LightClientStrategy, SyncCommitteeStrategy,
};
use eth2::{BeaconNodeHttpClient, Timeouts};
use futures::future::join_all;
use http_api::test_utils::InteractiveTester;
use std::future::Future;
use std::time::{Duration, Instant};
use types::{Epoch, EthSpec, ForkName, Slot, Spec};

type E = Spec;

const STALE_EPOCHS: &[u64] = &[1, 2, 4, 8, 16, 32];
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
    repeat_mean_wall_ms: f64,
    repeat_total_wall_ms: f64,
    concurrent_wall_ms: f64,
    concurrent_cpu_ms: f64,
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
        "| fork | endpoint | stale_epochs | first_ms | repeat_mean_ms (n={REPEAT_CALLS}) | repeat_total_ms | concurrent_wall_ms (n={CONCURRENT_CALLS}) | concurrent_cpu_ms |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|");
    for row in rows {
        println!(
            "| {} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            row.fork,
            row.endpoint,
            row.stale_epochs,
            row.first_wall_ms,
            row.repeat_mean_wall_ms,
            row.repeat_total_wall_ms,
            row.concurrent_wall_ms,
            row.concurrent_cpu_ms,
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

async fn measure_endpoint_series<MakeFut, Fut, T>(make: MakeFut) -> (f64, f64, f64, f64, f64)
where
    MakeFut: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let (_first, first_wall) = measure_wall(make()).await;

    let mut repeat_total = Duration::ZERO;
    for _ in 0..REPEAT_CALLS {
        let (_v, wall) = measure_wall(make()).await;
        repeat_total += wall;
    }
    let repeat_mean = repeat_total / (REPEAT_CALLS as u32);

    let cpu_before = process_cpu_time();
    let wall_start = Instant::now();
    let results = join_all((0..CONCURRENT_CALLS).map(|_| make())).await;
    let concurrent_wall = wall_start.elapsed();
    let concurrent_cpu = process_cpu_time().saturating_sub(cpu_before);
    assert_eq!(results.len(), CONCURRENT_CALLS);

    (
        ms(first_wall),
        ms(repeat_mean),
        ms(repeat_total),
        ms(concurrent_wall),
        ms(concurrent_cpu),
    )
}

async fn run_fork(fork: ForkName, fork_label: &'static str, validator_count: usize) -> Vec<Row> {
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

    let mut rows = Vec::new();

    for &stale_epochs in STALE_EPOCHS {
        let future_slot = head_slot + Slot::new(stale_epochs * slots_per_epoch);
        harness.set_current_slot(future_slot);

        let current_epoch = harness.chain.epoch().unwrap();
        assert_eq!(current_epoch, head_epoch + stale_epochs);
        let next_epoch = current_epoch + 1;

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring proposer duties");
        let (first, repeat_mean, repeat_total, conc_wall, conc_cpu) =
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    client
                        .get_validator_duties_proposer(next_epoch)
                        .await
                        .expect("proposer duties")
                }
            })
            .await;
        rows.push(Row {
            fork: fork_label,
            endpoint: "proposer_duties(next_epoch)",
            stale_epochs,
            first_wall_ms: first,
            repeat_mean_wall_ms: repeat_mean,
            repeat_total_wall_ms: repeat_total,
            concurrent_wall_ms: conc_wall,
            concurrent_cpu_ms: conc_cpu,
        });

        eprintln!("--> fork={fork_label} stale_epochs={stale_epochs} measuring attestation_data");
        let slot = future_slot;
        let (first, repeat_mean, repeat_total, conc_wall, conc_cpu) =
            measure_endpoint_series(|| {
                let client = client.clone();
                async move {
                    client
                        .get_validator_attestation_data(slot, 0)
                        .await
                        .expect("attestation_data")
                }
            })
            .await;
        rows.push(Row {
            fork: fork_label,
            endpoint: "attestation_data(current_slot)",
            stale_epochs,
            first_wall_ms: first,
            repeat_mean_wall_ms: repeat_mean,
            repeat_total_wall_ms: repeat_total,
            concurrent_wall_ms: conc_wall,
            concurrent_cpu_ms: conc_cpu,
        });
    }

    rows
}

/// Ignored cost benchmark for stale-head proposer duties + attestation_data.
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
        let rows = run_fork(fork, label, validator_count).await;
        all_rows.extend(rows);
    }

    print_table(&variant, validator_count, &all_rows);
}
