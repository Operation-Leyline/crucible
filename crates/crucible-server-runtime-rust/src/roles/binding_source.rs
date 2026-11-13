use std::sync::Arc;

use super::{Refusal, RoleBinding, Root, Unavailable};
use crate::Context;

/// What only the service knows, for [`RoleAuthorizer`](super::RoleAuthorizer):
/// who the caller is and the role bindings it holds (docs/roles.md §7.2).
/// Crucible stores nothing; storage, caching, freshness and tenancy stay
/// here. The hatches a model leaves to the service are traits of their
/// own, [`ServiceGrants`](super::ServiceGrants) and
/// [`CustomRoles`](super::CustomRoles), with no default: the generated
/// `roles::authorizer(source)` requires each one the model needs, so
/// forgetting one is a type error, never a silent refusal.
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
    /// [`Root::Every`] (a listing of a root resource's instances) asks for
    /// every binding the principal holds, under any root.
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

    /// How a check no binding allows is refused, and a `filters` listing
    /// no binding shows anything of. `covered`: some binding whose role is
    /// valid where it is bound covers the target (for a listing, the
    /// collection or one instance in it). By default a target the caller
    /// holds nothing over is hidden (404) and one it holds something over
    /// denied (403).
    fn refusal(&self, covered: bool) -> Refusal {
        if covered {
            Refusal::Denied
        } else {
            Refusal::Hidden
        }
    }
}
