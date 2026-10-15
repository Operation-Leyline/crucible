use crate::{
    Algorithm, AuthError, AuthenticateResult, Authenticator, ClientClaim, IssuerConfig, Principal,
};
use crate::{jwks_cache::JwksCache, signing_key::SigningKey};
use axum::http::{HeaderName, header, request::Parts};
use crucible_server_runtime_rust::operation::AuthScheme;
use jsonwebtoken::{DecodingKey, Validation, decode, decode_header, jwk::Jwk};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

/// The shortest gap between fetches that a key ID no fetch has yet been
/// found to lack may cause (capped by `min_refresh_interval`). Such a `kid`
/// is unverified (a rotated key, or a forged one), so this is what an
/// attacker minting random key IDs can make the authenticator fetch: about
/// one document a second, whatever the request rate.
const UNKNOWN_KID_FLOOR: Duration = Duration::from_secs(1);

/// One issuer's isolated trust store. Neither `iss` nor `jku` from a token selects a URL.
pub struct JwtAuthenticator {
    config: IssuerConfig,
    client: reqwest::Client,
    cache: RwLock<JwksCache>,
    refresh: Mutex<()>,
    credential_headers: Vec<HeaderName>,
    clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
    rsa_validation: Validation,
    ec_validation: Validation,
}

impl JwtAuthenticator {
    pub(crate) fn issuer(&self) -> &str {
        &self.config.issuer
    }

    pub(crate) fn mcp_access_policy(&self) -> bool {
        self.credential_headers == [header::AUTHORIZATION]
            && self
                .config
                .token_rules
                .iter()
                .all(|rule| rule.kind == crate::TokenKind::Access)
    }
    pub fn new(config: IssuerConfig) -> std::result::Result<Self, AuthError> {
        validate_url(&config, &config.issuer)?;
        validate_url(&config, &config.jwks_url)?;
        if config.algorithms.is_empty()
            || config
                .algorithms
                .iter()
                .any(|algorithm| !matches!(algorithm, Algorithm::RS256 | Algorithm::ES256))
        {
            return Err(AuthError::Configuration(
                "only RS256 and ES256 are supported".into(),
            ));
        }
        if config.token_rules.is_empty()
            || config.token_rules.iter().any(|rule| {
                rule.allowed_clients.is_empty()
                    || rule.allowed_clients.iter().any(|id| id.trim().is_empty())
            })
        {
            return Err(AuthError::Configuration(
                "every token rule needs allowed clients".into(),
            ));
        }
        let kind = &config.token_rules[0].kind;
        if config.token_rules.iter().any(|rule| &rule.kind != kind) {
            return Err(AuthError::Configuration(
                "install separate authenticators for distinct token kinds".into(),
            ));
        }
        if config.token_rules.iter().any(|rule| {
            rule.kind == crate::TokenKind::Access
                && (!matches!(rule.client_claim, ClientClaim::ClientId)
                    || !rule
                        .required_claims
                        .iter()
                        .any(|(name, value)| name == "token_use" && value == "access"))
        }) {
            return Err(AuthError::Configuration(
                "access tokens require client_id and token_use=access".into(),
            ));
        }
        if config.cache_ttl.is_zero() || config.min_refresh_interval.is_zero() {
            return Err(AuthError::Configuration(
                "cache and refresh intervals must be positive".into(),
            ));
        }
        // reqwest's no-provider mode must also work when a consumer never constructs a Rust client.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        // jsonwebtoken requires each Validation's allowlist to contain only one key family.
        // Keep both prepared; the trusted JWK family must match before choosing either.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&config.issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        // Time checks use the configured clock. Client claims are checked against complete rules.
        validation.validate_nbf = false;
        validation.validate_exp = false;
        validation.validate_aud = false;
        let mut ec_validation = validation.clone();
        ec_validation.algorithms = vec![Algorithm::ES256];
        Ok(Self {
            config,
            client,
            cache: RwLock::new(JwksCache::default()),
            refresh: Mutex::new(()),
            credential_headers: vec![header::AUTHORIZATION],
            clock: Arc::new(SystemTime::now),
            rsa_validation: validation,
            ec_validation,
        })
    }

