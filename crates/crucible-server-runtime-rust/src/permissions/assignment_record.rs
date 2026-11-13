use super::{Assignment, ObligationOutcome};

/// The operation's `assigns` obligation (docs/roles.md §4.5), and what
/// became of it: which roles the handler asked to grant or revoke, where,
/// and whether the Authorizer let the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AssignmentRecord {
    pub assignment: Assignment,
    /// `Open` until `roles::assign` closes it; `Discharged(Allowed)` when
    /// the caller may assign every role, `Discharged(Denied)` when it may
    /// not assign one; `Undischarged` when the operation succeeded with it
    /// open; `Replayed` for a replay. Never `NotApplicable`: an operation
    /// that assigns nothing says so in the model (`assigns none`), and one
    /// that grants no role this time passes none.
    pub outcome: ObligationOutcome,
    /// Each role value the handler passed, with whether the caller may
    /// grant or revoke it there. The values come from the request or the
    /// service's store.
    pub roles: Vec<(String, bool)>,
    /// The scope instance the binding is written at: the scope's value and
    /// its identifiers, root to leaf.
    pub at: Option<(String, Vec<String>)>,
}
