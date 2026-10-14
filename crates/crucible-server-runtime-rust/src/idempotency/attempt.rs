use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::store_unavailable::StoreUnavailable;
use super::{IdempotencyKey, IdempotencyRefusal, IdempotencyStore, Ownership, StoredResponse};
use crate::{Error, Result};

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;
pub(crate) type RefusalFactory = Arc<dyn Fn(IdempotencyRefusal) -> Error + Send + Sync>;

const TARGET: &str = "crucible_server_runtime_rust::idempotency";

/// When a succeeded request's settlement could not be written at all, it is
/// tried again in the background after each of these: inside the default
/// lease, so it lands before a retry could take the token over.
const SETTLE_RETRIES: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(25),
];

/// One request's idempotency state, left on the request context by
/// [`super::Idempotency`] for [`super::commit`], [`super::abandon`] and its
/// own `after` hook.
/// `claim` is `None` when the request runs unclaimed: it carried no token
/// and the service accepts that, or its API version has no token.
#[derive(Clone)]
pub(crate) struct Attempt {
    pub claim: Option<Arc<HeldClaim>>,
}

/// What a claim needs to settle itself, fixed when the interceptor claims.
pub(crate) struct ClaimConfig {
    pub store: Arc<dyn IdempotencyStore>,
    pub retention: Duration,
    /// The largest stored response, in bytes.
    pub max_stored: usize,
    /// How long any one store call may take.
    pub timeout: Duration,
    pub refusal: RefusalFactory,
    pub clock: Clock,
}

/// A claim this request holds in flight, or is claiming.
///
/// It is also the request's drop guard: a request that ends without
/// settling its claim (the client disconnected and the host dropped the
/// request's future, a deadline stopped the claim's own store call, or that
/// call failed ambiguously) settles it when the last handle goes. A claim
/// with no commit standing is released, so a retry runs at once instead of
/// waiting out the lease; one past its point of no return is recorded as
/// failed, as a failure after `commit` is.
pub(crate) struct HeldClaim {
    config: ClaimConfig,
    key: IdempotencyKey,
    /// Names this request's claim, so a claim that lapsed and was taken by
    /// a later attempt is never completed or released by this one.
    attempt: String,
    /// The service's current API version, which a stored output is in.
    api_version: &'static str,
    /// The commits standing: each [`commit`](Self::commit) adds one, and each
    /// [`abandon`](Self::abandon) takes back the latest. A failure keeps the
    /// claim while any stands.
    commits: AtomicUsize,
    /// Set once [`finish`](Self::finish) took the claim over, or the claim
    /// turned out not to be this attempt's: the drop guard does nothing then.
    settled: AtomicBool,
}

impl HeldClaim {
    pub fn new(
        config: ClaimConfig,
        key: IdempotencyKey,
        attempt: String,
        api_version: &'static str,
    ) -> Self {
        Self {
            config,
            key,
            attempt,
            api_version,
            commits: AtomicUsize::new(0),
            settled: AtomicBool::new(false),
        }
    }

    pub fn key(&self) -> &IdempotencyKey {
        &self.key
    }

    pub fn attempt(&self) -> &str {
        &self.attempt
    }

    /// The claim answered with another attempt's record, so there is
    /// nothing of this attempt's to settle.
    pub fn disown(&self) {
        self.settled.store(true, Ordering::SeqCst);
    }

    /// Holds the claim for the whole retention: from here a failure keeps it,
    /// so a retry is refused rather than run again. Written before the
    /// irreversible step, so a crash or a dropped request after it is
    /// covered too. Every call writes, so every point of no return finds out
    /// whether the claim is still this attempt's: one it no longer holds is
    /// refused, since another attempt may be running the operation.
    pub async fn commit(&self) -> Result<()> {
        match self
            .hold()
            .await
            .map_err(|error| Error::new(StoreUnavailable(error)))?
        {
            Ownership::Held => {
                self.commits.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            Ownership::Lost => {
                tracing::warn!(
                    target: TARGET,
                    operation = self.key.operation(),
                    "the operation reached its point of no return after its idempotency claim lapsed, and was stopped there; the lease is shorter than the operation runs"
                );
                Err((self.config.refusal)(IdempotencyRefusal::Lapsed))
            }
        }
    }

    /// Takes back the latest [`commit`](Self::commit) still standing: the
    /// step after it did not happen. Earlier commits stand, since their
    /// steps did. Once none stands, a failure releases the claim as if it
    /// had never been committed. Only the count changes; the claim stays
    /// held in the store (until the retention) while the operation runs, so
    /// a retry is still refused as in flight until [`finish`](Self::finish)
    /// settles it.
    pub fn abandon(&self) {
        let _ = self
            .commits
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |commits| {
                commits.checked_sub(1)
            });
    }

