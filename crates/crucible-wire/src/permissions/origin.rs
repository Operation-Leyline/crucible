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
}
