use crate::{lru_map::LruMap, signing_key::SigningKey};
use std::{collections::BTreeMap, time::Instant};

/// Key IDs remembered as missing from a fetched key set. Forgetting one only
/// lets it cause one more fetch, still under the global floor.
pub(crate) const ABSENT_CAPACITY: usize = 1024;

pub(crate) struct JwksCache {
    pub keys: BTreeMap<String, SigningKey>,
    pub fetched: Option<Instant>,
    pub last_attempt: Option<Instant>,
    /// Whether the last fetch attempt failed: an outage holds every fetch
    /// off for the full refresh interval.
    pub failed: bool,
    /// When a successful fetch was found not to contain each key ID. Such a
    /// `kid` waits the full refresh interval before it may cause another
    /// fetch; one not seen yet waits only for the short floor, so a token
    /// with a forged `kid` cannot hold a rotated key off for the interval.
    pub absent: LruMap<Instant>,
    /// Configuration-owned keys are replaced by rebuilding the authenticator, never by fetching.
    pub pinned: bool,
}

impl Default for JwksCache {
    fn default() -> Self {
        Self {
            keys: BTreeMap::new(),
            fetched: None,
            last_attempt: None,
            failed: false,
            absent: LruMap::new(ABSENT_CAPACITY),
            pinned: false,
        }
    }
}
