//! Server-side idempotency: client tokens.
//!
//! An operation whose input has an idempotency token (`@idempotencyToken`)
//! can be retried safely: generated SDKs fill the token once per call and
//! send the same one on every retry. [`Idempotency`] makes that true on the
//! server:
//!
//! - **Required.** A request without a token (absent or empty) is refused
//!   before the operation runs, as the service's validation error naming the
//!   member, unless [`Idempotency::optional_tokens`] says otherwise.
//! - **Claimed.** The token is claimed atomically in the service's
//!   [`IdempotencyStore`], keyed by the service, the operation, the caller
//!   and the token, with a [`Fingerprint`] of the rest of the decoded input.
//!   Of two concurrent first requests, exactly one runs.
//! - **Replayed.** A retry with the same token and the same input gets the
//!   first success's output, encoded afresh for the retry (its protocol, its
//!   API version, its own request id), without the operation running again.
//! - **Refused.** The same token with different input, or while the first
//!   request is still running, is a 409 `ConflictException`
//!   ([`IdempotencyRefusal`]; [`Idempotency::refusal_factory`] builds a
//!   service's own errors instead).
//! - **Released on failure.** A failed attempt removes its claim, so a retry
//!   runs the operation again, unless the operation has called [`commit`].
//!
//! An operation with an external side effect that cannot be undone (sending
//! mail, charging a card) calls [`commit`] right before it. From then on a
//! failure keeps the claim in flight for the whole retention, so a retry is
//! refused rather than repeating the side effect:
//!
//! ```ignore
//! let message = build(&input)?;                    // a failure here: the retry runs again
//! idempotency::commit().await?;                    // the point of no return
//! mailer.send(&message).await?;                    // a failure from here: the retry is refused
//! store.record(&message).await?;
//! ```

mod attempt;
mod claim;
mod fingerprint;
#[allow(clippy::module_inception)]
mod idempotency;
mod idempotency_key;
mod idempotency_refusal;
mod idempotency_store;
#[cfg(any(test, feature = "test-support"))]
mod memory_idempotency_store;
mod store_unavailable;
mod stored_response;

pub use claim::Claim;
pub use fingerprint::Fingerprint;
pub use idempotency::Idempotency;
pub use idempotency_key::IdempotencyKey;
pub use idempotency_refusal::IdempotencyRefusal;
pub use idempotency_store::IdempotencyStore;
#[cfg(any(test, feature = "test-support"))]
pub use memory_idempotency_store::MemoryIdempotencyStore;
pub use stored_response::StoredResponse;

use crate::{Context, Error, Result};
use attempt::Attempt;

/// Marks this request's operation as past its point of no return: call it
/// right before an external side effect that cannot be undone. From then
/// on a failure does not release the request's claim; it stays in flight
/// for the whole retention, and a retry with the token is refused as in
/// flight instead of running the operation, and the side effect, again.
///
/// The claim is extended in the store before this returns, so a process
/// that stops, or a request dropped, after the side effect is covered too.
/// An error means the store could not record it: return it, and do not take
/// the irreversible step. Calling it again does nothing.
///
/// A request that carried no token, where [`Idempotency::optional_tokens`]
/// allows that, has no claim, and this does nothing. An error also means
/// [`Idempotency`] is not installed or the operation has no idempotency
/// token: a wiring bug, not a bad request.
pub async fn commit() -> Result<()> {
    let attempt = Context::try_current()
        .and_then(|context| context.attribute::<Attempt>())
        .ok_or_else(|| {
            Error::internal(
                "no idempotency claim on the request: install Idempotency, and call this from an operation with an idempotency token",
            )
        })?;
    match attempt.claim {
        Some(claim) => claim.commit().await,
        None => Ok(()),
    }
}
