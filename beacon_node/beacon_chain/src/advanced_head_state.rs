//! Retains empty-slot advances of the canonical head so validator API calls stay cheap while
//! the chain is not producing blocks.
//!
//! Validator API handlers that need a state at the wall-clock epoch used to clone the
//! unadvanced head and replay every empty epoch. During an outage that cost grows with
//! staleness, and it was paid again on every new epoch and by every concurrent caller.
//!
//! The cache keeps at most:
//! - one partial advance (intermediate state roots are not computed; suitable for shuffling,
//!   duties, and attestation justified checkpoints), and
//! - one complete advance (valid state roots; suitable for block production).
//!
//! Callers always receive a state at exactly the slot they asked for. An entry is reused only
//! when its slot is less than or equal to the target; it is then cloned and advanced forward.
//! Anything past roughly the wall clock plus one epoch is computed on a throwaway clone and is
//! not stored, so a far request cannot evict the entry later callers need.
//!
//! ## Serialized advances and invalidation
//!
//! Advances that can be retained run one at a time per kind. Each kind has its own advance lock,
//! taken before the cache mutex. A caller that waits for the lock usually finds the previous
//! caller's result in the cache and returns it without advancing. An advance whose target is past
//! the retain window is not stored, so it runs without the lock and cannot hold up nearby requests.
//! Callers for that far target each advance.
//!
//! `invalidate` takes only the cache mutex. It does not wait for an advance in progress: it bumps
//! a generation and drops the entries, and a store from an older generation is ignored.
//!
//! ## Lock ordering
//!
//! Advance lock, then the cache mutex. `canonical_head`'s `recompute_head` mutex is taken before
//! the cache mutex, because `invalidate` runs while `recompute_head` is held. Do not advance a
//! state or read the database while holding the cache mutex.
//!
//! ## What is safe to persist
//!
//! Partial results must not be written with `put_state`. Complete results have valid state roots
//! and may be used for block production, but this cache does not persist them. A multi-slot jump
//! does not form the continuous hot-summary chain `put_state` expects. The state-advance timer
//! persists at most one slot at a time, and only within `MAX_ADVANCE_DISTANCE` of the head block.
//!
//! Importing a block this node produced still replays empty slots once. The demand-path complete
//! state lives only in this cache, so block import loads the unadvanced head from the store.
//! That replay is paid once per new head; the new head snapshot then replaces these entries.

use crate::metrics;
use parking_lot::{Mutex, MutexGuard};
use tracing::debug;
use types::{BeaconState, EthSpec, Hash256, Slot};

/// One retained empty-slot advance of a single head block.
#[derive(Clone)]
pub(crate) struct AdvancedHeadStateEntry<E: EthSpec> {
    pub(crate) head_block_root: Hash256,
    /// State root to feed the next partial or complete advance.
    ///
    /// For a partial advance this is zero once the state is past the block header slot.
    pub(crate) state_root: Hash256,
    pub(crate) state: BeaconState<E>,
}

