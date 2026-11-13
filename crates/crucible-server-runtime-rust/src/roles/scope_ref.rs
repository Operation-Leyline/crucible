use super::RoleBinding;

/// One scope instance, borrowed: the scope's value and the instance's
/// identifiers, root to leaf (`graph(o-1, g-1)`). What a binding is made
/// at, and what
/// [`RoleCatalogue::may_assign`](super::RoleCatalogue::may_assign) asks an
/// assigner's bindings to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeRef<'a> {
    pub scope: &'a str,
    pub identifiers: &'a [String],
}

impl<'a> ScopeRef<'a> {
    pub fn new(scope: &'a str, identifiers: &'a [String]) -> Self {
        Self { scope, identifiers }
    }

    /// Where `binding` is made.
    pub fn of(binding: &'a RoleBinding) -> Self {
        Self::new(&binding.scope, &binding.identifiers)
    }
}
