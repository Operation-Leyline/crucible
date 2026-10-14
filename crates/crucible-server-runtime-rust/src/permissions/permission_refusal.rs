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

    /// The missing permission, whatever its tier: for logs and audit, never
    /// for the response. Use [`public_permission`](Self::public_permission)
    /// for what the caller may be told.
    pub fn permission(&self) -> &'static PermissionInfo {
        self.check.permission
    }

    /// The missing permission when a refusal may name it; `None` for an
    /// internal permission, which no response names.
    pub fn public_permission(&self) -> Option<&'static PermissionInfo> {
        Some(self.check.permission).filter(|permission| permission.is_public())
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
            resource_type: self.check.permission.resource.name,
            permission: self.public_permission().map(|permission| permission.value),
        }
        .into()
    }
}