    /// The one header to read the bearer token from. Defaults to
    /// `Authorization`; [`credential_headers`](Self::credential_headers) is the
    /// ordered form.
    pub fn credential_header(mut self, header: HeaderName) -> Self {
        self.credential_headers = vec![header];
        self
    }

    /// Headers to read the bearer token from, in order; the first one present
    /// is the only one read, so a later header never rescues a refused token.
    /// Duplicate values of that header are refused. An empty list is a
    /// configuration error: it would never see a token.
    pub fn credential_headers(
        mut self,
        headers: impl IntoIterator<Item = HeaderName>,
    ) -> std::result::Result<Self, AuthError> {
        let headers: Vec<_> = headers.into_iter().collect();
        if headers.is_empty() {
            return Err(AuthError::Configuration(
                "JWT credential headers must not be empty".into(),
            ));
        }
        self.credential_headers = headers;
        Ok(self)
    }

    fn credential_header_present(&self, request: &Parts) -> Option<&HeaderName> {
        self.credential_headers
            .iter()
            .find(|name| request.headers.contains_key(*name))
    }

    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Validates and pins a public key set for this authenticator's lifetime, without fetching.
    /// Cache TTL and stale limits do not apply; unknown keys are refused without network access.
    /// Replace the authenticator to rotate or revoke keys. Key material must come from trusted
    /// configuration, never a token. Token expiry and all claim checks still apply.
    /// A key that does not parse is a configuration error here, unlike in a
    /// fetched set, where it is skipped.
    pub fn with_jwks(config: IssuerConfig, jwks: Value) -> std::result::Result<Self, AuthError> {
        let authenticator = Self::new(config)?;
        let (keys, malformed) = parse_keys(&jwks)?;
        if let Some((kid, error)) = malformed.first() {
            return Err(AuthError::Configuration(format!(
                "JWKS key {kid:?} does not parse: {error}"
            )));
        }
        authenticator.store(keys);
        authenticator.cache.write().expect("JWKS cache lock").pinned = true;
        Ok(authenticator)
    }

    /// Installs a fetched key set. One key that does not parse (a provider
    /// publishing a key type this crate cannot read, or a corrupt entry) is
    /// skipped with a warning rather than costing every other key.
    fn install(&self, jwks: Value) -> std::result::Result<(), AuthError> {
        let (keys, malformed) = parse_keys(&jwks)?;
        for (kid, error) in malformed {
            tracing::warn!(
                issuer = %self.config.issuer,
                kid,
                error,
                "skipping a JWKS key that does not parse"
            );
        }
        self.store(keys);
        Ok(())
    }

    fn store(&self, keys: BTreeMap<String, SigningKey>) {
        let mut cache = self.cache.write().expect("JWKS cache lock");
        cache.keys = keys;
        cache.fetched = Some(Instant::now());
    }

    fn cached_key(&self, kid: &str, algorithm: Algorithm) -> Option<Arc<DecodingKey>> {
        self.cached_key_with_age(kid, algorithm, self.config.cache_ttl)
    }

    fn stale_key(&self, kid: &str, algorithm: Algorithm) -> Option<Arc<DecodingKey>> {
        self.cached_key_with_age(
            kid,
            algorithm,
            self.config.cache_ttl.saturating_add(self.config.max_stale),
        )
    }

    fn cached_key_with_age(
        &self,
        kid: &str,
        algorithm: Algorithm,
        max_age: std::time::Duration,
    ) -> Option<Arc<DecodingKey>> {
        let cache = self.cache.read().expect("JWKS cache lock");
        if !cache.pinned && !cache.fetched.is_some_and(|time| time.elapsed() < max_age) {
            return None;
        }
        cache
            .keys
            .get(kid)
            .filter(|key| key.algorithm == algorithm)
            .map(|key| key.decoding.clone())
    }

