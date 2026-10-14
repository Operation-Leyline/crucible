use super::{Mode, PermissionInfo};

/// One clause of an operation's requirements, in source order (which is
/// the order refusals are chosen in). Its bindings and condition are not
/// data here: `Operation::CHECKS` evaluates them against the typed input.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct RequirementInfo {
    pub mode: Mode,
    /// The primary permission, never an alias.
    pub permission: &'static PermissionInfo,
    /// Whether the clause has a `when`, so it may produce no check for a
    /// request. Any other `Check` or `Filter` clause produces exactly one.
    pub conditional: bool,
}
