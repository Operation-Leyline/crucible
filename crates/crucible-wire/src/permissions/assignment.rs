/// An `assigns <member> at ..` or `assigns loaded at ..` clause
/// (docs/roles.md §4.5), named so the handler can discharge it: generated
/// as the operation's `ASSIGNMENT` constant. The handler checks every role
/// the request grants or revokes through the Authorizer
/// (`roles::assign`) before the operation's point of no return; a success
/// with it still open fails, as one with an open `by handler` obligation
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Assignment {
    /// The operation's shape id.
    pub operation: &'static str,
    /// The input member path holding the roles granted (`roles`,
    /// `binding.roles`); `None` for `assigns loaded`, whose roles the
    /// handler reads from the stored binding.
    pub member: Option<&'static str>,
}