    fn committed(&self) -> bool {
        self.commits.load(Ordering::SeqCst) > 0
    }

    /// Settles the claim once the outcome is known: `output` (the
    /// operation's success, in its JSON form) completes it for replay; a
    /// failure releases it, unless a commit still stands, when it is
    /// recorded as failed so a retry is refused as such. Neither changes the
    /// request's answer, so problems here are logged.
    ///
    /// The settlement runs as a task of its own, so a request dropped while
    /// it waits on the store still settles.
    pub async fn finish(self: Arc<Self>, output: Option<Value>, authorization: Option<Value>) {
        self.settled.store(true, Ordering::SeqCst);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let claim = self.clone();
                let settled = runtime
                    .spawn(async move { claim.settle(output, authorization).await })
                    .await;
                if let Err(error) = settled
                    && error.is_panic()
                {
                    tracing::error!(
                        target: TARGET,
                        operation = self.key.operation(),
                        "settling the idempotency claim panicked; retries are refused as in flight until its lease ends"
                    );
                }
            }
            Err(_) => self.settle(output, authorization).await,
        }
    }

    async fn settle(self: &Arc<Self>, output: Option<Value>, authorization: Option<Value>) {
        let operation = self.key.operation();
        match output {
            Some(output) => {
                let response = StoredResponse::wrap(self.api_version, output, authorization);
                if response.as_str().len() > self.config.max_stored {
                    tracing::warn!(
                        target: TARGET,
                        operation,
                        bytes = response.as_str().len(),
                        limit = self.config.max_stored,
                        "the operation's response is larger than an idempotency record keeps; retries are refused as unreplayable for the retention instead of replayed"
                    );
                    self.keep_unkept().await;
                    return;
                }
                match self.complete(&response).await {
                    Ok(Ownership::Held) => {}
                    Ok(Ownership::Lost) => self.lost("succeeded; its response is not stored"),
                    Err(error) => {
                        tracing::error!(
                            target: TARGET,
                            operation,
                            cause = %error.chain(),
                            "the operation succeeded but its idempotency record was not completed; recording the success without its response, so retries are refused rather than run again"
                        );
                        self.keep_unkept().await;
                    }
                }
            }
            None if self.committed() => {
                tracing::warn!(
                    target: TARGET,
                    operation,
                    "the operation failed after its point of no return; a retry with its idempotency token is refused rather than repeated, until the retention ends"
                );
                match self
                    .complete(&StoredResponse::failed(self.api_version))
                    .await
                {
                    Ok(Ownership::Held) => {}
                    Ok(Ownership::Lost) => self.lost("failed after its point of no return"),
                    Err(error) => {
                        // `commit` held the claim for the retention already:
                        // retries are still refused, as in flight.
                        tracing::warn!(
                            target: TARGET,
                            operation,
                            cause = %error.chain(),
                            "the failure was not recorded on the idempotency claim; retries are refused as in flight until the retention ends"
                        );
                    }
                }
            }
            None => match bounded(
                self.config.timeout,
                self.config.store.release(&self.key, &self.attempt),
            )
            .await
            {
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

    async fn complete(&self, response: &StoredResponse) -> Result<Ownership> {
        let expires = after((self.config.clock)(), self.config.retention);
        bounded(
            self.config.timeout,
            self.config
                .store
                .complete(&self.key, &self.attempt, response, expires),
        )
        .await
    }

    async fn hold(&self) -> Result<Ownership> {
        let expires = after((self.config.clock)(), self.config.retention);
        bounded(
            self.config.timeout,
            self.config.store.hold(&self.key, &self.attempt, expires),
        )
        .await
    }

    /// Records a success whose response is not kept, so a retry is refused
    /// as unreplayable for the retention rather than run again. When that
    /// write fails, the claim is held in flight for the retention instead;
    /// when both fail, they are tried again in the background, since the
    /// claim otherwise lapses with its lease and a retry runs the succeeded
    /// operation again.
    async fn keep_unkept(self: &Arc<Self>) {
        if self.try_keep_unkept().await {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.unsettled();
            return;
        };
        let claim = self.clone();
        runtime.spawn(async move {
            for delay in SETTLE_RETRIES {
                tokio::time::sleep(delay).await;
                if claim.try_keep_unkept().await {
                    return;
                }
            }
            claim.unsettled();
        });
    }

    /// Whether the success is now recorded or held, or is no longer this
    /// attempt's to record.
    async fn try_keep_unkept(&self) -> bool {
        match self
            .complete(&StoredResponse::unkept(self.api_version))
            .await
        {
            Ok(Ownership::Held) => return true,
            Ok(Ownership::Lost) => {
                // Completed already (a write that failed ambiguously may
                // have landed), or taken over.
                self.lost("succeeded; its response is not stored");
                return true;
            }
            Err(error) => tracing::warn!(
                target: TARGET,
                operation = self.key.operation(),
                cause = %error.chain(),
                "the success of an operation whose response is not stored could not be recorded; holding its idempotency claim instead"
            ),
        }
        match self.hold().await {
            Ok(Ownership::Held) => true,
            Ok(Ownership::Lost) => {
                self.lost("succeeded; its claim could not be held");
                true
            }
            Err(error) => {
                tracing::warn!(
                    target: TARGET,
                    operation = self.key.operation(),
                    cause = %error.chain(),
                    "the idempotency claim of a succeeded operation could not be held; trying again"
                );
                false
            }
        }
    }

    fn unsettled(&self) {
        tracing::error!(
            target: TARGET,
            operation = self.key.operation(),
            "the idempotency claim of a succeeded operation could not be recorded or held; a retry after its lease runs it again"
        );
    }

    fn lost(&self, outcome: &str) {
        tracing::warn!(
            target: TARGET,
            operation = self.key.operation(),
            outcome,
            "the operation finished after its idempotency claim lapsed, or its record was settled already; if the lease is shorter than the operation runs, another attempt may have run it too"
        );
    }
}

impl Drop for HeldClaim {
    /// The request ended without settling its claim: settles it in the
    /// background, conditional on this attempt as every settlement is, so a
    /// claim another attempt took over is left alone.
    fn drop(&mut self) {
        if *self.settled.get_mut() {
            return;
        }
        let committed = *self.commits.get_mut() > 0;
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                target: TARGET,
                operation = self.key.operation(),
                "a request ended without settling its idempotency claim, outside a runtime that could; retries are refused as in flight until its lease ends"
            );
            return;
        };
        let store = self.config.store.clone();
        let timeout = self.config.timeout;
        let key = self.key.clone();
        let attempt = self.attempt.clone();
        let failed = committed.then(|| {
            (
                StoredResponse::failed(self.api_version),
                after((self.config.clock)(), self.config.retention),
            )
        });
        runtime.spawn(async move {
            let operation = key.operation();
            let settled = match &failed {
                Some((response, expires)) => {
                    bounded(timeout, store.complete(&key, &attempt, response, *expires)).await
                }
                None => bounded(timeout, store.release(&key, &attempt)).await,
            };
            match settled {
                Ok(_) => tracing::debug!(
                    target: TARGET,
                    operation,
                    committed,
                    "a request ended without settling its idempotency claim; released it, or recorded it as failed past its point of no return"
                ),
                Err(error) => tracing::warn!(
                    target: TARGET,
                    operation,
                    cause = %error.chain(),
                    "a request ended without settling its idempotency claim, and settling it failed; retries are refused as in flight until it expires"
                ),
            }
        });
    }
}

/// `call`, failing once it has taken longer than `timeout`: a store that
/// stops answering must not hold a request, or its after phases, forever.
pub(crate) async fn bounded<T>(
    timeout: Duration,
    call: impl Future<Output = Result<T>>,
) -> Result<T> {
    match tokio::time::timeout(timeout, call).await {
        Ok(result) => result,
        Err(_) => Err(Error::internal(format!(
            "the idempotency store did not answer within {} ms",
            timeout.as_millis()
        ))),
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
