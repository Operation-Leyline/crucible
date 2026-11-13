use std::sync::Arc;

use super::{ByService, PermissionSet, Refusal, RoleBinding, Root, Unavailable};
use crate::Context;
use crate::permissions::{Listing, Visibility};

/// What only the service knows, for [`RoleAuthorizer`](super::RoleAuthorizer):
/// who the caller is, the role bindings it holds, and the answers to the
/// hatches the model leaves to the service (docs/roles.md §7.2). Crucible
/// stores nothing; storage, caching, freshness and tenancy stay here.
///
/// Every method that reads a store answers [`Unavailable`] when it cannot
/// tell, and `RoleAuthorizer` then answers the whole batch as an error:
/// there is no path that allows on error.
///
/// Its methods are `async fn`s in the implementation (no `async_trait`):
/// `RoleAuthorizer` is generic over the source, so their futures are not
/// boxed per request. Each must be `Send`.
pub trait BindingSource: Send + Sync + 'static {
    /// The caller, as the service's authentication leaves it on the request.
    type Principal: Clone + Send + Sync + 'static;

    /// The caller of the request: by default the context's
    /// `Self::Principal` attribute. `None` is a wiring error (an
    /// authenticated operation reached authorization with no principal),
    /// answered as the internal error, never an allow.
    fn principal(&self, context: &Context) -> Option<Self::Principal> {
        context.attribute::<Self::Principal>()
    }

    /// The principal's kind, as a wire value of the model's `principals`
    /// enum (`user`), against which roles' `holders [..]` are checked.
    /// `None` (the default): the service cannot say, so a role limited to
    /// some kinds grants it nothing.
    fn kind<'p>(&self, principal: &'p Self::Principal) -> Option<&'p str> {
        let _ = principal;
        None
    }

    /// Every binding `principal` holds under `root` (its tenant), in one
    /// read: `RoleAuthorizer` asks once per root per batch of checks.
    /// Bindings at scopes that name no identifier (`on service`) lie under
    /// every root. Returning more is harmless (only those `root` holds are
    /// read); returning fewer refuses what they would allow. `fresh` asks
    /// for no cached answer: the operation is not read-only.
    ///
    /// An `Arc`, so a source that caches a principal's bindings hands out
    /// the cached ones without copying them per request.
    fn bindings(
        &self,
        principal: &Self::Principal,
        root: Root<'_>,
        fresh: bool,
    ) -> impl Future<Output = Result<Arc<[RoleBinding]>, Unavailable>> + Send;

    /// Whether a `by service` grant applies (docs/roles.md §4.6): the
    /// binding's role grants the permission on the target only when the
    /// service says so, such as a graph administrator reading the history
    /// of a graph its binding covers while the graph is live. Asked for
    /// every binding under the root whose role is valid at its scope and
    /// grants the permission `by service`, whether or not its scope covers
    /// the target, since the relationship it follows is not the resource
    /// tree. Only asked when no binding grants the permission outright.
    ///
    /// The default refuses: a model with a `by service` grant whose source
    /// does not answer it gets no allow from it.
    fn decide_by_service(
        &self,
        principal: &Self::Principal,
        grant: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send {
        let _ = (principal, grant);
        std::future::ready(Ok(false))
    }

    /// The permissions of a custom role (`roles [..] custom`, docs/roles.md
    /// §4.7), by the id a binding names: build the set with
    /// [`RoleCatalogue::permission_set`](super::RoleCatalogue::permission_set).
    /// Of it, only grantable permissions (neither internal nor
    /// `@notGrantable`) are honoured. `None` (the default): no such role,
    /// and its bindings grant nothing. Asked once per id per batch, and
    /// only for a catalogue that declares `custom`.
    fn custom_role(
        &self,
        role: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send {
        let _ = role;
        std::future::ready(Ok(None))
    }

    /// How a check no binding allows is refused. `covered`: some binding
    /// whose role is valid where it is bound covers the target. By default
    /// a target the caller holds nothing over is hidden (404) and one it
    /// holds something over denied (403).
    fn refusal(&self, covered: bool) -> Refusal {
        if covered {
            Refusal::Denied
        } else {
            Refusal::Hidden
        }
    }

    /// What a `filters` listing may show (`Authorizer::visible`), which
    /// stays the service's: by default the internal error, as an
    /// `Authorizer` that answers no listing. `RoleAuthorizer::capabilities`
    /// answers the per-instance question a listing filters by.
    fn visible(
        &self,
        principal: &Self::Principal,
        listing: &Listing,
    ) -> impl Future<Output = crate::Result<Visibility>> + Send {
        let _ = principal;
        std::future::ready(Err(crate::Error::internal(format!(
            "the BindingSource answers no `filters` listing, and {} on {} needs one",
            listing.permission.value,
            crate::permissions::loggable(&listing.resource)
        ))))
    }
}
