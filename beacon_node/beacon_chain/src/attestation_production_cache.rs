//! Single-flight cache for the expensive stale-head path of attestation production.
//!
//! When the head epoch differs from the request epoch, producing attestation data requires a
//! `partial_state_advance` through every empty epoch up to the request (to obtain
//! `current_justified_checkpoint` and build the committee cache). During prolonged outages,
//! validator clients / Charon retry `attestation_data` many times per duty; without dedup each
//! call replays that advance.
//!
//! This cache is deliberately narrower than the deleted `AttesterCache` (#8469):
//! - Used **only** on the slow path (`head_epoch != request_epoch`).
//! - Mutex is held solely for LRU lookup / insert of an `OnceCell`; computation runs outside
//!   the lock via `OnceCell::get_or_try_init` (no write-lock-during-advance contention).
//! - Not touched by block processing or finalization.
//! - Tiny LRU (capacity 4); keyed by `(request_epoch, beacon_block_root)`.

use hashlink::lru_cache::LruCache;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::Arc;
use tracing::debug;
use types::{
    BeaconStateError, Checkpoint, CommitteeIndex, Epoch, Hash256, Slot, state::CommitteeCache,
};

use crate::BeaconChainError;

/// Small enough for the rare stale-head case; large enough to cover a few concurrent request
/// epochs while the head is lagging.
const CACHE_SIZE: usize = 4;

/// Cached results of advancing a state to `request_epoch` from `beacon_block_root`.
#[derive(Clone)]
pub struct AttestationProductionCacheEntry {
    pub justified_checkpoint: Checkpoint,
    pub committee_cache: Arc<CommitteeCache>,
}

impl AttestationProductionCacheEntry {
    pub fn new(justified_checkpoint: Checkpoint, committee_cache: Arc<CommitteeCache>) -> Self {
        Self {
            justified_checkpoint,
            committee_cache,
        }
    }

    /// Committee length for `(slot, committee_index)` within the cached epoch.
    pub fn committee_len(
        &self,
        slot: Slot,
        committee_index: CommitteeIndex,
    ) -> Result<usize, BeaconChainError> {
        self.committee_cache
            .get_beacon_committee(slot, committee_index)
            .map(|committee| committee.committee.len())
            .ok_or_else(|| {
                BeaconChainError::BeaconStateError(BeaconStateError::NoCommittee {
                    slot,
                    index: committee_index,
                })
            })
    }
}

/// LRU of single-flight cells keyed by `(request_epoch, beacon_block_root)`.
pub struct AttestationProductionCache {
    cache: LruCache<(Epoch, Hash256), Arc<OnceCell<AttestationProductionCacheEntry>>>,
}

impl Default for AttestationProductionCache {
    fn default() -> Self {
        Self {
            cache: LruCache::new(CACHE_SIZE),
        }
    }
}

impl AttestationProductionCache {
    /// Returns the `OnceCell` for `(epoch, beacon_block_root)`, inserting an empty one on miss.
    ///
    /// Callers initialise via `get_or_try_init` **without** holding this cache's mutex.
    pub fn get_or_insert_key(
        &mut self,
        epoch: Epoch,
        beacon_block_root: Hash256,
    ) -> Arc<OnceCell<AttestationProductionCacheEntry>> {
        let key = (epoch, beacon_block_root);
        self.cache
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }
}

/// Look up or compute the justified checkpoint + committee cache for a stale-head attestation.
///
/// `compute` runs at most once per `(request_epoch, beacon_block_root)` across concurrent callers.
pub fn with_attestation_production_cache<E>(
    cache: &Mutex<AttestationProductionCache>,
    request_epoch: Epoch,
    beacon_block_root: Hash256,
    compute: impl FnOnce() -> Result<AttestationProductionCacheEntry, E>,
) -> Result<AttestationProductionCacheEntry, E>
where
    E: From<BeaconChainError>,
{
    let cache_entry = cache
        .lock()
        .get_or_insert_key(request_epoch, beacon_block_root);

    let entry = cache_entry.get_or_try_init(|| {
        debug!(
            %request_epoch,
            ?beacon_block_root,
            "Attestation production cache miss"
        );
        compute()
    })?;

    Ok(entry.clone())
}
