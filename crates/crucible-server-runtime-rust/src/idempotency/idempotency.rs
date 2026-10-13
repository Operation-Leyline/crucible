use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::attempt::{Attempt, Clock, HeldClaim, after};
use super::store_unavailable::StoreUnavailable;
use super::{Claim, Fingerprint, IdempotencyKey, IdempotencyRefusal, IdempotencyStore};
use crate::{
    Context, DecodedJob, Error, Hook, Interceptor, OperationJob, Outcome, Prepare, Result,
};

/// How long a completed request's response is replayed: the API standard
/// asks for at least the lesser of 8 hours and the resource's lifetime plus
/// an hour; a day covers a client retrying across a working day.
const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a claim stays in flight when its request never settles it (the
/// process stopped, or the request was dropped): Lambda's longest
/// invocation, so no attempt still running loses its claim to a retry.
const DEFAULT_LEASE: Duration = Duration::from_secs(15 * 60);

/// Past a request's deadline the framework has stopped its operation; this
/// leaves the after phases time to settle the claim before it lapses.
const DEADLINE_GRACE: Duration = Duration::from_secs(60);

type Caller = Arc<dyn Fn(&Context) -> Option<String> + Send + Sync>;
type RefusalFactory = Arc<dyn Fn(IdempotencyRefusal) -> Error + Send + Sync>;

/// Server-side idempotency for every operation with an idempotency token
/// (see the [module docs](super)): it requires the token, claims it in the
/// service's [`IdempotencyStore`] before the operation runs, answers a retry
/// with the first success, refuses a reused token whose input differs or
/// whose first request is still running, and releases the claim when the
/// operation fails before its point of no return ([`super::commit`]).
///
/// An `Interceptor`: it reads the decoded input, so the fingerprint is the
/// same whichever protocol or binding carried it. Operations without a token
/// pass through untouched. Register it before interceptors that should not
/// run for a replay or a refused duplicate, such as ones with side effects.
pub struct Idempotency {
    store: Arc<dyn IdempotencyStore>,
    caller: Caller,
    refusal: RefusalFactory,
    retention: Duration,
    lease: Duration,
    required: bool,
    clock: Clock,
}

impl Idempotency {
    /// Claims tokens in `store`. `caller` names who is asking, from the
    /// request context (an authenticated principal's issuer and subject,
    /// say), so one caller's token never replays or blocks another's. `None`
    /// is an anonymous caller, and every anonymous caller shares one
    /// binding: return a name for every authenticated request.
    pub fn new(
        store: impl IdempotencyStore,
        caller: impl Fn(&Context) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            store: Arc::new(store),
            caller: Arc::new(caller),
            refusal: Arc::new(IdempotencyRefusal::default_error),
            retention: DEFAULT_RETENTION,
            lease: DEFAULT_LEASE,
            required: true,
            clock: Arc::new(SystemTime::now),
        }
    }

    /// How long a completed request's output answers its retries, and how
    /// long a claim past its point of no return stays in flight. 24 hours
    /// by default.
    pub fn retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    /// How long a claim stays in flight when nothing settles it: a request
    /// dropped mid-operation, or a process that stopped. A retry is refused
    /// as in flight until then. 15 minutes by default, and never past the
    /// request's deadline plus a minute, since the framework stops the
    /// operation at its deadline. Set it above the longest an operation
    /// runs: a claim that lapses while its operation still runs lets a
    /// retry run it again.
    pub fn lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// Lets a request without a token run, unprotected, rather than
    /// refusing it. The API standard asks services that implement
    /// idempotency to require the token, and generated SDKs always send one.
    pub fn optional_tokens(mut self) -> Self {
        self.required = false;
        self
    }

    /// How a refusal is answered. By default it is
    /// [`IdempotencyRefusal::default_error`]: the service's validation
    /// error naming the token member for a missing token, and a 409
    /// `ConflictException` for a mismatch or a request still in flight. A
    /// service with errors of its own (a `ConflictException` with a
    /// `reason`) builds them here; the error must be declared by the
    /// operation or the service.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(IdempotencyRefusal) -> Error + Send + Sync + 'static,
    ) -> Self {
        self.refusal = Arc::new(factory);
        self
    }

    /// The clock claims and records expire by, for deterministic tests.
    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// How long this request's claim stays in flight unsettled.
    fn lease_for(&self, context: &Context) -> Duration {
        let lease = self.lease.min(self.retention);
        match context.deadline() {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                lease.min(remaining.saturating_add(DEADLINE_GRACE))
            }
            None => lease,
        }
    }

    async fn claim(
        &self,
        job: &mut DecodedJob<'_>,
        key: IdempotencyKey,
        fingerprint: Fingerprint,
    ) -> Result<()> {
        let now = (self.clock)();
        let attempt = uuid::Uuid::new_v4().to_string();
        let expires = after(now, self.lease_for(job.context()));
        let claim = self
            .store
            .claim(&key, &fingerprint, &attempt, now, expires)
            .await
            .map_err(|error| Error::new(StoreUnavailable(error)))?;
        match claim {
            Claim::Fresh => {
                job.context().set_attribute(Attempt {
                    claim: Some(Arc::new(HeldClaim {
                        store: self.store.clone(),
                        key,
                        attempt,
                        retention: self.retention,
                        clock: self.clock.clone(),
                        committed: AtomicBool::new(false),
                    })),
                });
                Ok(())
            }
            Claim::Replay {
                fingerprint: earlier,
                response,
            } if earlier == fingerprint => {
                let output: Value = serde_json::from_str(response.as_str()).map_err(|error| {
                    Error::internal(format!(
                        "{}: a stored idempotent response is not JSON: {error}",
                        key.operation()
                    ))
                })?;
                job.replay_output(output)
            }
            Claim::Replay { .. } => Err((self.refusal)(IdempotencyRefusal::Mismatch)),
            Claim::InFlight {
                fingerprint: earlier,
            } if earlier != fingerprint => Err((self.refusal)(IdempotencyRefusal::Mismatch)),
            Claim::InFlight { .. } => Err((self.refusal)(IdempotencyRefusal::InFlight)),
        }
    }
}

