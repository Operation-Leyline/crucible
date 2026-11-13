use std::fmt;

/// One role a principal holds at one scope instance, as the service stores
/// it: the role's value, the scope's value, and the identifiers of the
/// scope instance, root to leaf (`graphReader` at `graph(o-1, g-1)`).
/// Crucible stores none; a `BindingSource` reads them, and the
/// `AuthorizationRecord` names the one that allowed a check.
///
/// The values are strings, not the generated enums: a stored binding may
/// name a role or scope the model no longer has (it then grants nothing),
/// or a custom role.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RoleBinding {
    /// The role's value: `graphReader`, or a custom role's id.
    pub role: String,
    /// The scope's value: `allGraphs`.
    pub scope: String,
    /// The scope instance's identifiers, root to leaf: those of the scope's
    /// resource for `on R`, of its parent for `on R(*)`, none for `on
    /// service`. A count that differs from the scope's covers nothing.
    pub identifiers: Vec<String>,
}

impl RoleBinding {
    pub fn new<I>(role: impl Into<String>, scope: impl Into<String>, identifiers: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Self {
            role: role.into(),
            scope: scope.into(),
            identifiers: identifiers.into_iter().map(Into::into).collect(),
        }
    }
}

/// `graphReader at allGraphs(o-1)`, for logs and audit. The values come
/// from the service's store, not the request.
impl fmt::Display for RoleBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}(", self.role, self.scope)?;
        for (index, id) in self.identifiers.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(id)?;
        }
        f.write_str(")")
    }
}
