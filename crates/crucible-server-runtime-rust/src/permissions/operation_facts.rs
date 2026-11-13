use super::{Assignment, Requirements};

/// What the model says about the operation being authorized, so an
/// Authorizer can decide what it may cache and what it reads fresh, without
/// a freshness marker in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OperationFacts {
    /// The operation's shape id: `pigeon.api#SendMessage`.
    pub id: &'static str,
    /// Whether the operation, as the API version the request is served at
    /// describes it, is read-only and idempotent.
    pub readonly: bool,
    pub idempotent: bool,
    /// Every clause: mode, permission, and whether it is conditional. At an
    /// older API version, the current version's, since requirements bind the
    /// current input.
    pub requirements: &'static Requirements,
    /// The `assigns` obligation the operation opens (docs/roles.md §4.5),
    /// if any: it grants or revokes roles, and its handler checks them with
    /// `roles::assign`.
    pub assigns: Option<Assignment>,
}