    async fn key(
        &self,
        kid: &str,
        algorithm: Algorithm,
    ) -> std::result::Result<Arc<DecodingKey>, AuthError> {
        {
            let cache = self.cache.read().expect("JWKS cache lock");
            if cache.pinned {
                return cache
                    .keys
                    .get(kid)
                    .filter(|key| key.algorithm == algorithm)
                    .map(|key| key.decoding.clone())
                    .ok_or(AuthError::Refused);
            }
        }
        if self
            .cache
            .read()
            .expect("JWKS cache lock")
            .keys
            .get(kid)
            .is_some_and(|key| key.algorithm != algorithm)
        {
            // A known key's family is provider configuration, never a choice made by the token.
            return Err(AuthError::Refused);
        }
        if let Some(key) = self.cached_key(kid, algorithm) {
            return Ok(key);
        }
        // Serialize refreshes, then recheck: all requests waiting for a rotation reuse its result.
        let _refresh = self.refresh.lock().await;
        if let Some(key) = self.cached_key(kid, algorithm) {
            return Ok(key);
        }
        {
            let mut cache = self.cache.write().expect("JWKS cache lock");
            if self.cooling_down(&cache, kid) {
                let stale = cache.fetched.is_some_and(|time| {
                    time.elapsed() < self.config.cache_ttl.saturating_add(self.config.max_stale)
                });
                if stale
                    && let Some(key) = cache.keys.get(kid).filter(|key| key.algorithm == algorithm)
                {
                    return Ok(key.decoding.clone());
                }
                return if cache
                    .fetched
                    .is_some_and(|time| time.elapsed() < self.config.cache_ttl)
                {
                    Err(AuthError::Refused)
                } else {
                    Err(AuthError::Unavailable(
                        "JWKS refresh is cooling down".into(),
                    ))
                };
            }
            // Failed requests are throttled too; an outage cannot amplify incoming traffic.
            cache.last_attempt = Some(Instant::now());
        }
        let url = &self.config.jwks_url;
        let refreshed = match self.fetch(url).await {
            Ok(jwks) => self.install(jwks),
            Err(error) => Err(error),
        };
        self.cache.write().expect("JWKS cache lock").failed = refreshed.is_err();
        if let Err(error) = refreshed {
            return self.stale_key(kid, algorithm).ok_or(error);
        }
        self.cached_key(kid, algorithm).ok_or_else(|| {
            self.cache
                .write()
                .expect("JWKS cache lock")
                .absent
                .insert(kid, Instant::now());
            AuthError::Refused
        })
    }

    /// Whether a lookup for `kid` must not fetch yet. A failed fetch holds
    /// every fetch off for `min_refresh_interval`, so an outage cannot
    /// amplify traffic; so does a `kid` the last fetches did not contain.
    /// A `kid` not looked up yet waits only for [`UNKNOWN_KID_FLOOR`]: an
    /// unverified token with a random `kid` cannot then hold a newly rotated
    /// key off for the whole interval.
    fn cooling_down(&self, cache: &JwksCache, kid: &str) -> bool {
        let Some(last) = cache.last_attempt else {
            return false;
        };
        let interval = self.config.min_refresh_interval;
        let since = last.elapsed();
        if cache.failed {
            return since < interval;
        }
        if cache
            .absent
            .peek(kid)
            .is_some_and(|at| at.elapsed() < interval)
        {
            return true;
        }
        since < interval.min(UNKNOWN_KID_FLOOR)
    }

