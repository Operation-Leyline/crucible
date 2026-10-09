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
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

/// One issuer's isolated trust store. Neither `iss` nor `jku` from a token selects a URL.
pub struct JwtAuthenticator {
    config: IssuerConfig,
    client: reqwest::Client,
    cache: RwLock<JwksCache>,
    refresh: Mutex<()>,
    credential_header: HeaderName,
    clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
    rsa_validation: Validation,
    ec_validation: Validation,
}

impl JwtAuthenticator {
    pub(crate) fn issuer(&self) -> &str {
        &self.config.issuer
    }

    pub(crate) fn mcp_access_policy(&self) -> bool {
        self.credential_header == header::AUTHORIZATION
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
            credential_header: header::AUTHORIZATION,
            clock: Arc::new(SystemTime::now),
            rsa_validation: validation,
            ec_validation,
        })
    }

    pub fn credential_header(mut self, header: HeaderName) -> Self {
        self.credential_header = header;
        self
    }

    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Validates and pins a public key set for this authenticator's lifetime, without fetching.
    /// Cache TTL and stale limits do not apply; unknown keys are refused without network access.
    /// Replace the authenticator to rotate or revoke keys. Key material must come from trusted
    /// configuration, never a token. Token expiry and all claim checks still apply.
    pub fn with_jwks(config: IssuerConfig, jwks: Value) -> std::result::Result<Self, AuthError> {
        let authenticator = Self::new(config)?;
        authenticator.install(jwks)?;
        authenticator.cache.write().expect("JWKS cache lock").pinned = true;
        Ok(authenticator)
    }

    fn install(&self, jwks: Value) -> std::result::Result<(), AuthError> {
        let keys = parse_keys(&jwks)?;
        let mut cache = self.cache.write().expect("JWKS cache lock");
        cache.keys = keys;
        cache.fetched = Some(Instant::now());
        Ok(())
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
            if cache
                .last_attempt
                .is_some_and(|time| time.elapsed() < self.config.min_refresh_interval)
            {
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
        if let Err(error) = refreshed {
            return self.stale_key(kid, algorithm).ok_or(error);
        }
        self.cached_key(kid, algorithm).ok_or(AuthError::Refused)
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
        let scopes = match claims.get("scope") {
            None => BTreeSet::new(),
            Some(value) => value
                .as_str()
                .ok_or(AuthError::Refused)?
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        };
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
        Ok(Principal {
            subject: subject.to_owned(),
            issuer: self.config.issuer.clone(),
            client_id: Some(client_id),
            token_kind: rule.kind.clone(),
            scopes,
            groups,
            claims,
        })
    }
}

#[async_trait::async_trait]
impl Authenticator for JwtAuthenticator {
    fn supports(&self, scheme: &AuthScheme) -> bool {
        matches!(scheme, AuthScheme::Bearer)
    }

    fn has_credentials(&self, request: &Parts, _: &AuthScheme) -> bool {
        request.headers.contains_key(&self.credential_header)
    }

    async fn authenticate(&self, request: &Parts, _: &AuthScheme) -> AuthenticateResult {
        let mut headers = request.headers.get_all(&self.credential_header).iter();
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

fn parse_keys(value: &Value) -> std::result::Result<BTreeMap<String, SigningKey>, AuthError> {
    let values = value
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| AuthError::Unavailable("JWKS keys must be an array".into()))?;
    let mut keys = BTreeMap::new();
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
        let jwk: Jwk = serde_json::from_value(value.clone())
            .map_err(|error| AuthError::Unavailable(error.to_string()))?;
        let decoding = DecodingKey::from_jwk(&jwk)
            .map_err(|error| AuthError::Unavailable(error.to_string()))?;
        if keys
            .insert(
                kid.to_owned(),
                SigningKey {
                    algorithm,
                    decoding: Arc::new(decoding),
                },
            )
            .is_some()
        {
            return Err(AuthError::Unavailable("JWKS contains duplicate kid".into()));
        }
    }
    if keys.is_empty() {
        return Err(AuthError::Unavailable(
            "JWKS contains no supported signing keys".into(),
        ));
    }
    Ok(keys)
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
