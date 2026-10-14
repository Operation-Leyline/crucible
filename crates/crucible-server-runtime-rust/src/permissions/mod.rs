//! Typed permissions (see docs/framework.md, "Typed permissions").
//!
//! A model declares the permissions each resource accepts and what each
//! operation requires; generated code states both as `'static` data
//! (`PermissionInfo`, `Operation::REQUIREMENTS`)
//! and evaluates the bindings against the decoded input
//! (`Operation::CHECKS`). Those descriptions are
//! always here. With the `permissions` feature, `Permissions` enforces
//! them through the service's `Authorizer`, which alone decides whether a
//! principal holds a permission:
//!
//! - **Declared checks** run before the operation, in one batched
//!   `Authorizer::decide` call, and the first verdict in declaration order
//!   that is not `Verdict::Allowed` refuses the request: `Verdict::Hidden`
//!   is the framework's 404 (`FrameworkError::NotFound`), or the service's
//!   own not-found error when the model designates one (`notFound` in the
//!   service; `Permissions::not_found_factory` builds it), and
//!   `Verdict::Denied` a 403 that may name the missing permission (never an
//!   internal one), or what `Permissions::refusal_factory` builds.
//! - **Listings that filter** ask `Authorizer::visible`, and the handler
//!   reads the answer with `visible`; a hidden or denied listing is refused
//!   as a Hidden or Denied verdict on its clause would be.
//! - **Handler obligations** (`requires p on R by handler`) are opened before
//!   the operation and must be closed by the handler, with `discharge` or
//!   `not_applicable`, before `idempotency::commit()`
//!   and before it succeeds.
//! - **Handler checks** the model cannot declare (a permission the request
//!   chooses) go through `require` and `decide`.
//!
//! Register it after authentication and before `PageTokens` and
//! `Idempotency`, so a refused request never claims an idempotency token and
//! every replay is authorized again:
//!
//! ```ignore
//! builder
//!     .operation_middleware(chain)
//!     .interceptor(Permissions::new(authorizer).refusal_factory(refused))
//!     .interceptor(PageTokens::new(keys, caller))
//!     .interceptor(Idempotency::new(store, caller))
//! ```

#[cfg(feature = "permissions")]
mod authorization_record;
#[cfg(feature = "permissions")]
mod authorization_request;
#[cfg(feature = "permissions")]
mod authorizer;
#[cfg(feature = "permissions")]
mod authorizer_unavailable;
#[cfg(feature = "permissions")]
mod decision;
#[cfg(feature = "permissions")]
mod listing;
#[cfg(feature = "permissions")]
mod obligation_outcome;
#[cfg(feature = "permissions")]
mod obligation_record;
#[cfg(feature = "permissions")]
mod operation_facts;
#[cfg(feature = "permissions")]
mod permission_refusal;
#[cfg(feature = "permissions")]
#[allow(clippy::module_inception)]
mod permissions;
#[cfg(feature = "permissions")]
mod session;
#[cfg(feature = "permissions")]
mod verdict;
#[cfg(feature = "permissions")]
mod visibility;

pub use crate::operation_metadata::ErasedChecks;
pub use crucible_wire::permissions::{
    Check, Checks, Mode, Obligation, Origin, PermissionInfo, RequirementInfo, Requirements,
    ResourceInfo, ResourceRef, Target, Tier,
};

#[cfg(feature = "permissions")]
pub use authorization_record::AuthorizationRecord;
#[cfg(feature = "permissions")]
pub use authorization_request::AuthorizationRequest;
#[cfg(feature = "permissions")]
pub use authorizer::Authorizer;
#[cfg(feature = "permissions")]
pub use decision::Decision;
#[cfg(feature = "permissions")]
pub use listing::Listing;
#[cfg(feature = "permissions")]
pub use obligation_outcome::ObligationOutcome;
#[cfg(feature = "permissions")]
pub use obligation_record::ObligationRecord;
#[cfg(feature = "permissions")]
pub use operation_facts::OperationFacts;
#[cfg(feature = "permissions")]
pub use permission_refusal::PermissionRefusal;
#[cfg(feature = "permissions")]
pub use permissions::{Permissions, UndeclaredOperations};
#[cfg(feature = "permissions")]
pub use verdict::Verdict;
#[cfg(feature = "permissions")]
pub use visibility::Visibility;

