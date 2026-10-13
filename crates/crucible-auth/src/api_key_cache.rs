use crate::{
    api_key_policy::ApiKeyPolicy, cached_api_key::CachedApiKey, lookup_outcome::LookupOutcome,
    lru_map::LruMap, refusal::Refusal,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, atomic::AtomicU64},
    time::SystemTime,
};
use tokio::sync::{OnceCell, Semaphore};

/// How many keys the last-used throttle remembers. Forgetting one only means
/// its next use is recorded early.
const LAST_USED_CAPACITY: usize = 10_000;

/// An `ApiKeyAuthenticator`'s mutable state. Valid and refused keys live in
/// separate LRUs so a flood of forged keys can only evict other refusals.
/// Lock order: `valid` before `refused`; `inflight` is never held with either.
pub(crate) struct ApiKeyCache {
    pub valid: Mutex<LruMap<CachedApiKey>>,
    pub refused: Mutex<LruMap<Refusal>>,
    pub used: Mutex<LruMap<SystemTime>>,
    /// Single flight: concurrent misses for one digest share one store read.
    pub inflight: Mutex<HashMap<String, Arc<OnceCell<LookupOutcome>>>>,
    /// Digests with a refresh-ahead read in flight, so a hit starts at most one.
    pub refreshing: Mutex<HashSet<String>>,
    pub lookups: Semaphore,
    /// Bumped (under `valid`) by every `forget`, so a read that started before
    /// a revocation cannot cache what it read.
    pub epoch: AtomicU64,
}

impl ApiKeyCache {
    pub fn new(policy: &ApiKeyPolicy) -> Self {
        Self {
            valid: Mutex::new(LruMap::new(policy.valid_capacity)),
            refused: Mutex::new(LruMap::new(policy.refused_capacity)),
            used: Mutex::new(LruMap::new(LAST_USED_CAPACITY)),
            inflight: Mutex::new(HashMap::new()),
            refreshing: Mutex::new(HashSet::new()),
            lookups: Semaphore::new(policy.lookup_concurrency.max(1)),
            epoch: AtomicU64::new(0),
        }
    }
}
