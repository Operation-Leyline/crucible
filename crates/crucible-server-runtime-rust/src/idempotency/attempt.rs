use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;

use super::store_unavailable::StoreUnavailable;
use super::{IdempotencyKey, IdempotencyRefusal, IdempotencyStore, Ownership, StoredResponse};
use crate::{Context, Error, Result};

pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;
pub(crate) type RefusalFactory = Arc<dyn Fn(IdempotencyRefusal) -> Error + Send + Sync>;

const TARGET: &str = "crucible_server_runtime_rust::idempotency";

/// When a settlement could not be written, it is tried again in the
/// background after each of these, as long as they add up to less than the
/// claim's lease: so it lands before a retry could take the token over.
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
    /// How long this request's claim stays in flight unsettled.
    pub lease: Duration,
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
    /// The request's span and subscriber, captured when it claimed: work in
    /// the background (the drop guard's settlement, retries) logs as the
    /// request's, without keeping its context alive.
    span: tracing::Span,
    dispatch: tracing::Dispatch,
}

/// What a background retry writes.
#[derive(Clone, Copy)]
enum Retry {
    /// A success whose response is not kept: the note, or else a hold.
    Unkept,
    /// A failure past a commit: the note (the commit held the claim).
    Failed,
}

impl HeldClaim {
    /// Made on the request's task, whose span and subscriber it keeps.
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
            span: tracing::Span::current(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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
        // A decrement that stops at zero (`fetch_update` is deprecated on
        // newer toolchains, and its replacement is missing on older ones).
        let mut commits = self.commits.load(Ordering::SeqCst);
        while commits > 0 {
            match self.commits.compare_exchange_weak(
                commits,
                commits - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(actual) => commits = actual,
            }
        }
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
    /// it waits on the store still settles; the task runs with the request's
    /// context current, so the store and the log see the request.
    pub async fn finish(self: Arc<Self>, output: Option<Value>, authorization: Option<Value>) {
        self.settled.store(true, Ordering::SeqCst);
        let claim = self.clone();
        let settle = async move { claim.settle(output, authorization).await };
        let task = match Context::try_current() {
            Some(context) => tokio::spawn(context.scope(settle)),
            None => tokio::spawn(self.in_request(settle)),
        };
        if let Err(error) = task.await
            && error.is_panic()
        {
            tracing::error!(
                target: TARGET,
                operation = self.key.operation(),
                "settling the idempotency claim panicked; retries are refused as in flight until its lease ends"
            );
        }
    }

    /// `future`, logging as the request's: in its span, to its subscriber.
    fn in_request<F: Future>(&self, future: F) -> impl Future<Output = F::Output> + use<F> {
        future
            .instrument(self.span.clone())
            .with_subscriber(self.dispatch.clone())
    }

    /// The delays of background retries: [`SETTLE_RETRIES`] that fit in the
    /// lease.
    fn retry_delays(&self) -> Vec<Duration> {
        retry_delays(self.config.lease)
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
                        // retries are refused, as in flight, until the note
                        // lands.
                        tracing::warn!(
                            target: TARGET,
                            operation,
                            cause = %error.chain(),
                            "the failure was not recorded on the idempotency claim; retries are refused as in flight until it is"
                        );
                        self.retry(Retry::Failed);
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
        if !self.try_keep_unkept().await {
            self.retry(Retry::Unkept);
        }
    }

    /// Writes `what` again in the background, after each of the retry
    /// delays until it lands. Outside the request's context: a store must
    /// not need it.
    fn retry(self: &Arc<Self>, what: Retry) {
        let claim = self.clone();
        tokio::spawn(self.in_request(async move {
            for delay in claim.retry_delays() {
                tokio::time::sleep(delay).await;
                let landed = match what {
                    Retry::Unkept => claim.try_keep_unkept().await,
                    Retry::Failed => claim.try_record_failed().await,
                };
                if landed {
                    return;
                }
            }
            match what {
                Retry::Unkept => claim.unsettled(),
                Retry::Failed => tracing::warn!(
                    target: TARGET,
                    operation = claim.key.operation(),
                    "the failure was never recorded on the idempotency claim; retries are refused as in flight until the retention ends"
                ),
            }
        }));
    }

    /// Whether the failure is now recorded, or is no longer this attempt's
    /// to record.
    async fn try_record_failed(&self) -> bool {
        self.complete(&StoredResponse::failed(self.api_version))
            .await
            .is_ok()
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
        // A failure past a commit is retried, as `finish` retries it; a
        // release is not: the claim lapses with its lease anyway, and during
        // an outage every failed claim would multiply the store's load.
        let mut delays = if committed {
            self.retry_delays()
        } else {
            Vec::new()
        }
        .into_iter();
        let settle = async move {
            let operation = key.operation();
            loop {
                let settled = match &failed {
                    Some((response, expires)) => {
                        bounded(timeout, store.complete(&key, &attempt, response, *expires)).await
                    }
                    None => bounded(timeout, store.release(&key, &attempt)).await,
                };
                let error = match settled {
                    Ok(_) => {
                        tracing::debug!(
                            target: TARGET,
                            operation,
                            committed,
                            "a request ended without settling its idempotency claim; released it, or recorded it as failed past its point of no return"
                        );
                        return;
                    }
                    Err(error) => error,
                };
                match delays.next() {
                    Some(delay) => tokio::time::sleep(delay).await,
                    None => {
                        tracing::warn!(
                            target: TARGET,
                            operation,
                            cause = %error.chain(),
                            "a request ended without settling its idempotency claim, and settling it failed; retries are refused as in flight until it expires"
                        );
                        return;
                    }
                }
            }
        };
        runtime.spawn(self.in_request(settle));
    }
}

/// The [`SETTLE_RETRIES`] that add up to less than `lease`: a retry later
/// than that could find the token taken over, and could not help.
fn retry_delays(lease: Duration) -> Vec<Duration> {
    let mut waited = Duration::ZERO;
    SETTLE_RETRIES
        .into_iter()
        .take_while(|delay| {
            waited += *delay;
            waited < lease
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::retry_delays;

    #[test]
    fn background_retries_end_inside_the_lease() {
        assert_eq!(retry_delays(Duration::from_secs(15 * 60)).len(), 3);
        assert_eq!(
            retry_delays(Duration::from_secs(10)),
            [Duration::from_secs(1), Duration::from_secs(5)]
        );
        assert!(retry_delays(Duration::from_millis(500)).is_empty());
    }
}
