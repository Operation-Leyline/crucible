use crate::{
    ApiKeyRecord, ApiKeyStore, AuthenticateResult, Principal, TokenKind,
    api_key_cache::ApiKeyCache,
    api_key_digest,
    api_key_policy::ApiKeyPolicy,
    api_key_store_unavailable::ApiKeyStoreUnavailable,
    cached_api_key::CachedApiKey,
    lookup_outcome::LookupOutcome,
    lru_map::LruMap,
    refresh_claim::RefreshClaim,
    refusal::{Refusal, within},
};
use crucible_server_runtime_rust::Error;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime},
};

const LOCK: &str = "API key cache lock";

/// The authenticator's working parts, cheap to clone into a background refresh.
#[derive(Clone)]
pub(crate) struct ApiKeyCore {
    pub policy: Arc<ApiKeyPolicy>,
    pub cache: Arc<ApiKeyCache>,
    pub store: Arc<dyn ApiKeyStore>,
}

impl ApiKeyCore {
    pub async fn authenticate(&self, credential: &str) -> AuthenticateResult {
        // A fence (an unhealthy credential replica) refuses before any cache or
        // read: a cached answer is no better than the replica it came from.
        if self.policy.fence.as_ref().is_some_and(|fenced| fenced()) {
            return AuthenticateResult::Refused;
        }
        let digest = api_key_digest(credential);
        let outcome = match self.cached(&digest, self.now()) {
            Some(outcome) => outcome,
            None => self.lookup(&digest).await,
        };
        match outcome {
            LookupOutcome::Valid(record) => self.admit(&digest, record).await,
            LookupOutcome::Refused => AuthenticateResult::Refused,
            LookupOutcome::Unavailable(error) => {
                AuthenticateResult::Unavailable(Error::new(ApiKeyStoreUnavailable(error)))
            }
        }
    }

    pub fn forget(&self, digest: &str) {
        let mut valid = self.cache.valid.lock().expect(LOCK);
        self.cache.epoch.fetch_add(1, Ordering::SeqCst);
        valid.remove(digest);
        self.cache.refused.lock().expect(LOCK).remove(digest);
        drop(valid);
        // Later requests start a fresh read instead of joining one that began
        // before the revocation.
        self.cache.inflight.lock().expect(LOCK).remove(digest);
    }

    fn now(&self) -> SystemTime {
        (self.policy.clock)()
    }

    /// The cached answer, or `None` when the store must be read.
    fn cached(&self, digest: &str, now: SystemTime) -> Option<LookupOutcome> {
        let policy = &self.policy;
        let mut valid = self.cache.valid.lock().expect(LOCK);
        let hit = valid.get(digest).map(|entry| {
            (
                within(now, entry.read_at, policy.valid_ttl),
                entry.read_at,
                Arc::clone(&entry.record),
            )
        });
        match hit {
            Some((true, read_at, record)) if record.admits(now) => {
                drop(valid);
                if policy.refresh_due(now, read_at) {
                    self.refresh(digest);
                }
                return Some(LookupOutcome::Valid(record));
            }
            Some((true, _, _)) => {
                // The record's own expiry passed since it was read.
                valid.remove(digest);
                let mut refused = self.cache.refused.lock().expect(LOCK);
                refuse(&mut refused, digest, now, policy.refusal_ceiling);
                return Some(LookupOutcome::Refused);
            }
            Some((false, _, _)) => {
                valid.remove(digest);
            }
            None => {}
        }
        drop(valid);
        let mut refused = self.cache.refused.lock().expect(LOCK);
        refused
            .get(digest)
            .is_some_and(|refusal| refusal.active(now))
            .then_some(LookupOutcome::Refused)
    }

    /// Reads the store once for every concurrent request for this digest.
    async fn lookup(&self, digest: &str) -> LookupOutcome {
        let cell = {
            let mut inflight = self.cache.inflight.lock().expect(LOCK);
            Arc::clone(inflight.entry(digest.to_owned()).or_default())
        };
        // A cancelled reader leaves the cell empty; the next waiter reads instead.
        cell.get_or_init(|| async {
            let outcome = self.read(digest).await;
            let mut inflight = self.cache.inflight.lock().expect(LOCK);
            if inflight
                .get(digest)
                .is_some_and(|current| Arc::ptr_eq(current, &cell))
            {
                inflight.remove(digest);
            }
            outcome
        })
        .await
        .clone()
    }

