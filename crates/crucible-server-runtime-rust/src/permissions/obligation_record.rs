use super::{Obligation, ObligationOutcome};

/// A `by handler` obligation of this request, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObligationRecord {
    pub obligation: Obligation,
    pub outcome: ObligationOutcome,
}
