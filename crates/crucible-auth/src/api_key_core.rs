use crate::{
    ApiKeyRecord, ApiKeyStore, AuthenticateResult, Principal, TokenKind,
    api_key_cache::ApiKeyCache,
    api_key_digest,
    api_key_policy::ApiKeyPolicy,
    api_key_store_unavailable::ApiKeyStoreUnavailable,
    cached_api_key::CachedApiKey,
    inflight_waiter::InflightWaiter,
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
        // A fence (an unhealthy credential replica) answers before any cache or
        // read: a cached answer is no better than the replica it came from. It
        // is an outage (5xx), not a verdict on the key: valid keys must not be
        // told they are invalid.
        if self.policy.fence.as_ref().is_some_and(|fenced| fenced()) {
            return AuthenticateResult::Unavailable(Error::internal(
                "API key lookups fenced: credential store degraded",
            ));
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

    #[cfg(test)]
    pub fn inflight(&self) -> usize {
        self.cache.inflight.lock().expect(LOCK).len()
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
    ///
    /// An answer is only as fresh as the moment its read started. One older
    /// than the valid TTL when it arrives (a slow store, or a refresh paused by
    /// a Lambda freeze that a later request joined) is not served: the read is
    /// repeated once, and a second stale answer is an outage.
    async fn lookup(&self, digest: &str) -> LookupOutcome {
        for _ in 0..2 {
            let waiter = InflightWaiter::join(&self.cache.inflight, digest);
            // A cancelled reader leaves the cell empty; the next waiter reads instead.
            let (read_at, outcome) = waiter
                .cell
                .get_or_init(|| async {
                    let read = self.read(digest).await;
                    waiter.finish();
                    read
                })
                .await
                .clone();
            if !self.too_old(read_at, self.now()) {
                return outcome;
            }
        }
        LookupOutcome::Unavailable(Arc::new(Error::internal(
            "API key store reads take longer than the valid-key TTL",
        )))
    }

    /// With no valid-key cache there is no staleness bound to keep: the
    /// answer goes only to the requests that were waiting for it.
    fn too_old(&self, read_at: SystemTime, now: SystemTime) -> bool {
        let ttl = self.policy.valid_ttl;
        !ttl.is_zero() && now.duration_since(read_at).is_ok_and(|age| age >= ttl)
    }

    async fn read(&self, digest: &str) -> (SystemTime, LookupOutcome) {
        let epoch = self.cache.epoch.load(Ordering::SeqCst);
        let (read_at, result) = {
            let Ok(_permit) = self.cache.lookups.acquire().await else {
                let error = Arc::new(Error::internal("API key lookups closed"));
                return (self.now(), LookupOutcome::Unavailable(error));
            };
            // Stamped before the read, not when the reply arrives: the reply
            // may describe the row as it was at any point since.
            let read_at = self.now();
            (read_at, self.store.lookup(digest).await)
        };
        let now = self.now();
        (read_at, self.settle(digest, epoch, read_at, now, result))
    }

    /// Applies a finished read to the caches and decides its outcome.
    fn settle(
        &self,
        digest: &str,
        epoch: u64,
        read_at: SystemTime,
        now: SystemTime,
        result: Result<Option<ApiKeyRecord>, Error>,
    ) -> LookupOutcome {
        let policy = &self.policy;
        let mut valid = self.cache.valid.lock().expect(LOCK);
        // A read that started before a forget, or whose answer is already too
        // old, caches nothing; evicting stays safe either way.
        let current =
            self.cache.epoch.load(Ordering::SeqCst) == epoch && !self.too_old(read_at, now);
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
                                read_at,
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
                    refuse(&mut refused, digest, read_at, lasts);
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

#[cfg(test)]
mod tests;
