use super::Requirements;

/// What the model says about the operation being authorized, so an
/// Authorizer can decide what it may cache and what it reads fresh, without
/// a freshness marker in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OperationFacts {
    /// The operation's shape id: `pigeon.api#SendMessage`.
    pub id: &'static str,
    pub readonly: bool,
    pub idempotent: bool,
    /// Every clause: mode, permission, and whether it is conditional.
    pub requirements: &'static Requirements,
}
