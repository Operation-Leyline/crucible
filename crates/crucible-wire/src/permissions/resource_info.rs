use crate::error::{Error, FrameworkError};

/// A resource that declares permissions, as authorization names it.
/// Generated, `'static`: the IR's `ResourcePermissions` without its
/// permission list.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ResourceInfo {
    /// The authorization name: the shape name in lowerCamelCase (`inbox`,
    /// `apiKey`), or what `@authorizationName` says. Grants store it, so it
    /// is stable data.
    pub name: &'static str,
    /// The identifiers that name one instance, root to leaf:
    /// `["workspaceId", "inboxId"]`.
    pub identifiers: &'static [&'static str],
}

impl ResourceInfo {
    /// The framework's 404 for this resource: what a hidden instance and a
    /// genuinely missing one both answer, so the two cannot be told apart.
    pub fn not_found(&self) -> Error {
        FrameworkError::NotFound {
            resource_type: self.name,
        }
        .into()
    }
}
