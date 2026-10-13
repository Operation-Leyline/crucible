use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::store_unavailable::StoreUnavailable;
use super::{IdempotencyKey, IdempotencyRefusal, IdempotencyStore, Ownership, StoredResponse};
use crate::{Error, Result};

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;
pub(crate) type RefusalFactory = Arc<dyn Fn(IdempotencyRefusal) -> Error + Send + Sync>;

const TARGET: &str = "crucible_server_runtime_rust::idempotency";

/// One request's idempotency state, left on the request context by
/// [`super::Idempotency`] for [`super::commit`], [`super::abandon`] and its
/// own `after` hook.
/// `claim` is `None` when the request runs unclaimed: it carried no token
/// and the service accepts that, or its API version has no token.
#[derive(Clone)]
pub(crate) struct Attempt {
    pub claim: Option<Arc<HeldClaim>>,
}

/// A claim this request holds in flight.
pub(crate) struct HeldClaim {
    pub store: Arc<dyn IdempotencyStore>,
    pub key: IdempotencyKey,
    /// Names this request's claim, so a claim that lapsed and was taken by
    /// a later attempt is never completed or released by this one.
    pub attempt: String,
    pub retention: Duration,
    /// The largest stored response, in bytes.
    pub max_stored: usize,
    /// The service's current API version, which a stored output is in.
    pub api_version: &'static str,
    pub refusal: RefusalFactory,
    pub clock: Clock,
    /// Set once the operation passed its point of no return, and cleared
    /// again when it abandoned that step: what a failure does with the claim.
    pub committed: AtomicBool,
}

impl HeldClaim {
    /// Holds the claim for the whole retention: from here a failure keeps it,
    /// so a retry is refused as in flight rather than run again. Written
    /// before the irreversible step, so a crash or a dropped request after
    /// it is covered too. A claim this request no longer holds is refused:
    /// another attempt may be running the operation, so this one must stop.
    pub async fn commit(&self) -> Result<()> {
        if self.committed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let expires = after((self.clock)(), self.retention);
        match self
            .store
            .hold(&self.key, &self.attempt, expires)
            .await
            .map_err(|error| Error::new(StoreUnavailable(error)))?
        {
            Ownership::Held => {
                self.committed.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ownership::Lost => {
                tracing::warn!(
                    target: TARGET,
                    operation = self.key.operation(),
                    "the operation reached its point of no return after its idempotency claim lapsed, and was stopped there; the lease is shorter than the operation runs"
                );
                Err((self.refusal)(IdempotencyRefusal::Lapsed))
            }
        }
    }

    /// Takes back [`commit`](Self::commit): the irreversible step did not
    /// happen, so a failure releases the claim as if it had never been
    /// committed. Only the flag changes; the claim stays held in the store
    /// (until the retention) while the operation runs, so a retry is still
    /// refused as in flight until [`finish`] settles it, and a crash before
    /// `finish` keeps it held: the conservative outcome. A later `commit`
    /// commits again.
    ///
    /// [`finish`]: Self::finish
    pub fn abandon(&self) {
        self.committed.store(false, Ordering::SeqCst);
    }

    /// Settles the claim once the outcome is known: `output` (the
    /// operation's success, in its JSON form) completes it for replay; a
    /// failure releases it, unless the operation had committed (and not
    /// abandoned since). Neither changes the request's answer, so problems
    /// here are logged.
    ///
    /// A success that cannot be stored (too large, or the write failed) is
    /// held in flight for the retention instead, so a retry is refused
    /// rather than running a succeeded operation again.
    pub async fn finish(&self, output: Option<Value>) {
        let operation = self.key.operation();
        match output {
            Some(output) => {
                let response = StoredResponse::wrap(self.api_version, output);
                if response.as_str().len() > self.max_stored {
                    tracing::warn!(
                        target: TARGET,
                        operation,
                        bytes = response.as_str().len(),
                        limit = self.max_stored,
                        "the operation's response is larger than an idempotency record keeps; retries are refused as in flight for the retention instead of replayed"
                    );
                    self.hold_for_retention().await;
                    return;
                }
                let expires = after((self.clock)(), self.retention);
                match self
                    .store
                    .complete(&self.key, &self.attempt, &response, expires)
                    .await
                {
                    Ok(Ownership::Held) => {}
                    Ok(Ownership::Lost) => self.lost("succeeded; its response is not stored"),
                    Err(error) => {
                        tracing::error!(
                            target: TARGET,
                            operation,
                            cause = %error.chain(),
                            "the operation succeeded but its idempotency record was not completed; holding the claim so retries are refused rather than run again"
                        );
                        self.hold_for_retention().await;
                    }
                }
            }
            None if self.committed.load(Ordering::SeqCst) => {
                tracing::warn!(
                    target: TARGET,
                    operation,
                    "the operation failed after its point of no return; its idempotency claim stays in flight until it expires, so a retry is refused rather than repeated"
                );
            }
            None => match self.store.release(&self.key, &self.attempt).await {
                Ok(Ownership::Held) => {}
                Ok(Ownership::Lost) => self.lost("failed"),
                Err(error) => {
                    tracing::warn!(
                        target: TARGET,
                        operation,
                        cause = %error.chain(),
                        "the operation failed and its idempotency claim was not released; retries are refused as in flight until the claim expires"
                    );
                }
            },
        }
    }

    async fn hold_for_retention(&self) {
        let expires = after((self.clock)(), self.retention);
        match self.store.hold(&self.key, &self.attempt, expires).await {
            Ok(Ownership::Held) => {}
            Ok(Ownership::Lost) => self.lost("succeeded; its claim could not be held"),
            Err(error) => {
                tracing::error!(
                    target: TARGET,
                    operation = self.key.operation(),
                    cause = %error.chain(),
                    "the idempotency claim of a succeeded operation could not be held; a retry after the lease runs it again"
                );
            }
        }
    }

    fn lost(&self, outcome: &str) {
        tracing::warn!(
            target: TARGET,
            operation = self.key.operation(),
            outcome,
            "the operation finished after its idempotency claim lapsed, and another attempt may have run it too; the lease is shorter than the operation runs"
        );
    }
}

/// `now + length`. A length too long for the platform's clock is a century:
/// a retention that long keeps the record for good rather than not at all.
pub(crate) fn after(now: SystemTime, length: Duration) -> SystemTime {
    const CENTURY: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);
    now.checked_add(length)
        .or_else(|| now.checked_add(CENTURY))
        .unwrap_or(now)
}
