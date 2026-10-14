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
//! - **Refused.** The same token with different input, while the first
//!   request is still running, or after it failed past its point of no
//!   return, is a 409 `ConflictException` ([`IdempotencyRefusal`];
//!   [`Idempotency::refusal_factory`] builds a service's own errors instead).
//! - **Released on failure.** A failed attempt removes its claim, so a retry
//!   runs the operation again, unless a [`commit`] still stands (one not
//!   taken back by [`abandon`]). A request that ends without finishing,
//!   such as one dropped when its client disconnects, settles its claim the
//!   same way in the background: released, or recorded as failed when a
//!   commit stands.
//!
//! An operation calls [`commit`] right before any step that is not safe to
//! repeat: an external side effect (sending mail, charging a card), or a
//! write a second run would make twice. From then on a failure is recorded
//! for the whole retention, so a retry is refused rather than repeating the
//! step; and an attempt that outlived its claim is stopped there:
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
//!
//! `abandon` takes back only the latest `commit`. An operation with several
//! irreversible steps commits before each; abandoning the last leaves the
//! earlier ones standing, so a failure still keeps the claim.
//!
//! What a replay does not carry: headers an extension added to the first
//! response outside the operation's output (the retry gets its own), and
//! what an outer extension did after the record was written (`Idempotency`
//! stores the operation's outcome in its own after hook, before extensions
//! registered ahead of it unwind and before an older API version's
//! conversion). The record keeps the output as JSON, `@sensitive` members
//! included, in plaintext for the retention.

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

#[cfg(feature = "permissions")]
pub(crate) use fingerprint::hex;

use crate::{Context, Error, Result};
use attempt::Attempt;

/// Marks this request's operation as past its point of no return: call it
/// right before a step that is not safe to repeat (an external side effect,
/// or a write a second run would duplicate). From then on a failure does not
/// release the request's claim; it is kept for the whole retention, and a
/// retry with the token is refused ([`IdempotencyRefusal::Failed`]) instead
/// of running the operation, and the step, again.
///
/// The claim is extended in the store before this returns, so a process
/// that stops, or a request dropped, after the step is covered too. An
/// error means the step must not be taken: return it. It is the service's
/// internal error when the store could not record the claim, and
/// [`IdempotencyRefusal::Lapsed`] (a 409 by default) when this request no
/// longer holds its claim because it ran past its lease, so another attempt
/// may be running the operation. Call it before each such step: every call
/// writes, so each finds out whether the claim is still this request's, and
/// each is one step [`abandon`] can take back.
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
    // The point of no return: no permission obligation may still be open.
    #[cfg(feature = "permissions")]
    crate::permissions::before_commit()?;
    match attempt()?.claim {
        Some(claim) => claim.commit().await,
        None => Ok(()),
    }
}

/// Takes back the latest [`commit`]: declares that the irreversible step
/// after it provably did not happen. Earlier commits stand, since their
/// steps did happen; once none stands, a failure releases the claim exactly
/// as if `commit` had never been called, and a retry with the token runs the
/// operation again.
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
/// conservative outcome. Calling it with no commit standing does nothing (a
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
