use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::store_unavailable::StoreUnavailable;
use super::{IdempotencyKey, IdempotencyStore, StoredResponse};
use crate::{Error, Result};

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// One request's idempotency state, left on the request context by
/// [`super::Idempotency`] for [`super::commit`] and its own `after` hook.
/// `claim` is `None` when the request carried no token and the service
/// accepts that: there is nothing to hold or complete.
#[derive(Clone)]
pub(crate) struct Attempt {
    pub claim: Option<Arc<HeldClaim>>,
}

/// A claim this request holds in flight.
pub(crate) struct HeldClaim {
    pub store: Arc<dyn IdempotencyStore>,
    pub key: IdempotencyKey,
    /// Names this request's claim, so a claim that expired and was taken by
    /// a later attempt is never completed or released by this one.
    pub attempt: String,
    pub retention: Duration,
    pub clock: Clock,
    /// Set once the operation passed its point of no return.
    pub committed: AtomicBool,
}

impl HeldClaim {
    /// Holds the claim for the whole retention: from here a failure keeps it,
    /// so a retry is refused as in flight rather than run again. Written
    /// before the irreversible step, so a crash or a dropped request after
    /// it is covered too.
    pub async fn commit(&self) -> Result<()> {
        if self.committed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let expires = after((self.clock)(), self.retention);
        self.store
            .hold(&self.key, &self.attempt, expires)
            .await
            .map_err(|error| Error::new(StoreUnavailable(error)))?;
        self.committed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Settles the claim once the outcome is known: `output` (the
    /// operation's success, in its JSON form) completes it for replay;
    /// a failure releases it, unless the operation had committed. Neither
    /// changes the request's answer, so a store failure here is logged: the
    /// claim then stays in flight until it expires.
    pub async fn finish(&self, output: Option<Value>) {
        let operation = self.key.operation();
        match output {
            Some(output) => {
                let response = StoredResponse::new(output.to_string());
                let expires = after((self.clock)(), self.retention);
                if let Err(error) = self
                    .store
                    .complete(&self.key, &self.attempt, &response, expires)
                    .await
                {
                    tracing::error!(
                        target: "crucible_server_runtime_rust::idempotency",
                        operation,
                        cause = %error.chain(),
                        "the operation succeeded but its idempotency record was not completed; retries are refused as in flight until the claim expires"
                    );
                }
            }
            None if self.committed.load(Ordering::SeqCst) => {
                tracing::warn!(
                    target: "crucible_server_runtime_rust::idempotency",
                    operation,
                    "the operation failed after its point of no return; its idempotency claim stays in flight until it expires, so a retry is refused rather than repeated"
                );
            }
            None => {
                if let Err(error) = self.store.release(&self.key, &self.attempt).await {
                    tracing::warn!(
                        target: "crucible_server_runtime_rust::idempotency",
                        operation,
                        cause = %error.chain(),
                        "the operation failed and its idempotency claim was not released; retries are refused as in flight until the claim expires"
                    );
                }
            }
        }
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
