use super::PermissionInfo;

/// A `requires p on R by handler` clause, named so the handler can
/// discharge it (generated as a constant beside the operation). The handler
/// checks it through the Authorizer, or says it does not apply to the
/// request, before the operation's point of no return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Obligation {
    /// The operation's shape id.
    pub operation: &'static str,
    /// The clause's position in the operation's requirements.
    pub index: u16,
    pub permission: &'static PermissionInfo,
}
