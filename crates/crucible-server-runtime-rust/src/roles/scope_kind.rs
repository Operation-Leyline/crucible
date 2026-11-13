/// What a scope's instance names (`scope .. on ..`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// `on service`: the whole service; an instance names no identifier.
    Service,
    /// `on R`: one instance of `R`, named by all its identifiers.
    Instance,
    /// `on R(*)`: every instance of `R` under one parent instance, those
    /// created later included, named by the parent's identifiers.
    Every,
}
