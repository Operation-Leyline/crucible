use std::sync::Arc;
use std::time::SystemTime;

use super::{Claim, Fingerprint, IdempotencyKey, Ownership, StoredResponse};
use crate::Result;

/// Where a service keeps its idempotency records: a database table with
/// conditional writes (DynamoDB, a SQL row with a unique key), or
/// [`MemoryIdempotencyStore`](super::MemoryIdempotencyStore) in tests.
///
/// A record is the key, the fingerprint of the request that claimed it, the
/// attempt holding it, an expiry, and, once that attempt succeeded, its
/// [`StoredResponse`]. A record past its expiry is as good as absent to
/// `claim`: a store may delete it lazily (a DynamoDB TTL attribute), but
/// must never answer a claim from it.
///
/// `attempt` names one request's claim. A claim that lapsed may have been
/// taken by a later attempt, or deleted, so `hold`, `complete` and `release`
/// change a record only while it is still `attempt`'s in-flight record
/// (expired or not: until another attempt claims it, nobody else can be
/// running), and answer [`Ownership::Lost`] otherwise. Each is one
/// conditional write on the attempt; a store must never answer `Held`
/// without having made the change.
///
/// On DynamoDB, with [`IdempotencyKey::digest`] as the partition key:
///
/// - `claim` is one `PutItem` with the condition
///   `attribute_not_exists(pk) OR expires <= :now` and
///   `ReturnValuesOnConditionCheckFailure = ALL_OLD`, so a failed condition
///   returns the record that won in the same call. Reading it with a
///   separate `GetItem` after a failed put is not atomic (the record can
///   lapse, complete or be released in between): never do that.
/// - `hold` and `complete` are an `UpdateItem`, and `release` a
///   `DeleteItem`, each with the condition
///   `attempt = :attempt AND attribute_not_exists(response)`. A failed
///   condition is `Ownership::Lost`, not an error.
/// - `expires`, in epoch seconds, doubles as the TTL attribute.
///
/// A method may run outside the request's task: the settlement of a request
/// that was dropped, and retries of a settlement that failed, run on tasks
/// of their own, where [`Context::try_current`](crate::Context::try_current)
/// is `None`. A store must not depend on the request context.
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
    /// claims, exactly one is `Fresh`.
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> Result<Claim>;

    /// Keeps `attempt`'s in-flight record until `expires`: the operation has
    /// reached its point of no return ([`commit`](super::commit)), or its
    /// success could not be stored, and a retry must not run it again.
    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> Result<Ownership>;

    /// Turns `attempt`'s in-flight record into a completed one carrying
    /// `response`, kept (with its fingerprint) until `expires`.
    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<Ownership>;

    /// Removes `attempt`'s in-flight record: the attempt failed before its
    /// point of no return, or after [`abandon`](super::abandon)ing it (so
    /// the record may be one `hold` extended), and a retry runs the
    /// operation again.
    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<Ownership>;
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

    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> Result<Ownership> {
        (**self).hold(key, attempt, expires).await
    }

    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<Ownership> {
        (**self).complete(key, attempt, response, expires).await
    }

    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<Ownership> {
        (**self).release(key, attempt).await
    }
}
