use std::fmt;

use super::ResourceRef;
use crate::error::FrameworkError;

/// A resource that is not there, or not there for this caller, by the
/// instance the request named: what [`ResourceRef::not_found`] returns.
/// The server answers it as the framework's [`FrameworkError::NotFound`],
/// or, when the model designates the service's own not-found error, as what
/// the service's not-found factory builds from [`reference`](Self::reference),
/// so a handler's genuine miss reads as a hidden resource does either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingResource {
    reference: ResourceRef,
}

impl MissingResource {
    pub fn new(reference: ResourceRef) -> Self {
        Self { reference }
    }

    /// The instance that is missing, its identifiers as the caller named
    /// them.
    pub fn reference(&self) -> &ResourceRef {
        &self.reference
    }

    /// The framework's 404 for it, naming only the resource type.
    pub fn framework_error(&self) -> FrameworkError {
        FrameworkError::NotFound {
            resource: self.reference.resource(),
        }
    }
}

/// The resource's fixed message: never an identifier.
impl fmt::Display for MissingResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reference.resource().not_found)
    }
}

impl std::error::Error for MissingResource {}