#[cfg(all(feature = "permissions", feature = "idempotency"))]
pub(crate) use session::{before_commit, reauthorize, stored_checks};
#[cfg(feature = "permissions")]
pub(crate) use session::{replayed, settle};

use crate::Error;
#[cfg(feature = "permissions")]
use crate::Result;
#[cfg(feature = "permissions")]
use session::Session;

/// The 404 for `resource`: what a handler returns when the resource is
/// genuinely missing, so the answer is byte-identical to the one an
/// authorization that hides it gets. The framework's `NotFoundException`,
/// naming the resource's type and nothing else; or, when the model
/// designates the service's own not-found error, what the
/// `Permissions::not_found_factory` builds from `resource`, which needs the
/// request to be one `Permissions` covers (elsewhere it is the framework's).
pub fn not_found(resource: &ResourceRef) -> Error {
    #[cfg(feature = "permissions")]
    if let Ok((_, session)) = Session::current() {
        return session.not_found(resource);
    }
    resource.not_found()
}

/// Requires `permission` on `resource`, through the service's Authorizer,
/// from a handler: for a check the model cannot declare. A refusal is
/// answered exactly as a declared check's (Hidden is the framework 404,
/// Denied goes through the refusal factory), and returning it before
/// `idempotency::commit()` releases the request's idempotency claim. Call it
/// before any write. An internal error when [`Permissions`] is not
/// installed for the operation, and a sanitized internal error when the
/// Authorizer cannot tell.
#[cfg(feature = "permissions")]
pub async fn require(
    permission: impl Into<&'static PermissionInfo>,
    resource: ResourceRef,
) -> Result<()> {
    let (context, session) = Session::current()?;
    let check = Check::handler(permission, resource);
    let verdicts = session
        .decide_for_handler(&context, std::slice::from_ref(&check))
        .await?;
    match verdicts[0] {
        Verdict::Allowed => Ok(()),
        verdict => Err(session.refuse(&context, &check, verdict)),
    }
}

/// One verdict per check, in order, for a handler that maps refusals itself
/// (delegation refuses with its own error, not a missing permission). Every
/// check is a [`Check::handler`], and all of them go to the Authorizer in one
/// batch. Nothing is refused here: a check that is not [`Verdict::Allowed`]
/// is the handler's to answer, before any write.
#[cfg(feature = "permissions")]
pub async fn decide(checks: &[Check]) -> Result<Vec<Verdict>> {
    let (context, session) = Session::current()?;
    if let Some(check) = checks.iter().find(|check| check.origin != Origin::Handler) {
        return Err(Error::internal(format!(
            "permissions::decide takes handler checks; {} on {} came from {:?}",
            check.permission.value, check.resource, check.origin
        )));
    }
    session.decide_for_handler(&context, checks).await
}

/// Discharges a `by handler` obligation: checks its permission on
/// `resource`, which must be an instance of the obligation's resource,
/// through the Authorizer, and refuses a verdict that is not
/// [`Verdict::Allowed`] exactly as a declared check is refused. Call it
/// before any write: `idempotency::commit()` refuses while the obligation
/// is open, so a refusal here always releases the idempotency claim. The
/// obligation is closed once: discharging it again, or calling
/// [`not_applicable`] after a refusal, is an internal error. Allowed, the
/// check is kept with the request's idempotency record and asked again
/// before a replay is served.
#[cfg(feature = "permissions")]
pub async fn discharge(obligation: Obligation, resource: ResourceRef) -> Result<()> {
    let (context, session) = Session::current()?;
    session.discharge(&context, obligation, resource).await
}

/// Closes a `by handler` obligation without a check: this request is not
/// the case the clause covers (a read-only query, a grant without the
/// permission). `reason` goes in the [`AuthorizationRecord`].
#[cfg(feature = "permissions")]
pub fn not_applicable(obligation: Obligation, reason: impl Into<String>) -> Result<()> {
    let (context, session) = Session::current()?;
    session.not_applicable(&context, obligation, reason.into())
}

/// What a `filters` listing may show, as the Authorizer answered it before
/// the operation ran: [`Visibility::All`], or [`Visibility::Only`] the ids
/// the handler filters to. (A Hidden or Denied listing was refused before
/// the handler ran, except under `Permissions::shadow`, where it shows
/// nothing.) An internal error for an operation without a `filters` clause.
#[cfg(feature = "permissions")]
pub fn visible() -> Result<Visibility> {
    let (_, session) = Session::current()?;
    session.visibility()
}
