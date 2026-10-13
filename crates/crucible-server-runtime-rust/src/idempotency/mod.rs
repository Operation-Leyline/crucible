//! Server-side idempotency: client tokens.
//!
//! An operation whose input has an idempotency token (`@idempotencyToken`)
//! can be retried safely: generated SDKs fill the token once per call and
//! send the same one on every retry. [`Idempotency`] makes that true on the
//! server:
//!
//! - **Required.** A request without a token (absent or empty), or with one
//!   longer than 256 characters or outside visible ASCII, is refused before
//!   the operation runs, as the service's validation error naming the
//!   member. [`Idempotency::optional_tokens`] lets a request without one run
//!   unclaimed.
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
//!   runs the operation again, unless the operation has called [`commit`]
//!   (and not [`abandon`] since).
//!
//! An operation calls [`commit`] right before any step that is not safe to
//! repeat: an external side effect (sending mail, charging a card), or a
//! write a second run would make twice. From then on a failure keeps the
//! claim in flight for the whole retention, so a retry is refused rather
//! than repeating the step; and an attempt that outlived its claim is
//! stopped there:
//!
//! ```ignore
//! let message = build(&input)?;                    // a failure here: the retry runs again
//! idempotency::commit().await?;                    // the point of no return
//! mailer.send(&message).await?;                    // a failure from here: the retry is refused
//! store.record(&message).await?;
//! ```
//!
//! When the step fails in a way that proves it did not happen (the provider
//! answered with an explicit refusal), the operation calls [`abandon`]
//! before returning the error, so the retry runs again instead of being
//! refused. An ambiguous failure (a timeout, a reset connection: the
//! provider may have acted) is returned without it:
//!
//! ```ignore
//! idempotency::commit().await?;
//! if let Err(error) = mailer.send(&message).await {
//!     if error.nothing_sent() {                    // throttled, or the message was rejected
//!         idempotency::abandon()?;                 // the retry runs again
//!     }
//!     return Err(error.into());                    // otherwise it may have been sent: refused
//! }
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
mod ownership;
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
pub use ownership::Ownership;
pub use stored_response::StoredResponse;

use crate::{Context, Error, Result};
use attempt::Attempt;

/// Marks this request's operation as past its point of no return: call it
/// right before a step that is not safe to repeat (an external side effect,
/// or a write a second run would duplicate). From then on a failure does not
/// release the request's claim; it stays in flight for the whole retention,
/// and a retry with the token is refused as in flight instead of running the
/// operation, and the step, again.
///
/// The claim is extended in the store before this returns, so a process
/// that stops, or a request dropped, after the step is covered too. An
/// error means the step must not be taken: return it. It is the service's
/// internal error when the store could not record the claim, and
/// [`IdempotencyRefusal::Lapsed`] (a 409 by default) when this request no
/// longer holds its claim because it ran past its lease, so another attempt
/// may be running the operation. Calling it again does nothing, unless
/// [`abandon`] was called since: then it commits again.
///
/// A request that runs unclaimed (it carried no token where
/// [`Idempotency::optional_tokens`] allows that, or its API version has no
/// token) has no claim, and this does nothing. An error also means
/// [`Idempotency`] is not installed or the operation has no idempotency
/// token: a wiring bug, not a bad request.
///
/// A step that then fails in a way that proves it did not happen is taken
/// back with [`abandon`].
pub async fn commit() -> Result<()> {
    match attempt()?.claim {
        Some(claim) => claim.commit().await,
        None => Ok(()),
    }
}

/// Takes back [`commit`]: declares that the irreversible step after it
/// provably did not happen, so if the operation now fails, its claim is
/// released exactly as if `commit` had never been called, and a retry with
/// the token runs the operation again.
///
/// Call it only on a failure that proves nothing happened (the provider
/// answered with an explicit refusal: throttled, the message rejected), then
/// return the error. Never on an ambiguous one (a timeout, a reset
/// connection: the provider may have acted), which must keep the claim so a
/// retry is refused rather than repeating the step:
///
/// ```ignore
/// idempotency::commit().await?;
/// if let Err(error) = mailer.send(&message).await {
///     if error.nothing_sent() {
///         idempotency::abandon()?;
///     }
///     return Err(error.into());
/// }
/// ```
///
/// It changes only what a failure does: an operation that succeeds anyway
/// completes as usual, and its retries replay the success. The claim is
/// released when the request finishes, not here, so a retry while the
/// operation still runs is refused as in flight. If the process stops after
/// this but before the operation finishes, the claim stays held as
/// committed and retries are refused until the retention ends: the
/// conservative outcome. Calling it without a prior `commit` does nothing (a
/// failure releases the claim anyway); a `commit` after it commits again.
///
/// Like `commit`, it does nothing for a request that runs unclaimed, and an
/// error means [`Idempotency`] is not installed or the operation has no
/// idempotency token: a wiring bug, not a bad request.
pub fn abandon() -> Result<()> {
    if let Some(claim) = attempt()?.claim {
        claim.abandon();
    }
    Ok(())
}

/// The request's idempotency state, which [`Idempotency`] leaves on every
/// request to an operation with a token.
fn attempt() -> Result<Attempt> {
    Context::try_current()
        .and_then(|context| context.attribute::<Attempt>())
        .ok_or_else(|| {
            Error::internal(
                "no idempotency claim on the request: install Idempotency, and call this from an operation with an idempotency token",
            )
        })
}
