use super::RequirementInfo;

/// What an operation requires of its caller (`Operation::REQUIREMENTS`):
/// the IR's `Requirements`, with each clause's bindings and condition left
/// to `Operation::CHECKS`.
#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub enum Requirements {
    /// The model says nothing. The compiler refuses that once a service
    /// declares any permission, so at runtime it means bindings generated
    /// before the service declared permissions (or a public operation); the
    /// `Permissions` interceptor refuses an authenticated one as a wiring
    /// error unless told otherwise.
    #[default]
    Undeclared,
    /// `requires none`: authenticated, and no permission is needed.
    None,
    /// `requires pending because "<reason>"`: not declared yet, and the
    /// handler still does its own checking. Passed through, with the reason
    /// recorded.
    Pending { reason: &'static str },
    /// Every clause must hold. Source order, which is the refusal order.
    List(&'static [RequirementInfo]),
}
