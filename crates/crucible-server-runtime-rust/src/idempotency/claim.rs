use super::{Fingerprint, StoredResponse};

/// What [`IdempotencyStore::claim`](super::IdempotencyStore::claim) found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Claim {
    /// No unexpired record held the key: this attempt holds it now, in
    /// flight, and runs the operation.
    Fresh,
    /// An earlier attempt with the key finished, and `response` is what it
    /// left: its success's output, or the note that there is none to replay
    /// (it failed past its point of no return, or its response was not
    /// kept). Its request had `fingerprint`.
    Replay {
        fingerprint: Fingerprint,
        response: StoredResponse,
    },
    /// An earlier attempt with the key holds it and has not finished; its
    /// request had `fingerprint`.
    InFlight { fingerprint: Fingerprint },
}
