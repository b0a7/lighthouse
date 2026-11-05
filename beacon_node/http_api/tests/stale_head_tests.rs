//! Regression tests for validator API cost while the canonical head is stale.
//!
//! When the head is many epochs behind the wall clock, handlers that need a current or next
//! epoch state must not replay every empty epoch from the head on each new epoch. These tests
//! jump the clock, then advance epoch-by-epoch and assert that `process_epoch` work for each
//! new epoch stays roughly one epoch.
//!
//! A few cases are labelled guards. They passed before this cache existed and check that the
//! cache does not make an already-cheap path expensive.

use beacon_chain::test_utils::{
    AttestationStrategy, BeaconChainHarness, BlockStrategy, EphemeralHarnessType,
    LightClientStrategy, SyncCommitteeStrategy, test_spec,
};
use eth2::BeaconNodeHttpClient;
use eth2::types::StateId;
use http_api::test_utils::InteractiveTester;
use state_processing::state_advance::partial_state_advance;
use std::sync::LazyLock;
use types::{Epoch, EthSpec, Hash256, Slot, Spec};

type E = Spec;

/// `process_epoch` metrics are process-global; serialize these tests so parallel `cargo test`
/// workers do not inflate each other's deltas.
static PROCESS_EPOCH_METRIC_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

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

struct StaleHeadHarness {
    tester: InteractiveTester<E>,
    validator_count: usize,
    slots_per_epoch: u64,
    stale_epochs: u64,
    head_block_root: Hash256,
    /// Wall-clock slot after the initial stale jump (head + stale_epochs).
    future_slot: Slot,
}

impl StaleHeadHarness {
    async fn setup(stale_epochs: u64) -> Self {
        let spec = test_spec::<E>();
        // 64 validators so Gloas genesis (balance-weighted proposer selection) succeeds.
        let validator_count = 64;
        let slots_per_epoch = E::slots_per_epoch();

        let tester = InteractiveTester::<E>::new(Some(spec), validator_count).await;
        let harness = &tester.harness;
        let mock_el = harness.mock_execution_layer.as_ref().unwrap();
        mock_el.server.all_payloads_valid();

        let num_slots = 3 * slots_per_epoch;
        harness.advance_slot();
        harness
            .extend_chain_with_sync(
                num_slots as usize,
                BlockStrategy::OnCanonicalHead,
                AttestationStrategy::AllValidators,
                SyncCommitteeStrategy::NoValidators,
                LightClientStrategy::Disabled,
            )
            .await;

        let head_slot = harness.head_slot();
        let head_epoch = head_slot.epoch(slots_per_epoch);
        assert_eq!(head_epoch, Epoch::new(3));

        let future_slot = head_slot + Slot::new(stale_epochs * slots_per_epoch);
        harness.set_current_slot(future_slot);

        let current_epoch = harness.chain.epoch().unwrap();
        assert_eq!(current_epoch, head_epoch + stale_epochs);
        assert_eq!(harness.chain.slot().unwrap(), future_slot);

        let head_block_root = harness.chain.canonical_head.cached_head().head_block_root();

        Self {
            tester,
            validator_count,
            slots_per_epoch,
            stale_epochs,
            head_block_root,
            future_slot,
        }
    }

    fn harness(&self) -> &BeaconChainHarness<EphemeralHarnessType<E>> {
        &self.tester.harness
    }

    fn client(&self) -> &BeaconNodeHttpClient {
        &self.tester.client
    }
}

/// Guard: sync duties in the same sync period were already cheap before the advanced-head
/// cache. This checks the cache does not start replaying empty epochs for them.
#[tokio::test(flavor = "multi_thread")]
async fn sync_duties_cheap_with_stale_head() {
    let _metric_guard = PROCESS_EPOCH_METRIC_LOCK.lock().await;
    let ctx = StaleHeadHarness::setup(12).await;
    let indices: Vec<u64> = (0..ctx.validator_count as u64).collect();
    let epoch = ctx.harness().chain.epoch().unwrap();

    let before = process_epoch_sample_count();
    let _duties = ctx
        .client()
        .post_validator_duties_sync(epoch, &indices)
        .await
        .unwrap();
    let delta = process_epoch_sample_count().saturating_sub(before);

    assert_eq!(
        delta, 0,
        "sync duties within the same sync period must not advance empty epochs (delta={delta})"
    );
}

