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
    /// Still open when the operation succeeded: a handler bug, which fails
    /// the request unless `Permissions::assert_obligations(false)` opted out
    /// (or the interceptor only shadows); then it is logged and the response
    /// kept, since whatever the handler did has happened.
    Undischarged,
    /// A replay: the operation did not run, and the handler checks its
    /// success was allowed were asked again before the replay was served.
    Replayed,
}
