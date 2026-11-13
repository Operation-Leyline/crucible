use super::{PermissionInfo, ResourceRef};

/// A `filters p on R` listing, for [`Authorizer::visible`](super::Authorizer::visible):
/// which instances of `R` under the bound parent may the caller see, by
/// holding `permission` on them?
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Listing {
    pub permission: &'static PermissionInfo,
    /// The listed resource under its parents, as the request binds them;
    /// its own identifier is [`Target::All`](super::Target::All).
    pub resource: ResourceRef,
}

impl Listing {
    /// The listing of `resource` (`R(parents, *)`) by `permission`: what
    /// `Permissions` asks for a `filters` clause, for asking an Authorizer
    /// directly (`RoleAuthorizer::visible_for`, a test, a conformance run).
    pub fn new(permission: impl Into<&'static PermissionInfo>, resource: ResourceRef) -> Self {
        Self {
            permission: permission.into(),
            resource,
        }
    }
}