/// Guard: `states/{slot}/validators` for a skipped slot at the wall clock was already cheap.
/// `state_root_at_slot` returns the head state root without empty-epoch processing. This checks
/// the cache does not change that.
#[tokio::test(flavor = "multi_thread")]
async fn states_validators_past_head_cheap_with_stale_head() {
    let _metric_guard = PROCESS_EPOCH_METRIC_LOCK.lock().await;
    let ctx = StaleHeadHarness::setup(12).await;
    let past_head_slot = ctx.future_slot;
    assert!(past_head_slot > ctx.harness().head_slot());

    let before = process_epoch_sample_count();
    let result = ctx
        .client()
        .post_beacon_states_validators(StateId::Slot(past_head_slot), None, None)
        .await
        .unwrap()
        .expect("skipped slot at/before wall clock resolves via head state root");
    let delta = process_epoch_sample_count().saturating_sub(before);

    assert_eq!(
        delta, 0,
        "states/{{slot}}/validators for a skip slot past the head must not process epochs \
         (delta={delta})"
    );
    assert_eq!(result.data.len(), ctx.validator_count);

    // Head fallback used by Charon is a cheap clone.
    let before_head = process_epoch_sample_count();
    let head = ctx
        .client()
        .post_beacon_states_validators(StateId::Head, None, None)
        .await
        .unwrap()
        .expect("head validators");
    let head_delta = process_epoch_sample_count().saturating_sub(before_head);
    assert_eq!(head_delta, 0);
    assert_eq!(head.data.len(), ctx.validator_count);
}

/// Sanity: without going through the HTTP cache layers, advancing from the head for N epochs
/// really does cost ~N process_epoch samples (documents the metric used above).
#[tokio::test(flavor = "multi_thread")]
async fn process_epoch_count_scales_with_manual_advance() {
    let _metric_guard = PROCESS_EPOCH_METRIC_LOCK.lock().await;
    let ctx = StaleHeadHarness::setup(6).await;
    let harness = ctx.harness();
    let mut state = harness.get_current_state();
    let state_root = harness.chain.canonical_head.cached_head().head_state_root();
    let target = ctx.future_slot;

    let before = process_epoch_sample_count();
    partial_state_advance(
        &mut state,
        Some(state_root),
        target,
        harness.chain.builder_onboarding_cache.as_deref(),
        &harness.spec,
    )
    .unwrap();
    let delta = process_epoch_sample_count().saturating_sub(before);
    assert_eq!(state.slot(), target);
    assert!(
        delta >= ctx.stale_epochs.saturating_sub(1),
        "manual advance over {stale} stale epochs should record ~that many process_epoch samples, got {delta}",
        stale = ctx.stale_epochs
    );
}

/// A sync-duty lookup for the next sync period, with an unknown validator, must not leave the
/// shared advanced state so far ahead that later duty calls fail.
#[tokio::test(flavor = "multi_thread")]
async fn sync_duties_next_period_unknown_validator_does_not_break_later_duties() {
    let _metric_guard = PROCESS_EPOCH_METRIC_LOCK.lock().await;
    let ctx = StaleHeadHarness::setup(8).await;
    let spec = ctx.harness().spec.clone();
    let current_epoch = ctx.harness().chain.epoch().unwrap();
    let period = current_epoch.sync_committee_period(&spec).unwrap();
    let next_period_epoch = spec.epochs_per_sync_committee_period * (period + 1);
    let unknown = vec![u64::MAX / 4];

    // The handler rejects the unknown index, but only after loading a state at that period.
    let _ = ctx
        .client()
        .post_validator_duties_sync(next_period_epoch, &unknown)
        .await;

    let slot = ctx.harness().chain.slot().unwrap();
    let attestation = ctx
        .client()
        .get_validator_attestation_data(slot, 0)
        .await
        .expect("attestation_data after a far sync-duty lookup");
    assert_eq!(attestation.data.beacon_block_root, ctx.head_block_root);
    assert_eq!(attestation.data.slot, slot);

    let indices: Vec<u64> = (0..ctx.validator_count as u64).collect();
    let epoch = ctx.harness().chain.epoch().unwrap();
    let attesters = ctx
        .client()
        .post_validator_duties_attester(epoch, &indices)
        .await
        .expect("attester duties after a far sync-duty lookup");
    assert!(!attesters.data.is_empty());

    let proposers = ctx
        .client()
        .get_validator_duties_proposer_v2(epoch)
        .await
        .expect("current-epoch proposer duties after a far sync-duty lookup");
    assert_eq!(proposers.data.len() as u64, ctx.slots_per_epoch);
}

/// Pre-Fulu, next-epoch proposer duties used to leave the shared state in the next epoch, and
/// the following current-epoch request failed with `SlotOutOfBounds`.
#[tokio::test(flavor = "multi_thread")]
async fn next_epoch_proposer_duties_do_not_break_current_epoch() {
    let _metric_guard = PROCESS_EPOCH_METRIC_LOCK.lock().await;
    let ctx = StaleHeadHarness::setup(8).await;
    let current_epoch = ctx.harness().chain.epoch().unwrap();
    let next_epoch = current_epoch + 1;

    let next = ctx
        .client()
        .get_validator_duties_proposer_v2(next_epoch)
        .await
        .expect("next-epoch proposer duties");
    assert_eq!(next.data.len() as u64, ctx.slots_per_epoch);

    let current = ctx
        .client()
        .get_validator_duties_proposer_v2(current_epoch)
        .await
        .expect("current-epoch proposer duties after a next-epoch request");
    assert_eq!(current.data.len() as u64, ctx.slots_per_epoch);
}