    async fn fetch(&self, url: &str) -> std::result::Result<Value, AuthError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| AuthError::Unavailable(error.to_string()))?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| AuthError::Unavailable(error.to_string()))?
        {
            if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
                return Err(AuthError::Unavailable(
                    "identity document exceeds 1 MiB".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|error| AuthError::Unavailable(error.to_string()))
    }

    pub async fn verify(&self, token: &str) -> std::result::Result<Principal, AuthError> {
        if token.len() > 64 * 1024 {
            return Err(AuthError::Refused);
        }
        let header = decode_header(token).map_err(|_| AuthError::Refused)?;
        if !self.config.algorithms.contains(&header.alg) {
            return Err(AuthError::Refused);
        }
        let kid = header
            .kid
            .as_deref()
            .filter(|kid| !kid.is_empty() && kid.len() <= 256)
            .ok_or(AuthError::Refused)?;
        // Only refuse based on untrusted claims: acceptance still validates the signed issuer.
        // Foreign tokens must not fetch this issuer's keys or consume its refresh cooldown.
        if jsonwebtoken::dangerous::insecure_decode_claims::<Value>(token)
            .map_err(|_| AuthError::Refused)?
            .get("iss")
            .and_then(Value::as_str)
            != Some(self.config.issuer.as_str())
        {
            return Err(AuthError::Refused);
        }
        let key = self.key(kid, header.alg).await?;
        let validation = if header.alg == Algorithm::RS256 {
            &self.rsa_validation
        } else {
            &self.ec_validation
        };
        let claims = decode::<Value>(token, &key, validation)
            .map_err(|_| AuthError::Refused)?
            .claims;
        let subject = claims
            .get("sub")
            .and_then(Value::as_str)
            .filter(|sub| !sub.is_empty())
            .ok_or(AuthError::Refused)?;
        let now = (self.clock)()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Unavailable("system clock before epoch".into()))?
            .as_secs();
        let expires = claims
            .get("exp")
            .and_then(Value::as_u64)
            .ok_or(AuthError::Refused)?;
        if now >= expires.saturating_add(self.config.leeway) {
            return Err(AuthError::Refused);
        }
        if let Some(value) = claims.get("nbf") {
            let not_before = value.as_u64().ok_or(AuthError::Refused)?;
            if now.saturating_add(self.config.leeway) < not_before {
                return Err(AuthError::Refused);
            }
        }
        let (rule, client_id) = self
            .config
            .token_rules
            .iter()
            .find_map(|rule| {
                if rule
                    .header_type
                    .as_ref()
                    .is_some_and(|typ| header.typ.as_ref() != Some(typ))
                    || rule.required_claims.iter().any(|(name, value)| {
                        claims.get(name).and_then(Value::as_str) != Some(value)
                    })
                {
                    return None;
                }
                let client = match rule.client_claim {
                    ClientClaim::Audience => {
                        let audience = claims.get("aud")?;
                        // Reject malformed array members, even if another member matches.
                        let values = if let Some(text) = audience.as_str() {
                            vec![text]
                        } else {
                            audience
                                .as_array()?
                                .iter()
                                .map(Value::as_str)
                                .collect::<Option<Vec<_>>>()?
                        };
                        values.into_iter().find(|value| {
                            rule.allowed_clients.iter().any(|allowed| allowed == value)
                        })?
                    }
                    ClientClaim::ClientId => {
                        let value = claims.get("client_id")?.as_str()?;
                        if !rule.allowed_clients.iter().any(|allowed| allowed == value) {
                            return None;
                        }
                        value
                    }
                };
                Some((rule, client.to_owned()))
            })
            .ok_or(AuthError::Refused)?;
        let groups = match claims.get("cognito:groups") {
            None => BTreeSet::new(),
            Some(value) => value
                .as_array()
                .ok_or(AuthError::Refused)?
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<BTreeSet<_>>>()
                .ok_or(AuthError::Refused)?,
        };
        // No tenant: a token claim never binds the credential to a tenant.
        Ok(
            Principal::new(subject, self.config.issuer.clone(), rule.kind.clone())
                .with_client_id(client_id)
                .with_groups(groups)
                .with_claims(claims),
        )
    }
}

#[async_trait::async_trait]
impl Authenticator for JwtAuthenticator {
    fn supports(&self, scheme: &AuthScheme) -> bool {
        matches!(scheme, AuthScheme::Bearer)
    }

    fn has_credentials(&self, request: &Parts, _: &AuthScheme) -> bool {
        self.credential_header_present(request).is_some()
    }

    async fn authenticate(&self, request: &Parts, _: &AuthScheme) -> AuthenticateResult {
        let Some(name) = self.credential_header_present(request) else {
            return AuthenticateResult::Refused;
        };
        let mut headers = request.headers.get_all(name).iter();
        let Some(value) = headers.next() else {
            return AuthenticateResult::Refused;
        };
        if headers.next().is_some() {
            return AuthenticateResult::Refused;
        }
        let Ok(value) = value.to_str() else {
            return AuthenticateResult::Refused;
        };
        let Some((scheme, token)) = value.split_once(' ') else {
            return AuthenticateResult::Refused;
        };
        if !scheme.eq_ignore_ascii_case("Bearer")
            || token.is_empty()
            || token.contains(char::is_whitespace)
        {
            return AuthenticateResult::Refused;
        }
        match self.verify(token).await {
            Ok(principal) => AuthenticateResult::Authenticated(principal),
            Err(AuthError::Refused) => AuthenticateResult::Refused,
            Err(error) => AuthenticateResult::Unavailable(error.into()),
        }
    }
}

