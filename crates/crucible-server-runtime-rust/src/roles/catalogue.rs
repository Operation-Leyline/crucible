use std::ops::Range;

use super::{Grant, PermissionSet, RoleInfo, ScopeInfo};
use crate::permissions::{PermissionInfo, ResourceInfo};

/// A service's roles and scopes (docs/roles.md §7.1), as generated code
/// states them: `roles::CATALOGUE`. Everything the runtime's
/// `RoleAuthorizer` decides from is here, settled at build time; the
/// generated `Role` and `Scope` enums read it through the methods below,
/// each a table read.
///
/// Roles and scopes are addressed by their index, which is the generated
/// enums' order (`Role::ALL`, the service's `roles [..]` list).
#[derive(Debug, PartialEq, Eq)]
pub struct RoleCatalogue {
    /// In catalogue order.
    pub roles: &'static [RoleInfo],
    /// In the service's `scopes [..]` order.
    pub scopes: &'static [ScopeInfo],
    /// The permissions a [`PermissionSet`] of this catalogue names, by bit:
    /// the generated `Permission::ALL` (by resource shape id, then
    /// declaration order). A resource's permissions are contiguous.
    pub permissions: &'static [&'static PermissionInfo],
    /// The values of the service's `principals` enum (wire values), in
    /// declaration order; empty when it names none.
    pub principals: &'static [&'static str],
    /// `roles [..] custom`: a binding may name a role the model does not
    /// declare, whose permissions the service supplies.
    pub custom: bool,
    /// What a custom role may grant: the permissions that are neither
    /// internal nor `@notGrantable` (docs/roles.md §4.7).
    pub grantable: PermissionSet,
}

impl RoleCatalogue {
    /// The role whose value is `value`; `None` for one the model does not
    /// declare (an old binding's, or a custom role's).
    pub fn find_role(&self, value: &str) -> Option<usize> {
        self.roles.iter().position(|role| role.value == value)
    }

    /// The scope whose value is `value`.
    pub fn find_scope(&self, value: &str) -> Option<usize> {
        self.scopes.iter().position(|scope| scope.value == value)
    }

    /// The bit of `permission` in this catalogue's sets; `None` for a
    /// permission of another model, which no role grants.
    #[inline]
    pub fn bit(&self, permission: &PermissionInfo) -> Option<usize> {
        // Generated checks name the generated statics, so the pointer
        // usually answers; the value pair is for a description built
        // elsewhere.
        self.permissions
            .iter()
            .position(|p| std::ptr::eq(*p, permission))
            .or_else(|| {
                self.permissions.iter().position(|p| {
                    p.resource.name == permission.resource.name && p.value == permission.value
                })
            })
    }

    /// What `role` grants of `permission`.
    pub fn allows(&self, role: usize, permission: &PermissionInfo) -> Grant {
        let (Some(info), Some(bit)) = (self.roles.get(role), self.bit(permission)) else {
            return Grant::No;
        };
        if info.grants.contains(bit) {
            Grant::Yes
        } else if info.by_service.contains(bit) {
            Grant::ByService
        } else {
            Grant::No
        }
    }

    /// Whether a binding of `role` at `scope`, held by a principal of kind
    /// `holder` (a value of the `principals` enum; `None` when the service
    /// cannot say), grants anything: the role binds at that scope, and the
    /// kind may hold it. A role that names no holders is for every kind.
    pub fn valid_for(&self, role: usize, scope: usize, holder: Option<&str>) -> bool {
        let Some(info) = self.roles.get(role) else {
            return false;
        };
        info.scopes.iter().any(|s| usize::from(*s) == scope)
            && (info.holders.is_empty() || holder.is_some_and(|h| info.holders.contains(&h)))
    }

    /// Whether a principal holding `assigners` may grant or revoke `role`:
    /// the role names no assigners, or one of them is held. Whether the
    /// assigner's binding covers the binding's scope is the caller's to
    /// check, with [`ScopeInfo::covers`].
    pub fn may_assign(&self, assigners: impl IntoIterator<Item = usize>, role: usize) -> bool {
        let Some(info) = self.roles.get(role) else {
            return false;
        };
        info.assigned_by.is_empty()
            || assigners
                .into_iter()
                .any(|a| info.assigned_by.iter().any(|b| usize::from(*b) == a))
    }

    /// The bits of `resource`'s permissions (contiguous); empty for a
    /// resource the catalogue does not know.
    pub fn bits_of(&self, resource: &ResourceInfo) -> Range<usize> {
        let on = |p: &&PermissionInfo| {
            std::ptr::eq(p.resource, resource) || p.resource.name == resource.name
        };
        match self.permissions.iter().position(on) {
            Some(start) => {
                let len = self.permissions[start..]
                    .iter()
                    .take_while(|p| on(p))
                    .count();
                start..start + len
            }
            None => 0..0,
        }
    }

    /// The set of `permissions`, for a custom role's definition; one of
    /// another model is left out.
    pub fn permission_set<'p>(
        &self,
        permissions: impl IntoIterator<Item = &'p PermissionInfo>,
    ) -> PermissionSet {
        PermissionSet::with_bits(permissions.into_iter().filter_map(|p| self.bit(p)))
    }

    /// The permissions `set` holds.
    pub fn permissions_in<'s>(
        &'s self,
        set: &'s PermissionSet,
    ) -> impl Iterator<Item = &'static PermissionInfo> + 's {
        set.bits()
            .filter_map(|bit| self.permissions.get(bit).copied())
    }
}
