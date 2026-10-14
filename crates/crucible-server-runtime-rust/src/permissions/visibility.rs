use std::collections::BTreeSet;

/// What a `filters` listing may show, as the Authorizer answers
/// [`Authorizer::visible`](super::Authorizer::visible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    /// Every instance under the parent: no filtering needed.
    All,
    /// Only these instances (by the listed resource's own identifier): the
    /// handler filters to them.
    Only(BTreeSet<String>),
    /// The parent is outside the caller's visibility boundary: the
    /// framework's 404.
    Hidden,
}

impl Visibility {
    /// Whether the instance named `id` may be shown.
    pub fn shows(&self, id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(id),
            Self::Hidden => false,
        }
    }
}
