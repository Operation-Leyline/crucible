use crate::permissions::{ResourceRef, Target};

/// The tenant a check's target lies in, which
/// [`BindingSource::bindings`](super::BindingSource::bindings) reads a
/// principal's bindings under: the target's first identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Root<'a> {
    /// The target names no instance (`Organization(*)`): only a binding
    /// whose scope instance names no identifier (`on service`, or `on
    /// R(*)` over a root resource) lies under it.
    Service,
    /// The target lies in the root instance `id`, named by the identifier
    /// `identifier` (`organizationId`).
    Instance {
        identifier: &'static str,
        id: &'a str,
    },
}

impl<'a> Root<'a> {
    /// The root of `target`.
    pub fn of(target: &'a ResourceRef) -> Self {
        match (
            target.identifiers().first(),
            target.resource().identifiers.first(),
        ) {
            (Some(Target::One(id)), Some(identifier)) => Self::Instance { identifier, id },
            _ => Self::Service,
        }
    }

    /// Whether a binding whose scope instance names `identifiers` (root to
    /// leaf) lies under this root: its first identifier is the root's, or
    /// it names none. A source may return exactly those; `RoleAuthorizer`
    /// ignores any other it returns.
    pub fn holds(&self, identifiers: &[String]) -> bool {
        match (self, identifiers.first()) {
            (_, None) => true,
            (Self::Service, Some(_)) => false,
            (Self::Instance { id, .. }, Some(first)) => first == id,
        }
    }
}
