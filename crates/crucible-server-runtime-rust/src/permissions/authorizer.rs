use super::{AuthorizationRequest, Check, Listing, Verdict, Visibility};
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
            listing.permission.value, listing.resource
        )))
    }

    // Reserved: a later provided method for per-element batch checks, with
    // correlation ids and partial results (docs/permissions.md, decision 7).
    // It will have a default, so adding it breaks no implementation.
}
