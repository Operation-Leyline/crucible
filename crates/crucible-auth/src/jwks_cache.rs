use crate::signing_key::SigningKey;
use std::{collections::BTreeMap, time::Instant};

#[derive(Default)]
pub(crate) struct JwksCache {
    pub keys: BTreeMap<String, SigningKey>,
    pub fetched: Option<Instant>,
    pub last_attempt: Option<Instant>,
    /// Configuration-owned keys are replaced by rebuilding the authenticator, never by fetching.
    pub pinned: bool,
}
