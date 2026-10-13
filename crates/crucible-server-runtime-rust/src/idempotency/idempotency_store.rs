use std::sync::Arc;
use std::time::SystemTime;

use super::{Claim, Fingerprint, IdempotencyKey, StoredResponse};
use crate::Result;

/// Where a service keeps its idempotency records: a database table with a
/// conditional write (DynamoDB, a SQL row with a unique key), or
/// [`MemoryIdempotencyStore`](super::MemoryIdempotencyStore) in tests.
///
/// A record is the key, the fingerprint of the request that claimed it, the
/// attempt holding it, an expiry, and, once that attempt succeeded, its
/// [`StoredResponse`]. A record past its expiry is as good as absent: a
/// store may delete it lazily (a DynamoDB TTL attribute), but must never
/// answer from it.
///
/// `attempt` names one request's claim. A claim that expired may have been
/// taken by a later attempt, so `hold`, `complete` and `release` change a
/// record only while `attempt` still holds it, and otherwise do nothing.
///
/// An `Err` from any method is an outage: the request fails with the
/// service's sanitized internal error, never as if the claim had succeeded.
/// Errors reach the service log, so they must not carry the token.
#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// Atomically: when no record of `key` is unexpired at `now`, writes an
    /// in-flight record for `attempt` with `fingerprint`, expiring at
    /// `expires`, and answers [`Claim::Fresh`]; otherwise answers what the
    /// existing record holds and changes nothing. Of two concurrent first
    /// claims, exactly one is `Fresh` (a conditional put).
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> Result<Claim>;

    /// Keeps `attempt`'s in-flight record until `expires`: the operation has
    /// reached its point of no return
    /// ([`commit`](super::commit)), and a retry must not run it again even
    /// if the attempt never finishes.
    async fn hold(&self, key: &IdempotencyKey, attempt: &str, expires: SystemTime) -> Result<()>;

    /// Turns `attempt`'s in-flight record into a completed one carrying
    /// `response`, kept (with its fingerprint) until `expires`.
    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<()>;

    /// Removes `attempt`'s in-flight record: the attempt failed before its
    /// point of no return, so a retry runs the operation again.
    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<()>;
}

/// Lets a service keep a handle on the store it gives the interceptor.
#[async_trait::async_trait]
impl<T: IdempotencyStore + ?Sized> IdempotencyStore for Arc<T> {
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> Result<Claim> {
        (**self)
            .claim(key, fingerprint, attempt, now, expires)
            .await
    }

    async fn hold(&self, key: &IdempotencyKey, attempt: &str, expires: SystemTime) -> Result<()> {
        (**self).hold(key, attempt, expires).await
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<()> {
        (**self).complete(key, attempt, response, expires).await
    }

    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<()> {
        (**self).release(key, attempt).await
    }
}
