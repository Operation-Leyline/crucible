use std::collections::BTreeSet;

/// What a `filters` listing may show, as the Authorizer answers
/// [`Authorizer::visible`](super::Authorizer::visible).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Visibility {
    /// Every instance under the parent: no filtering needed.
    All,
    /// Only these instances (by the listed resource's own identifier): the
    /// handler filters to them.
    Only(BTreeSet<String>),
    /// The parent is outside the caller's visibility boundary: the
    /// not-found error (the framework's 404, or the service's own), as a
    /// Hidden verdict is answered. A listing of a root resource has no
    /// parent to hide, so there it is answered as `Denied`, as a Hidden
    /// verdict on a check that binds nothing is.
    Hidden,
    /// The caller can see the parent but may not list it at all (a
    /// suspended workspace, a suspended user): a 403, answered as a Denied
    /// verdict on the `filters` clause is, through the refusal factory with
    /// the clause's permission.
    Denied,
}

impl Visibility {
    /// Whether the instance named `id` may be shown.
    pub fn shows(&self, id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(id),
            Self::Hidden | Self::Denied => false,
        }
    }
}
