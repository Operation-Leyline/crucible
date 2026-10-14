use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::attempt::{Attempt, ClaimConfig, Clock, HeldClaim, RefusalFactory, after, bounded};
use super::store_unavailable::StoreUnavailable;
use super::stored_response::Recorded;
use super::{Claim, Fingerprint, IdempotencyKey, IdempotencyRefusal, IdempotencyStore};
use crate::operation::{Location, OperationInfo};
use crate::{Context, DecodedJob, Hook, Interceptor, OperationJob, Outcome, Prepare, Result};

/// How long a completed request's response is replayed: the API standard
/// asks for at least the lesser of 8 hours and the resource's lifetime plus
/// an hour; a day covers a client retrying across a working day.
const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a claim stays in flight when its request never settles it (the
/// process stopped, or a dropped request's settlement failed): Lambda's longest
/// invocation, so no attempt still running loses its claim to a retry.
const DEFAULT_LEASE: Duration = Duration::from_secs(15 * 60);

/// Past a request's deadline the framework has stopped its operation; this
/// leaves the after phases time to settle the claim before it lapses.
const DEADLINE_GRACE: Duration = Duration::from_secs(60);

/// The largest response an idempotency record keeps by default: well under
/// DynamoDB's 400 KB item limit, with room for the record's other fields.
const DEFAULT_MAX_STORED: usize = 256 * 1024;

/// How long one store call may take before it counts as an outage: well
/// above a database's ordinary latency, well below a request's patience.
const DEFAULT_STORE_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest token accepted. SDKs send a UUID (36 characters); a longer
/// token only costs storage and log space.
const MAX_TOKEN: usize = 256;

type Caller = Arc<dyn Fn(&Context) -> Option<String> + Send + Sync>;

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
    max_stored: usize,
    store_timeout: Duration,
    required: bool,
    clock: Clock,
}

