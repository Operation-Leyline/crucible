use super::{PermissionSet, RoleCatalogue};
use crate::permissions::PermissionInfo;

/// What a principal may do at one resource instance
/// ([`RoleAuthorizer::capabilities`](super::RoleAuthorizer::capabilities)):
/// the permissions of the target's resource that a check on it would
/// allow, `by service` grants answered by the service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    pub catalogue: &'static RoleCatalogue,
    /// The allowed permissions, all of the target's resource.
    pub allowed: PermissionSet,
    /// Whether some binding whose role is valid where it is bound covers
    /// the target: what a refusal there would be chosen by.
    pub covered: bool,
}

impl Capabilities {
    /// Whether a check of `permission` on the target would be allowed.
    pub fn allows(&self, permission: &PermissionInfo) -> bool {
        self.catalogue
            .bit(permission)
            .is_some_and(|bit| self.allowed.contains(bit))
    }

    /// The allowed permissions, in catalogue order.
    pub fn permissions(&self) -> impl Iterator<Item = &'static PermissionInfo> + '_ {
        self.catalogue.permissions_in(&self.allowed)
    }
}
