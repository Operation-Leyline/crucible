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
    /// The message of its 404 (`No such inbox.`), fixed so a hidden
    /// instance and a missing one read alike. Generated with the
    /// description, so answering costs nothing.
    pub not_found: &'static str,
}

impl ResourceInfo {
    /// The framework's 404 for this resource: what a hidden instance and a
    /// genuinely missing one both answer, so the two cannot be told apart.
    /// It names no instance, so a service that designates its own not-found
    /// error cannot build that from it: there it is a wiring error, and
    /// [`ResourceRef::not_found`](super::ResourceRef::not_found) is the one
    /// to use.
    pub fn not_found_error(&'static self) -> Error {
        FrameworkError::NotFound { resource: self }.into()
    }
}
