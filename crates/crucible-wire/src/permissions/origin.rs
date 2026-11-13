/// Where a [`Check`](super::Check) came from, for the audit record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Origin {
    /// A clause of the operation's requirements, by position.
    Declared(u16),
    /// A `by handler` clause the handler discharged, by position.
    Obligation(u16),
    /// A check the handler made that the model does not declare: a
    /// permission the request chooses, such as one being delegated.
    Handler,
    /// An `assigns` obligation the handler discharged and the Authorizer
    /// refused (docs/roles.md §4.5): the caller may not grant or revoke a
    /// role there. A refusal names the operation's first declared check,
    /// whose 403 it is answered with, under this origin.
    Assignment,
}
