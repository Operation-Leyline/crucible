use super::{RoleBinding, RoleInfo};
use crate::permissions::{PermissionInfo, ResourceRef};

/// A `by service` grant that may apply (docs/roles.md §4.6), for
/// [`BindingSource::decide_by_service`](super::BindingSource::decide_by_service):
/// the principal holds `binding` under the target's root, its `role` is
/// valid at the binding's scope, and the role grants `permission` only when
/// the service says so. The binding need not cover `target` (a graph
/// binding's grant on a history it is related to): whether its scope
/// instance reaches the target is exactly what the service decides.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ByService<'a> {
    pub role: &'static RoleInfo,
    pub binding: &'a RoleBinding,
    pub permission: &'static PermissionInfo,
    pub target: &'a ResourceRef,
}
