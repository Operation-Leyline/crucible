use super::Verdict;

/// What became of a `by handler` obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObligationOutcome {
    /// Not closed yet.
    Open,
    /// Checked through the Authorizer ([`discharge`](super::discharge)).
    Discharged(Verdict),
    /// Closed without a check, for this reason
    /// ([`not_applicable`](super::not_applicable)).
    NotApplicable(String),
    /// Still open when the operation succeeded: a handler bug. Tests and
    /// debug builds fail the request; release builds log it and keep the
    /// response, since whatever the handler did has happened.
    Undischarged,
    /// A replay: the operation did not run, and the handler checks its
    /// success was allowed were asked again before the replay was served.
    Replayed,
}