impl<E: EthSpec> AdvancedHeadStateEntry<E> {
    pub(crate) fn new(
        head_block_root: Hash256,
        state_root: Hash256,
        state: BeaconState<E>,
    ) -> Self {
        Self {
            head_block_root,
            state_root,
            state,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AdvanceKind {
    /// Skip intermediate state roots. The state is not consensus-valid.
    Partial,
    /// Compute proper state roots. The state may be used for block production.
    Complete,
}

impl AdvanceKind {
    fn index(self) -> usize {
        match self {
            AdvanceKind::Partial => 0,
            AdvanceKind::Complete => 1,
        }
    }
}

/// Parameters for one `get_or_advance_to` call.
pub(crate) struct AdvanceRequest {
    pub(crate) kind: AdvanceKind,
    pub(crate) head_block_root: Hash256,
    pub(crate) target_slot: Slot,
    /// Furthest slot this call is allowed to retain. Typically the wall-clock slot plus one epoch.
    pub(crate) max_store_slot: Slot,
    pub(crate) wall_clock_slot: Slot,
}

struct Inner<E: EthSpec> {
    generation: u64,
    partial: Option<AdvancedHeadStateEntry<E>>,
    complete: Option<AdvancedHeadStateEntry<E>>,
}

impl<E: EthSpec> Default for Inner<E> {
    fn default() -> Self {
        Self {
            generation: 0,
            partial: None,
            complete: None,
        }
    }
}

/// At most one partial and one complete retained advanced head state.
#[derive(Default)]
pub struct AdvancedHeadStateCache<E: EthSpec> {
    inner: Mutex<Inner<E>>,
    /// One lock per `AdvanceKind`, held around each advance of that kind.
    advancing: [Mutex<()>; 2],
}

impl<E: EthSpec> AdvancedHeadStateCache<E> {
    fn lock(&self) -> MutexGuard<'_, Inner<E>> {
        let _timer = metrics::start_timer(&metrics::ADVANCED_HEAD_STATE_LOCK_WAIT);
        self.inner.lock()
    }

    /// Drop retained states and reject stores from earlier generations.
    ///
    /// Does not wait for an advance that is already running. That advance may still finish and
    /// return a state to the caller that started it, but the result is not stored.
    pub fn invalidate(&self) {
        let mut inner = self.lock();
        inner.generation = inner.generation.wrapping_add(1);
        inner.partial = None;
        inner.complete = None;
        publish_gauges(&inner, None);
    }

    /// Update gauges from the current entries. `wall_clock_slot` sets the lag gauges.
    pub(crate) fn scrape_metrics(&self, wall_clock_slot: Option<Slot>) {
        let inner = self.lock();
        publish_gauges(&inner, wall_clock_slot);
    }

    /// Return a state for `head_block_root` advanced to exactly `request.target_slot`.
    ///
    /// Advances whose target is inside the retain window run one at a time per kind. A caller that
    /// waits for the advance lock finds the previous caller's result in the cache, and otherwise
    /// seeds from the retained entries itself. An advance past the retain window runs without the
    /// lock, because nothing it computes is kept for the target. `load_starting_state` must return a state at a slot
    /// `<= target_slot`. `advance` must move the state forward to the slot it is given. Both run
    /// inside the advance lock but outside the cache mutex.
    ///
    /// On a hit the returned state's slot is `target_slot`. The cache may retain a different slot:
    /// the furthest retained slot is `min(target_slot, max_store_slot)`.
    pub(crate) fn get_or_advance_to<Err, Load, Advance>(
        &self,
        request: AdvanceRequest,
        mut load_starting_state: Load,
        mut advance: Advance,
    ) -> Result<AdvancedHeadStateEntry<E>, Err>
    where
        Load: FnMut() -> Result<(Hash256, BeaconState<E>), Err>,
        Advance: FnMut(BeaconState<E>, Hash256, Slot) -> Result<(Hash256, BeaconState<E>), Err>,
    {
        let _advance_turn = (request.target_slot <= request.max_store_slot)
            .then(|| self.advancing[request.kind.index()].lock());
        let (generation, seed) = {
            let inner = self.lock();
            if let Some(hit) = exact_hit(
                &inner,
                request.kind,
                request.head_block_root,
                request.target_slot,
            ) {
                let entry = hit.clone();
                publish_gauges(&inner, Some(request.wall_clock_slot));
                drop(inner);
                metrics::inc_counter(&metrics::ADVANCED_HEAD_STATE_HITS);
                return Ok(entry);
            }
            let seed = select_seed(
                &inner,
                request.kind,
                request.head_block_root,
                request.target_slot,
            )
            .cloned();
            (inner.generation, seed)
        };

        metrics::inc_counter(&metrics::ADVANCED_HEAD_STATE_MISSES);
        let result = self.run_advance(
            &request,
            generation,
            seed,
            &mut load_starting_state,
            &mut advance,
        );
        publish_gauges(&self.lock(), Some(request.wall_clock_slot));
        result
    }

    fn run_advance<Err, Load, Advance>(
        &self,
        request: &AdvanceRequest,
        generation: u64,
        seed: Option<AdvancedHeadStateEntry<E>>,
        load_starting_state: &mut Load,
        advance: &mut Advance,
    ) -> Result<AdvancedHeadStateEntry<E>, Err>
    where
        Load: FnMut() -> Result<(Hash256, BeaconState<E>), Err>,
        Advance: FnMut(BeaconState<E>, Hash256, Slot) -> Result<(Hash256, BeaconState<E>), Err>,
    {
        let (mut state_root, mut state) = if let Some(seed) = seed {
            (seed.state_root, seed.state)
        } else {
            load_starting_state()?
        };

        // Never hand back a later slot. `advance` is required to reject `state.slot() > target`.
        if state.slot() > request.target_slot {
            let (root, state) = advance(state, state_root, request.target_slot)?;
            return Ok(AdvancedHeadStateEntry::new(
                request.head_block_root,
                root,
                state,
            ));
        }

        let store_ceiling = std::cmp::min(request.target_slot, request.max_store_slot);

        if state.slot() < store_ceiling {
            let before = state.slot();
            let (root, advanced) = advance(state, state_root, store_ceiling)?;
            record_slots_advanced(before, advanced.slot());
            state_root = root;
            state = advanced;
            self.try_store(
                generation,
                request.kind,
                AdvancedHeadStateEntry::new(request.head_block_root, state_root, state.clone()),
                request.max_store_slot,
            );
        } else if state.slot() == store_ceiling {
            // Already at the furthest slot we are allowed to retain (a disk load, or a seed).
            self.try_store(
                generation,
                request.kind,
                AdvancedHeadStateEntry::new(request.head_block_root, state_root, state.clone()),
                request.max_store_slot,
            );
        }

        if state.slot() < request.target_slot {
            let before = state.slot();
            let (root, advanced) = advance(state, state_root, request.target_slot)?;
            record_slots_advanced(before, advanced.slot());
            // Past the retain window: the caller gets the state, the cache does not keep it.
            debug!(
                kind = ?request.kind,
                head_block_root = ?request.head_block_root,
                target_slot = %request.target_slot,
                max_store_slot = %request.max_store_slot,
                "Computed advanced head state past the retain window"
            );
            state_root = root;
            state = advanced;
        }

        Ok(AdvancedHeadStateEntry::new(
            request.head_block_root,
            state_root,
            state,
        ))
    }

    fn try_store(
        &self,
        generation: u64,
        kind: AdvanceKind,
        entry: AdvancedHeadStateEntry<E>,
        max_store_slot: Slot,
    ) {
        if entry.state.slot() > max_store_slot {
            return;
        }
        if !block_root_matches(&entry) {
            metrics::inc_counter(&metrics::ADVANCED_HEAD_STATE_BLOCK_ROOT_MISMATCH);
            debug!(
                ?kind,
                head_block_root = ?entry.head_block_root,
                state_slot = %entry.state.slot(),
                "Refusing to retain advanced head state for a different block"
            );
            return;
        }

        let mut inner = self.lock();
        if inner.generation != generation {
            metrics::inc_counter(&metrics::ADVANCED_HEAD_STATE_STALE_STORE);
            debug!(
                ?kind,
                head_block_root = ?entry.head_block_root,
                state_slot = %entry.state.slot(),
                "Dropping advanced head state from a stale generation"
            );
            return;
        }
        let slot = match kind {
            AdvanceKind::Partial => &mut inner.partial,
            AdvanceKind::Complete => &mut inner.complete,
        };
        store(slot, entry);
    }
}

fn store<E: EthSpec>(
    slot: &mut Option<AdvancedHeadStateEntry<E>>,
    entry: AdvancedHeadStateEntry<E>,
) {
    let replace = match slot.as_ref() {
        None => true,
        Some(existing) if existing.head_block_root != entry.head_block_root => true,
        Some(existing) => entry.state.slot() >= existing.state.slot(),
    };
    if replace {
        *slot = Some(entry);
    }
}

fn selectable<E: EthSpec>(
    entry: &AdvancedHeadStateEntry<E>,
    head_block_root: Hash256,
    target_slot: Slot,
) -> bool {
    entry.head_block_root == head_block_root && entry.state.slot() <= target_slot
}

/// Furthest entry at or before `target_slot`.
///
/// A partial request may start from either kind. A complete request starts only from the complete
/// entry: a partial state must not seed block production.
fn select_seed<E: EthSpec>(
    inner: &Inner<E>,
    kind: AdvanceKind,
    head_block_root: Hash256,
    target_slot: Slot,
) -> Option<&AdvancedHeadStateEntry<E>> {
    let partial = inner
        .partial
        .as_ref()
        .filter(|entry| selectable(entry, head_block_root, target_slot));
    let complete = inner
        .complete
        .as_ref()
        .filter(|entry| selectable(entry, head_block_root, target_slot));
    match kind {
        AdvanceKind::Partial => match (partial, complete) {
            (Some(partial), Some(complete)) => {
                Some(if partial.state.slot() >= complete.state.slot() {
                    partial
                } else {
                    complete
                })
            }
            (Some(partial), None) => Some(partial),
            (None, Some(complete)) => Some(complete),
            (None, None) => None,
        },
        AdvanceKind::Complete => complete,
    }
}

fn exact_hit<E: EthSpec>(
    inner: &Inner<E>,
    kind: AdvanceKind,
    head_block_root: Hash256,
    target_slot: Slot,
) -> Option<&AdvancedHeadStateEntry<E>> {
    let matches = |entry: &AdvancedHeadStateEntry<E>| {
        entry.head_block_root == head_block_root && entry.state.slot() == target_slot
    };
    let partial = inner.partial.as_ref().filter(|entry| matches(entry));
    let complete = inner.complete.as_ref().filter(|entry| matches(entry));
    match kind {
        // Prefer the complete state when both sit on the target: its state root is real.
        AdvanceKind::Partial => complete.or(partial),
        AdvanceKind::Complete => complete,
    }
}

/// `get_latest_block_root` fills a zero `latest_block_header.state_root` before hashing.
///
/// Comparing `latest_block_header().canonical_root()` directly rejects the head state on its own
/// slot, where that field is still zero. After at least one slot of advance the header root is
/// the block root either way.
fn block_root_matches<E: EthSpec>(entry: &AdvancedHeadStateEntry<E>) -> bool {
    entry.state.get_latest_block_root(entry.state_root) == entry.head_block_root
}

fn record_slots_advanced(before: Slot, after: Slot) {
    let slots = after.as_u64().saturating_sub(before.as_u64());
    if slots > 0 {
        metrics::inc_counter_by(&metrics::ADVANCED_HEAD_STATE_SLOTS_ADVANCED, slots);
    }
}

fn publish_gauges<E: EthSpec>(inner: &Inner<E>, wall_clock_slot: Option<Slot>) {
    let wall = wall_clock_slot.map(|slot| slot.as_u64() as i64);
    publish_one(
        inner.partial.as_ref().map(|entry| entry.state.slot()),
        wall,
        &metrics::ADVANCED_HEAD_STATE_PARTIAL_SLOT,
        &metrics::ADVANCED_HEAD_STATE_PARTIAL_SLOT_LAG,
    );
    publish_one(
        inner.complete.as_ref().map(|entry| entry.state.slot()),
        wall,
        &metrics::ADVANCED_HEAD_STATE_COMPLETE_SLOT,
        &metrics::ADVANCED_HEAD_STATE_COMPLETE_SLOT_LAG,
    );
}

fn publish_one(
    slot: Option<Slot>,
    wall: Option<i64>,
    slot_gauge: &metrics::Result<metrics::IntGauge>,
    lag_gauge: &metrics::Result<metrics::IntGauge>,
) {
    match slot {
        Some(slot) => {
            let slot = slot.as_u64() as i64;
            metrics::set_gauge(slot_gauge, slot);
            if let Some(wall) = wall {
                metrics::set_gauge(lag_gauge, wall - slot);
            }
        }
        None => {
            metrics::set_gauge(slot_gauge, 0);
            if wall.is_some() {
                metrics::set_gauge(lag_gauge, 0);
            }
        }
    }
}