    async fn read(&self, digest: &str) -> LookupOutcome {
        let epoch = self.cache.epoch.load(Ordering::SeqCst);
        let result = {
            let Ok(_permit) = self.cache.lookups.acquire().await else {
                return LookupOutcome::Unavailable(Arc::new(Error::internal(
                    "API key lookups closed",
                )));
            };
            self.store.lookup(digest).await
        };
        let now = self.now();
        let policy = &self.policy;
        let mut valid = self.cache.valid.lock().expect(LOCK);
        let current = self.cache.epoch.load(Ordering::SeqCst) == epoch;
        match result {
            // Nothing is cached: an outage is not evidence about the key, and a
            // valid entry keeps serving until its TTL runs out.
            Err(error) => LookupOutcome::Unavailable(Arc::new(error)),
            Ok(Some(record)) if record.admits(now) => {
                let record = Arc::new(record);
                if current {
                    self.cache.refused.lock().expect(LOCK).remove(digest);
                    if !policy.valid_ttl.is_zero() {
                        valid.insert(
                            digest,
                            CachedApiKey {
                                record: Arc::clone(&record),
                                read_at: now,
                            },
                        );
                    }
                }
                LookupOutcome::Valid(record)
            }
            Ok(found) => {
                valid.remove(digest);
                if current {
                    let mut refused = self.cache.refused.lock().expect(LOCK);
                    // A revoked, disabled or expired row will not come back:
                    // straight to the ceiling. An unknown digest backs off.
                    let lasts = if found.is_some() {
                        policy.refusal_ceiling
                    } else {
                        refused
                            .peek(digest)
                            .map_or(policy.refusal_initial, |previous| {
                                previous.lasts.saturating_mul(2)
                            })
                            .min(policy.refusal_ceiling)
                    };
                    refuse(&mut refused, digest, now, lasts);
                }
                LookupOutcome::Refused
            }
        }
    }

    /// One background re-read per digest. On Lambda the task may only run
    /// when a later invocation thaws the process; the TTL still bounds how
    /// long the entry it would refresh is served.
    fn refresh(&self, digest: &str) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if !self
            .cache
            .refreshing
            .lock()
            .expect(LOCK)
            .insert(digest.to_owned())
        {
            return;
        }
        let claim = RefreshClaim {
            cache: Arc::clone(&self.cache),
            digest: digest.to_owned(),
        };
        let core = self.clone();
        runtime.spawn(async move {
            core.lookup(&claim.digest).await;
            drop(claim);
        });
    }

    async fn admit(&self, digest: &str, record: Arc<ApiKeyRecord>) -> AuthenticateResult {
        if let Some(usage) = &self.policy.usage {
            let now = self.now();
            let due = {
                let mut used = self.cache.used.lock().expect(LOCK);
                let recent = used
                    .get(digest)
                    .is_some_and(|at| within(now, *at, self.policy.last_used_resolution));
                // Marked before the call: a failed write is not retried within the window.
                if !recent {
                    used.insert(digest, now);
                }
                !recent
            };
            if due {
                usage.record_use(digest, &record).await;
            }
        }
        AuthenticateResult::Authenticated(
            Principal::new(
                record.key_id.clone(),
                self.policy.issuer.clone(),
                TokenKind::ApiKey,
            )
            .with_tenant(record.tenant.clone())
            .with_claims(record.attributes.clone()),
        )
    }
}

/// A zero duration caches nothing, which is how refusal caching is disabled.
fn refuse(refused: &mut LruMap<Refusal>, digest: &str, now: SystemTime, lasts: Duration) {
    if lasts.is_zero() {
        refused.remove(digest);
    } else {
        refused.insert(digest, Refusal { at: now, lasts });
    }
}
