use super::{Obligation, Origin, PermissionInfo, ResourceRef};

/// One question for the Authorizer: does the caller hold `permission` on
/// `resource`? `Operation::CHECKS` produces one per declared clause that
/// applies to the request; handlers make their own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Check {
    pub permission: &'static PermissionInfo,
    pub resource: ResourceRef,
    pub origin: Origin,
}

impl Check {
    /// The check of clause `index` of the operation's requirements, whose
    /// permission is `permission`, bound to `resource` from the input.
    pub fn declared(
        index: u16,
        permission: &'static PermissionInfo,
        resource: ResourceRef,
    ) -> Self {
        Self {
            permission,
            resource,
            origin: Origin::Declared(index),
        }
    }

    /// A check the handler makes that the model does not declare.
    pub fn handler(permission: impl Into<&'static PermissionInfo>, resource: ResourceRef) -> Self {
        Self {
            permission: permission.into(),
            resource,
            origin: Origin::Handler,
        }
    }

    /// The check that discharges `obligation` against `resource`.
    pub fn obligation(obligation: &Obligation, resource: ResourceRef) -> Self {
        Self {
            permission: obligation.permission,
            resource,
            origin: Origin::Obligation(obligation.index),
        }
    }
}