fn validate_url(config: &IssuerConfig, value: &str) -> std::result::Result<(), AuthError> {
    let url =
        reqwest::Url::parse(value).map_err(|error| AuthError::Configuration(error.to_string()))?;
    let insecure = {
        #[cfg(feature = "test-support")]
        {
            config.allow_insecure_http
                && url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        }
        #[cfg(not(feature = "test-support"))]
        {
            let _ = config;
            false
        }
    };
    if (url.scheme() != "https" && !insecure)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(AuthError::Configuration(
            "issuer/JWKS URLs must be HTTPS without credentials, query or fragment".into(),
        ));
    }
    Ok(())
}

/// The signing keys, and each key that does not parse (its `kid` and why).
type ParsedKeys = (BTreeMap<String, SigningKey>, Vec<(String, String)>);

fn parse_keys(value: &Value) -> std::result::Result<ParsedKeys, AuthError> {
    let values = value
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| AuthError::Unavailable("JWKS keys must be an array".into()))?;
    let mut keys = BTreeMap::new();
    let mut malformed = Vec::new();
    // Every kid a signing entry names, parsed or not: a duplicate is refused
    // even when one of the two would be skipped as malformed, so which entry
    // the set means is never decided by which one happens to parse.
    let mut named = BTreeSet::new();
    for value in values {
        if value
            .get("use")
            .is_some_and(|usage| usage.as_str() != Some("sig"))
        {
            continue;
        }
        if let Some(operations) = value.get("key_ops")
            && !operations
                .as_array()
                .is_some_and(|ops| ops.iter().any(|op| op.as_str() == Some("verify")))
        {
            continue;
        }
        let algorithm = match value.get("kty").and_then(Value::as_str) {
            Some("RSA") => Algorithm::RS256,
            Some("EC") if value.get("crv").and_then(Value::as_str) == Some("P-256") => {
                Algorithm::ES256
            }
            _ => continue,
        };
        let name = if algorithm == Algorithm::RS256 {
            "RS256"
        } else {
            "ES256"
        };
        if value
            .get("alg")
            .is_some_and(|alg| alg.as_str() != Some(name))
        {
            continue;
        }
        let Some(kid) = value
            .get("kid")
            .and_then(Value::as_str)
            .filter(|kid| !kid.is_empty())
        else {
            continue;
        };
        if !named.insert(kid.to_owned()) {
            return Err(AuthError::Unavailable("JWKS contains duplicate kid".into()));
        }
        let decoding = match serde_json::from_value::<Jwk>(value.clone())
            .map_err(|error| error.to_string())
            .and_then(|jwk| DecodingKey::from_jwk(&jwk).map_err(|error| error.to_string()))
        {
            Ok(decoding) => decoding,
            Err(error) => {
                malformed.push((kid.to_owned(), error));
                continue;
            }
        };
        keys.insert(
            kid.to_owned(),
            SigningKey {
                algorithm,
                decoding: Arc::new(decoding),
            },
        );
    }
    if keys.is_empty() {
        return Err(AuthError::Unavailable(
            "JWKS contains no supported signing keys".into(),
        ));
    }
    Ok((keys, malformed))
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::JwtAuthenticator;
    use crate::{AuthError, test_support::TestTokenIssuer};
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn pinned_keys_survive_cache_expiry_without_fetching() {
        let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
        let config = issuer.config(vec!["client".into()]);
        let age = config.cache_ttl + config.max_stale + Duration::from_secs(1);
        let auth = JwtAuthenticator::with_jwks(config, issuer.jwks()).unwrap();
        auth.cache.write().unwrap().fetched = Some(Instant::now() - age);
        let claims = issuer.id_claims("person", "client");
        assert!(auth.verify(&issuer.mint(&claims).unwrap()).await.is_ok());

        let mut unknown = TestTokenIssuer::es256("https://issuer.example", "unknown").unwrap();
        assert!(matches!(
            auth.verify(&unknown.mint(&claims).unwrap()).await,
            Err(AuthError::Refused)
        ));
        unknown.kid = "known".into();
        assert!(matches!(
            auth.verify(&unknown.mint(&claims).unwrap()).await,
            Err(AuthError::Refused)
        ));
        let mut expired = claims;
        expired["exp"] = serde_json::json!(0);
        assert!(matches!(
            auth.verify(&issuer.mint(&expired).unwrap()).await,
            Err(AuthError::Refused)
        ));
        assert!(auth.cache.read().unwrap().last_attempt.is_none());
    }

    #[test]
    fn an_unseen_kid_waits_only_for_the_floor_but_a_missing_one_or_an_outage_waits_the_interval() {
        let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
        let config = issuer.config(vec!["client".into()]);
        let interval = config.min_refresh_interval;
        let auth = JwtAuthenticator::new(config).unwrap();
        let mut cache = auth.cache.write().unwrap();
        assert!(!auth.cooling_down(&cache, "new"), "never fetched");
        // A successful fetch just now, which a forged `kid` caused.
        cache.last_attempt = Some(Instant::now());
        cache.absent.insert("forged", Instant::now());
        assert!(auth.cooling_down(&cache, "new"), "inside the floor");
        cache.last_attempt = Some(Instant::now() - super::UNKNOWN_KID_FLOOR);
        assert!(
            !auth.cooling_down(&cache, "new"),
            "a rotated key is fetched once the floor has passed"
        );
        assert!(
            auth.cooling_down(&cache, "forged"),
            "a kid the last fetch lacked waits the whole interval"
        );
        cache.absent.insert("forged", Instant::now() - interval);
        assert!(!auth.cooling_down(&cache, "forged"));
        // An outage holds every fetch off for the whole interval.
        cache.failed = true;
        assert!(auth.cooling_down(&cache, "new"));
        cache.last_attempt = Some(Instant::now() - interval);
        assert!(!auth.cooling_down(&cache, "new"));
    }

    #[test]
    fn a_key_that_does_not_parse_is_skipped_in_a_fetched_set_and_refused_in_configuration() {
        let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
        let config = issuer.config(vec!["client".into()]);
        let mut jwks = issuer.jwks();
        let keys = jwks["keys"].as_array_mut().unwrap();
        // A missing modulus does not deserialize; a modulus that is not
        // base64url does not decode.
        keys.insert(0, serde_json::json!({"kty":"RSA","kid":"no-n","e":"AQAB"}));
        keys.push(serde_json::json!({"kty":"RSA","kid":"bad-n","n":"***","e":"AQAB"}));
        let auth = JwtAuthenticator::new(config.clone()).unwrap();
        auth.install(jwks.clone()).unwrap();
        let cache = auth.cache.read().unwrap();
        assert_eq!(cache.keys.keys().collect::<Vec<_>>(), ["known"]);
        drop(cache);
        assert!(matches!(
            JwtAuthenticator::with_jwks(config, jwks.clone()),
            Err(AuthError::Configuration(_))
        ));
        // A malformed entry sharing a good key's kid still makes the set
        // ambiguous, whichever comes first.
        let good = issuer.jwks()["keys"][0].clone();
        let twin = serde_json::json!({"kty":"EC","crv":"P-256","kid":"known","x":"***","y":"***"});
        for keys in [vec![good.clone(), twin.clone()], vec![twin, good]] {
            let jwks = serde_json::json!({ "keys": keys });
            assert!(matches!(auth.install(jwks), Err(AuthError::Unavailable(_))));
        }
    }

    #[test]
    fn remotely_installed_keys_still_expire() {
        let issuer = TestTokenIssuer::es256("https://issuer.example", "known").unwrap();
        let config = issuer.config(vec!["client".into()]);
        let ttl = config.cache_ttl;
        let max_age = ttl + config.max_stale;
        let auth = JwtAuthenticator::new(config).unwrap();
        auth.install(issuer.jwks()).unwrap();
        assert!(auth.cached_key("known", crate::Algorithm::ES256).is_some());
        auth.cache.write().unwrap().fetched = Some(Instant::now() - ttl);
        assert!(auth.cached_key("known", crate::Algorithm::ES256).is_none());
        assert!(auth.stale_key("known", crate::Algorithm::ES256).is_some());
        auth.cache.write().unwrap().fetched = Some(Instant::now() - max_age);
        assert!(auth.stale_key("known", crate::Algorithm::ES256).is_none());
    }
}
