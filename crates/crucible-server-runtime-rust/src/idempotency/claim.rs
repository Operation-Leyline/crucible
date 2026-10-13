use super::{Fingerprint, StoredResponse};

/// What [`IdempotencyStore::claim`](super::IdempotencyStore::claim) found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// No unexpired record held the key: this attempt holds it now, in
    /// flight, and runs the operation.
    Fresh,
    /// An earlier attempt with the key succeeded; its request had
    /// `fingerprint`.
    Replay {
        fingerprint: Fingerprint,
        response: StoredResponse,
    },
    /// An earlier attempt with the key holds it and has not finished; its
    /// request had `fingerprint`.
    InFlight { fingerprint: Fingerprint },
}