impl Idempotency {
    /// Claims tokens in `store`. `caller` names who is asking, from the
    /// request context, so one caller's token never replays or blocks
    /// another's. Encode it unambiguously (a JSON array of the principal's
    /// issuer and subject, say): two callers whose names encode alike share
    /// one binding. `None` is an anonymous caller, and every anonymous
    /// caller shares one binding, so return a name for every authenticated
    /// request: an authenticated caller answered `None` replays and blocks
    /// every other one.
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
            max_stored: DEFAULT_MAX_STORED,
            store_timeout: DEFAULT_STORE_TIMEOUT,
            required: true,
            clock: Arc::new(SystemTime::now),
        }
    }

    /// How long a completed request's output answers its retries, and how
    /// long a failure past its point of no return, or a success whose
    /// response could not be stored, is refused. 24 hours by default.
    pub fn retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    /// How long a claim stays in flight when nothing settles it: a process
    /// that stopped, or a dropped request whose background settlement could
    /// not reach the store. A retry is refused
    /// as in flight until then. 15 minutes by default, and never past the
    /// request's deadline plus a minute, since the framework stops the
    /// operation at its deadline. Set it above the longest an operation
    /// runs: an operation that outlives its claim is stopped at its
    /// [`commit`](super::commit), but a write it made before that may be
    /// made again by a retry.
    pub fn lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// The largest response a record keeps, in bytes of its JSON form;
    /// 256 KiB by default. A larger success is not stored: the record notes
    /// the success without it for the retention, so a retry is refused as
    /// [`IdempotencyRefusal::Unreplayable`] rather than run again, and the
    /// service log says why.
    pub fn max_stored_response(mut self, bytes: usize) -> Self {
        self.max_stored = bytes;
        self
    }

    /// How long one store call may take; 5 seconds by default. Past it the
    /// call counts as failed: a claim is the service's internal error (and
    /// its record, should the write have landed, is released in the
    /// background), and a settlement after the operation is logged and
    /// falls back as a failed write does. It bounds the after phases too,
    /// which run outside the request's deadline.
    pub fn store_timeout(mut self, timeout: Duration) -> Self {
        self.store_timeout = timeout;
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
    /// error naming the token member for a missing or malformed token, and
    /// a 409 `ConflictException` otherwise. A service with errors of its own
    /// (a `ConflictException` with a `reason`) builds them here; the error
    /// must be declared by the operation or the service.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(IdempotencyRefusal) -> crate::Error + Send + Sync + 'static,
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
        let api_version = job.service().version;
        let lease = self.lease_for(job.context());
        let expires = after(now, lease);
        // Made before the store is asked, so a claim whose call fails, times
        // out or is dropped with the request is released by its drop guard:
        // the write may have landed.
        let held = Arc::new(HeldClaim::new(
            ClaimConfig {
                store: self.store.clone(),
                retention: self.retention,
                max_stored: self.max_stored,
                timeout: self.store_timeout,
                lease,
                refusal: self.refusal.clone(),
                clock: self.clock.clone(),
            },
            key,
            uuid::Uuid::new_v4().to_string(),
            api_version,
        ));
        let claim = bounded(
            self.store_timeout,
            self.store
                .claim(held.key(), &fingerprint, held.attempt(), now, expires),
        )
        .await
        .map_err(|error| crate::Error::new(StoreUnavailable(error)))?;
        if claim == Claim::Fresh {
            job.context().set_attribute(Attempt { claim: Some(held) });
            return Ok(());
        }
        held.disown();
        match claim {
            Claim::Replay {
                fingerprint: earlier,
                response,
            } if earlier == fingerprint => {
                let (output, authorization) = match response.read(api_version) {
                    Recorded::Output {
                        output,
                        authorization,
                    } => (output, authorization),
                    Recorded::Failed => {
                        return Err((self.refusal)(IdempotencyRefusal::Failed));
                    }
                    // A response not kept, or written before a deploy that
                    // changed the operation's response, cannot be replayed
                    // faithfully; the caller is told so rather than given a
                    // 500.
                    Recorded::Unkept | Recorded::Unreadable => {
                        return Err((self.refusal)(IdempotencyRefusal::Unreplayable));
                    }
                };
                // The declared checks were made again by `Permissions`,
                // which runs first; the handler checks the first success
                // was allowed are asked again here, and the listing it
                // showed compared. A caller who lost access is refused
                // rather than served the stored success. A record kept
                // without an authorization (the operation was outside
                // `Permissions` then) has no checks and an unknown listing.
                #[cfg(feature = "permissions")]
                if !crate::permissions::reauthorize(
                    job.context(),
                    &authorization.unwrap_or(Value::Null),
                )
                .await?
                {
                    return Err((self.refusal)(IdempotencyRefusal::Unreplayable));
                }
                #[cfg(not(feature = "permissions"))]
                let _ = authorization;
                match job.replay_output(output) {
                    Ok(()) => Ok(()),
                    Err(_) => Err((self.refusal)(IdempotencyRefusal::Unreplayable)),
                }
            }
            Claim::Replay { .. } => Err((self.refusal)(IdempotencyRefusal::Mismatch)),
            Claim::InFlight {
                fingerprint: earlier,
            } if earlier != fingerprint => Err((self.refusal)(IdempotencyRefusal::Mismatch)),
            Claim::InFlight { .. } => Err((self.refusal)(IdempotencyRefusal::InFlight)),
            Claim::Fresh => unreachable!("a fresh claim is held above"),
        }
    }
}

