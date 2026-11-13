use super::{AuthorizationRequest, Check, Listing, Verdict, Visibility};
use crate::roles::{RoleBinding, ScopeRef};
use crate::{Error, Result};

/// Decides whether the caller holds permissions: the service's grants,
/// roles and memberships, which Crucible never stores. [`Permissions`]
/// calls it for an operation's declared checks, and handlers through
/// [`require`], [`decide`] and [`discharge`].
///
/// An `Err` means "could not tell" (the store is unreachable): the request
/// is answered with the service's sanitized internal error, never refused
/// and never allowed, and the framework caches nothing.
///
/// [`Permissions`]: super::Permissions
/// [`require`]: super::require
/// [`decide`]: super::decide
/// [`discharge`]: super::discharge
#[async_trait::async_trait]
pub trait Authorizer: Send + Sync + 'static {
    /// Every check of one call, at once, so a store can batch them (two
    /// declared checks become one read). One verdict per check, in order.
    /// The principal is `request.context.attribute::<..>()`, as the
    /// service's authentication left it; `request.operation` says whether
    /// the operation is read-only, for what may be served from a cache.
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>>;

    /// What a `filters` listing may show under its bound parent. A service
    /// without `filters` clauses need not answer it: by default it is an
    /// internal error.
    async fn visible(
        &self,
        request: &AuthorizationRequest<'_>,
        listing: &Listing,
    ) -> Result<Visibility> {
        let _ = request;
        Err(Error::internal(format!(
            "the Authorizer answers no `filters` listing, and {} on {} needs one",
            listing.permission.value,
            super::loggable(&listing.resource)
        )))
    }

    /// [`decide`](Self::decide), also naming the role binding that
    /// allowed each allowed check, for [`Decision::allowed_by`] in the
    /// `AuthorizationRecord`: push one entry per check onto `allowed_by`,
    /// in order (`None` where there is nothing to name), or none at all.
    /// The default asks `decide` and names nothing; `RoleAuthorizer`
    /// (the `roles` feature) names its bindings. `Permissions` calls this
    /// one, so an implementation that overrides it must answer as its
    /// `decide` would.
    ///
    /// [`Decision::allowed_by`]: super::Decision::allowed_by
    async fn decide_naming_bindings(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
        allowed_by: &mut Vec<Option<RoleBinding>>,
    ) -> Result<Vec<Verdict>> {
        let _ = allowed_by;
        self.decide(request, checks).await
    }

    /// Whether the caller may grant or revoke each of `roles` (role
    /// values, as bindings store them) at the scope instance `at`: what
    /// discharges an `assigns` obligation (docs/roles.md §4.5,
    /// `roles::assign`). One answer per role, in order. The default is the
    /// internal error: an Authorizer that judges no assignment cannot
    /// discharge one, and a model with `assigns` clauses needs one that
    /// does. `RoleAuthorizer` (the `roles` feature) answers from the
    /// catalogue's `assignedBy` and the caller's bindings.
    async fn may_assign(
        &self,
        request: &AuthorizationRequest<'_>,
        roles: &[&str],
        at: ScopeRef<'_>,
    ) -> Result<Vec<bool>> {
        let _ = (roles, at);
        Err(Error::internal(format!(
            "the Authorizer judges no role assignment, and {} assigns roles: use RoleAuthorizer, or implement Authorizer::may_assign",
            request.operation.id
        )))
    }

    // Reserved: a later provided method for per-element batch checks, with
    // correlation ids and partial results (docs/permissions.md, decision 7).
    // It will have a default, so adding it breaks no implementation.
}
