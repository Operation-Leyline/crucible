use crate::{
    ApiKeyStore, ApiKeyUsageRecorder, AuthError, AuthenticateResult, Authenticator,
    api_key_cache::ApiKeyCache, api_key_core::ApiKeyCore, api_key_digest,
    api_key_policy::ApiKeyPolicy, presented_credential::PresentedCredential,
};
use axum::http::{HeaderName, request::Parts};
use crucible_server_runtime_rust::operation::AuthScheme;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Bearer API keys checked against a service-owned [`ApiKeyStore`], with the
/// caching every store needs: valid keys for a TTL, refused keys with backoff,
/// one read per digest at a time, and a bounded number of reads at once.
///
/// It recognizes only credentials with its prefix (and passing its
/// well-formedness check), so in an [`AuthenticatorChain`](crate::AuthenticatorChain)
/// beside a `JwtAuthenticator` a token falls through to the JWT verifier
/// without a store read. See `docs/auth.md` for the caching and revocation
/// window.
pub struct ApiKeyAuthenticator {
    core: ApiKeyCore,
}

impl ApiKeyAuthenticator {
    /// Keys start with `prefix` (`"pgn_"`). The prefix must be nonempty
    /// printable ASCII without spaces: an empty prefix would claim every token.
    pub fn new(
        prefix: impl Into<String>,
        store: impl ApiKeyStore,
    ) -> std::result::Result<Self, AuthError> {
        let prefix = prefix.into();
        if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(AuthError::Configuration(
                "an API key prefix must be nonempty printable ASCII without spaces".into(),
            ));
        }
        let policy = ApiKeyPolicy::new(prefix);
        Ok(Self {
            core: ApiKeyCore {
                cache: Arc::new(ApiKeyCache::new(&policy)),
                policy: Arc::new(policy),
                store: Arc::new(store),
            },
        })
    }

    /// Configuration happens before the authenticator is shared; the caches
    /// are rebuilt (empty) so capacities and concurrency take effect.
    fn configure(mut self, change: impl FnOnce(&mut ApiKeyPolicy)) -> Self {
        let policy = Arc::make_mut(&mut self.core.policy);
        change(policy);
        self.core.cache = Arc::new(ApiKeyCache::new(policy));
        self
    }

    /// An extra check on the whole credential, prefix included (a checksum,
    /// an alphabet), run before any read. A credential that fails it is not
    /// this authenticator's and falls through to the next one.
    pub fn well_formed(self, check: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.configure(|policy| policy.well_formed = Some(Arc::new(check)))
    }

    /// Headers to read, in order; the first one present is the only one read.
    /// Defaults to `Authorization`. Duplicate values of that header are refused.
    pub fn credential_headers(self, headers: impl IntoIterator<Item = HeaderName>) -> Self {
        let headers = headers.into_iter().collect();
        self.configure(|policy| policy.headers = headers)
    }

    /// Also accept a key with no `Bearer ` scheme. Off by default.
    pub fn allow_bare(self, allow: bool) -> Self {
        self.configure(|policy| policy.allow_bare = allow)
    }

    /// `Principal::issuer` for keys from this store. Defaults to `"api-key"`.
    pub fn issuer(self, issuer: impl Into<String>) -> Self {
        let issuer = issuer.into();
        self.configure(|policy| policy.issuer = issuer)
    }

    /// How long a valid key is served from cache, measured from its read (a hit
    /// does not extend it). This is the longest a revocation made on another
    /// instance goes unseen. Defaults to five minutes. Zero disables the
    /// valid-key cache alone: every request reads the store, so revocation is
    /// immediate, while refusal caching, single flight and the lookup limit
    /// stay on.
    pub fn valid_ttl(self, ttl: Duration) -> Self {
        self.configure(|policy| policy.valid_ttl = ttl)
    }

    /// A hit this close to the end of its TTL is answered from cache and
    /// starts one background re-read that refreshes or evicts the entry.
    /// Defaults to the last fifth of the TTL; zero disables refresh-ahead.
    pub fn refresh_ahead(self, window: Duration) -> Self {
        self.configure(|policy| policy.refresh_ahead = Some(window))
    }

    /// Valid keys cached at once (least recently used evicted). Defaults to 10,000.
    pub fn valid_capacity(self, capacity: usize) -> Self {
        self.configure(|policy| policy.valid_capacity = capacity)
    }

    /// An unknown digest is refused without a read for `initial`, doubling each
    /// time it is still unknown, up to `ceiling`; a revoked, disabled or
    /// expired key is refused for `ceiling` at once. Defaults to 2 s and 60 s,
    /// kept below the valid TTL so a correct answer is cached longer than an
    /// incorrect one. A zero `ceiling` disables refusal caching.
    pub fn refusal_backoff(self, initial: Duration, ceiling: Duration) -> Self {
        self.configure(|policy| {
            policy.refusal_initial = initial;
            policy.refusal_ceiling = ceiling;
        })
    }

    /// Refused digests remembered at once, separately from valid keys so a
    /// flood of forged keys cannot evict them. Defaults to 20,000.
    pub fn refused_capacity(self, capacity: usize) -> Self {
        self.configure(|policy| policy.refused_capacity = capacity)
    }

    /// Store reads in flight at once, across all digests. Defaults to 32; at least 1.
    pub fn lookup_concurrency(self, limit: usize) -> Self {
        self.configure(|policy| policy.lookup_concurrency = limit)
    }

    /// While `fenced` returns true every key is refused without a read or a
    /// cache hit (an unhealthy credential replica).
    pub fn fence(self, fenced: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.configure(|policy| policy.fence = Some(Arc::new(fenced)))
    }

    /// Called with each authenticated key at most once per
    /// [`last_used_resolution`](Self::last_used_resolution), awaited inline.
    pub fn last_used(self, recorder: impl ApiKeyUsageRecorder) -> Self {
        self.configure(|policy| policy.usage = Some(Arc::new(recorder)))
    }

    /// Defaults to one hour. Zero records every use.
    pub fn last_used_resolution(self, resolution: Duration) -> Self {
        self.configure(|policy| policy.last_used_resolution = resolution)
    }

    /// The clock for cache ages and key expiry, for deterministic tests or host integration.
    pub fn clock(self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.configure(|policy| policy.clock = Arc::new(clock))
    }

    /// Drops a digest from the valid and refused caches on this instance, for a
    /// service's revoke or delete path; reads already in flight do not cache
    /// their result. Other instances see the revocation within the valid TTL.
    pub fn forget(&self, digest: &str) {
        self.core.forget(digest);
    }

    /// [`forget`](Self::forget) by credential.
    pub fn forget_credential(&self, credential: &str) {
        self.core.forget(&api_key_digest(credential));
    }
}

#[async_trait::async_trait]
impl Authenticator for ApiKeyAuthenticator {
    fn supports(&self, scheme: &AuthScheme) -> bool {
        matches!(scheme, AuthScheme::Bearer)
    }

    fn has_credentials(&self, request: &Parts, scheme: &AuthScheme) -> bool {
        self.supports(scheme)
            && !matches!(
                self.core.policy.presented(request),
                PresentedCredential::Absent
            )
    }

    async fn authenticate(&self, request: &Parts, _: &AuthScheme) -> AuthenticateResult {
        match self.core.policy.presented(request) {
            PresentedCredential::Key(credential) => self.core.authenticate(credential).await,
            PresentedCredential::Absent | PresentedCredential::Ambiguous => {
                AuthenticateResult::Refused
            }
        }
    }
}
