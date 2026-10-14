use crate::{Algorithm, ClientClaim, TokenKind, TokenRule};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct IssuerConfig {
    pub issuer: String,
    pub jwks_url: String,
    /// Only RS256 and ES256 are supported. HMAC keys cannot enter a public JWKS trust store.
    pub algorithms: Vec<Algorithm>,
    pub token_rules: Vec<TokenRule>,
    pub leeway: u64,
    pub cache_ttl: Duration,
    /// No fetch for this long after a failed one, nor for a `kid` that a successful fetch
    /// did not contain, preventing request-driven fetch storms. A `kid` not looked up yet
    /// may fetch one second after the last fetch (or after this, if shorter), so a token
    /// with a forged `kid` cannot hold a newly rotated key off for the whole interval,
    /// unless that key's `kid` was looked up before the provider published it: it then
    /// waits this interval too. Forged key IDs can cause about one fetch a second, which
    /// raises the chance of provider throttling; a failed fetch holds all fetches off.
    pub min_refresh_interval: Duration,
    /// Bounded outage fallback beyond cache_ttl. Signature, expiry and claims remain validated.
    pub max_stale: Duration,
    #[cfg(feature = "test-support")]
    pub allow_insecure_http: bool,
}

impl IssuerConfig {
    pub fn new(
        issuer: impl Into<String>,
        jwks_url: impl Into<String>,
        clients: Vec<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            jwks_url: jwks_url.into(),
            algorithms: vec![Algorithm::RS256, Algorithm::ES256],
            token_rules: vec![TokenRule::audience(clients)],
            leeway: 30,
            cache_ttl: Duration::from_secs(3600),
            min_refresh_interval: Duration::from_secs(30),
            max_stale: Duration::from_secs(24 * 3600),
            #[cfg(feature = "test-support")]
            allow_insecure_http: false,
        }
    }

    /// ID-only policy. Create a separate `cognito_access` instance for access-token operations.
    pub fn cognito(region: &str, pool_id: &str, clients: Vec<String>) -> Self {
        let issuer = format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}");
        let mut config = Self::new(
            &issuer,
            format!("{issuer}/.well-known/jwks.json"),
            clients.clone(),
        );
        config.algorithms = vec![Algorithm::RS256];
        config.token_rules = vec![TokenRule {
            kind: TokenKind::Id,
            client_claim: ClientClaim::Audience,
            allowed_clients: clients,
            required_claims: vec![("token_use".into(), "id".into())],
            header_type: None,
        }];
        config
    }

    pub fn cognito_access(region: &str, pool_id: &str, clients: Vec<String>) -> Self {
        let mut config = Self::cognito(region, pool_id, clients);
        config.token_rules[0].kind = TokenKind::Access;
        config.token_rules[0].client_claim = ClientClaim::ClientId;
        config.token_rules[0].required_claims[0].1 = "access".into();
        config
    }
}