/// Whether the token is one a client may send: at most [`MAX_TOKEN`]
/// visible ASCII characters, which a store can key and a log can show.
fn well_formed(token: &str) -> bool {
    token.len() <= MAX_TOKEN && token.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Whether `operation`'s input streams: a stream is not part of the decoded
/// input's JSON form, so two requests differing only in it would fingerprint
/// alike.
fn input_streams(operation: &OperationInfo) -> bool {
    operation.input.iter().any(|member| {
        matches!(
            member.location,
            Location::StreamingPayload { .. } | Location::EventStream
        )
    })
}

impl Interceptor for Idempotency {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        let metadata = job.metadata();
        let Some(member) = metadata.idempotency_token else {
            return Hook::Ready(Ok(()));
        };
        // One request has one idempotency state: a second `Idempotency`
        // covering the operation would take it over, and the first's claim
        // would never be settled.
        if job.context().attribute::<Attempt>().is_some() {
            return Hook::Ready(Err(crate::Error::internal(format!(
                "two Idempotency interceptors cover {}; scope them apart with `.only(&[..])` or `.except(&[..])`",
                job.operation().name
            ))));
        }
        if !metadata.idempotency_token_sent {
            // An older API version without the token: its clients cannot
            // send one, so its requests run as they always did.
            job.context().set_attribute(Attempt { claim: None });
            return Hook::Ready(Ok(()));
        }
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
        if !well_formed(&token) {
            return Hook::Ready(Err((self.refusal)(IdempotencyRefusal::Invalid {
                member: member.name,
            })));
        }
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
        // A success with a permission obligation unsatisfied fails here,
        // before it could be stored for replay (`Permissions` is registered
        // first, so it unwinds after this).
        #[cfg(feature = "permissions")]
        crate::permissions::settle(job);
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
        // What a replay of the success must be authorized for again: only
        // a stored success needs it.
        #[cfg(feature = "permissions")]
        let authorization = output
            .as_ref()
            .and_then(|_| crate::permissions::stored_checks(job.context()));
        #[cfg(not(feature = "permissions"))]
        let authorization = None;
        Hook::wait(claim.finish(output, authorization))
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        // Records the registration order for `Permissions`, which refuses
        // to follow.
        prepare.builtin("idempotency", "permissions");
        if self.retention.is_zero() || self.lease.is_zero() || self.store_timeout.is_zero() {
            return Err(
                "an idempotency retention, lease and store timeout must be positive".to_owned(),
            );
        }
        let tokened = |operation: &&'static OperationInfo| {
            prepare
                .metadata(operation)
                .is_some_and(|metadata| metadata.idempotency_token.is_some())
        };
        let mut problems = Vec::new();
        let streamed: Vec<&str> = prepare
            .operations()
            .filter(&tokened)
            .filter(|operation| input_streams(operation))
            .map(|operation| operation.name)
            .collect();
        if !streamed.is_empty() {
            problems.push(format!(
                "Idempotency cannot fingerprint the streaming input of {}: a retry with other bytes would replay the first",
                streamed.join(", ")
            ));
        }
        let unreplayable: Vec<&str> = prepare
            .operations()
            .filter(&tokened)
            .filter(|operation| {
                prepare
                    .metadata(operation)
                    .is_some_and(|metadata| !metadata.replays_output)
            })
            .map(|operation| operation.name)
            .collect();
        if !unreplayable.is_empty() {
            problems.push(format!(
                "Idempotency cannot replay the output of {}: it streams, or the bindings predate replay (regenerate them)",
                unreplayable.join(", ")
            ));
        }
        if problems.is_empty() {
            return Ok(());
        }
        Err(format!(
            "{}. Leave such operations out with `.except(&[..])`",
            problems.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use axum::http::{HeaderValue, Request};

    use super::{DEADLINE_GRACE, DEFAULT_LEASE, Idempotency, MAX_TOKEN, well_formed};
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

    #[test]
    fn tokens_are_short_visible_ascii() {
        assert!(well_formed("3f2b8c1e-5d4a-4c6b-9e7f-0a1b2c3d4e5f"));
        assert!(well_formed(&"x".repeat(MAX_TOKEN)));
        assert!(!well_formed(&"x".repeat(MAX_TOKEN + 1)));
        for bad in ["with space", "tab\t", "line\n", "caf\u{e9}", "\u{7f}"] {
            assert!(!well_formed(bad), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn commit_outside_an_idempotent_request_is_a_wiring_error() {
        assert!(crate::idempotency::commit().await.is_err());
    }

    #[test]
    fn abandon_outside_an_idempotent_request_is_a_wiring_error() {
        assert!(crate::idempotency::abandon().is_err());
    }
}
