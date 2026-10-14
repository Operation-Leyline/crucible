use super::{Check, Origin, PermissionInfo, ResourceRef};
use crate::{Error, FrameworkError};

/// A Denied verdict, for a service that answers it with its own error
/// ([`Permissions::refusal_factory`](super::Permissions::refusal_factory)).
/// It is also left on the request context as a typed attribute. A Hidden
/// verdict never becomes one: it is always the framework's 404.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRefusal {
    check: Check,
}

impl PermissionRefusal {
    pub(crate) fn new(check: Check) -> Self {
        Self { check }
    }

    /// The missing permission when a response may name it; `None` for an
    /// internal permission, which no response names. What a refusal factory
    /// builds its error from.
    pub fn public_permission(&self) -> Option<&'static PermissionInfo> {
        Some(self.check.permission).filter(|permission| permission.is_public())
    }

    /// The missing permission whatever its tier, internal ones included:
    /// for logs and audit only, never for a response.
    pub fn permission_for_logging(&self) -> &'static PermissionInfo {
        self.check.permission
    }

    /// The instance the caller lacks it on. Its identifiers came from the
    /// request, so echoing them reveals nothing the caller did not send.
    pub fn resource(&self) -> &ResourceRef {
        &self.check.resource
    }

    /// The check that was refused: declared, an obligation, or a handler's.
    pub fn origin(&self) -> Origin {
        self.check.origin
    }

    /// What a refusal is answered with when the service gives no factory: a
    /// 403 [`FrameworkError::PermissionDenied`] (`ForbiddenException`) whose
    /// message names the permission's value and resource type, unless the
    /// permission is internal.
    pub fn default_error(&self) -> Error {
        FrameworkError::PermissionDenied {
            permission: self.check.permission,
        }
        .into()
    }
}