impl Interceptor for Idempotency {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        let Some(member) = job.metadata().idempotency_token else {
            return Hook::Ready(Ok(()));
        };
        let mut input = job.input().to_json();
        let token = input
            .as_object_mut()
            .and_then(|members| members.remove(member.wire_name));
        let token = match token {
            Some(Value::String(token)) if !token.is_empty() => token,
            _ if self.required => {
                return Hook::Ready(Err((self.refusal)(IdempotencyRefusal::Missing {
                    member: member.name,
                })));
            }
            _ => {
                job.context().set_attribute(Attempt { claim: None });
                return Hook::Ready(Ok(()));
            }
        };
        let fingerprint = Fingerprint::of(&input);
        let key = IdempotencyKey::new(
            job.service().id,
            job.operation().id,
            (self.caller)(job.context()),
            token,
        );
        Hook::wait(self.claim(job, key, fingerprint))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        let Some(claim) = job
            .context()
            .attribute::<Attempt>()
            .and_then(|attempt| attempt.claim)
        else {
            return Hook::Ready(());
        };
        // The current version's output, before any older version's
        // conversion: a replay converts it again for the retry's version.
        let output = match job.outcome() {
            Some(Outcome::Output(output)) => Some(output.to_json()),
            _ => None,
        };
        Hook::wait(async move { claim.finish(output).await })
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        if self.retention.is_zero() || self.lease.is_zero() {
            return Err("an idempotency retention and lease must be positive".to_owned());
        }
        let refused: Vec<&str> = prepare
            .operations()
            .filter(|operation| {
                prepare.metadata(operation).is_some_and(|metadata| {
                    metadata.idempotency_token.is_some() && !metadata.replays_output
                })
            })
            .map(|operation| operation.name)
            .collect();
        if refused.is_empty() {
            return Ok(());
        }
        Err(format!(
            "Idempotency cannot replay the output of {}: it streams, or the bindings predate replay (regenerate them). Leave it out with `.except(&[..])`",
            refused.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use axum::http::{HeaderValue, Request};

    use super::{DEADLINE_GRACE, DEFAULT_LEASE, Idempotency};
    use crate::Context;
    use crate::idempotency::MemoryIdempotencyStore;

    fn request() -> Context {
        let (parts, ()) = Request::new(()).into_parts();
        Context::new(HeaderValue::from_static("r-1"), None, parts)
    }

    #[test]
    fn a_claim_lapses_by_its_lease_and_never_long_after_the_deadline() {
        let idempotency = Idempotency::new(MemoryIdempotencyStore::new(), |_| None);
        let context = request();
        assert_eq!(idempotency.lease_for(&context), DEFAULT_LEASE);
        context.set_deadline(Instant::now() + Duration::from_secs(30));
        let lease = idempotency.lease_for(&context);
        assert!(
            lease <= Duration::from_secs(30) + DEADLINE_GRACE,
            "{lease:?}"
        );
        assert!(lease > DEADLINE_GRACE, "{lease:?}");
        // A lease longer than the retention would outlive the record.
        let short = Idempotency::new(MemoryIdempotencyStore::new(), |_| None)
            .retention(Duration::from_secs(10));
        assert_eq!(short.lease_for(&request()), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn commit_outside_an_idempotent_request_is_a_wiring_error() {
        assert!(crate::idempotency::commit().await.is_err());
    }
}
