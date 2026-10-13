//! Real signing keys and an in-memory API-key store for service tests. Never enabled by default.
mod in_memory_api_key_store;

pub use in_memory_api_key_store::InMemoryApiKeyStore;

use crate::{Algorithm, AuthError, IssuerConfig};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{EncodingKey, Header, encode};
use rsa::{
    RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, pkcs8::EncodePrivateKey, traits::PublicKeyParts,
};
use serde_json::{Value, json};

/// Generates a fresh private key; publishes only its public JWK. Tokens take arbitrary claims.
pub struct TestTokenIssuer {
    pub issuer: String,
    pub kid: String,
    algorithm: Algorithm,
    encoding: EncodingKey,
    public: Value,
}

impl TestTokenIssuer {
    pub fn new(
        issuer: impl Into<String>,
        kid: impl Into<String>,
    ) -> std::result::Result<Self, AuthError> {
        let key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048)
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let der = key
            .to_pkcs1_der()
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let kid = kid.into();
        let public = json!({"kty":"RSA", "alg":"RS256", "use":"sig", "kid":kid,
            "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()), "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())});
        Ok(Self {
            issuer: issuer.into(),
            kid,
            algorithm: Algorithm::RS256,
            encoding: EncodingKey::from_rsa_der(der.as_bytes()),
            public,
        })
    }

    pub fn es256(
        issuer: impl Into<String>,
        kid: impl Into<String>,
    ) -> std::result::Result<Self, AuthError> {
        let key = p256::SecretKey::random(&mut rand::rngs::OsRng);
        let der = key
            .to_pkcs8_der()
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let point = key.public_key().to_sec1_bytes();
        let kid = kid.into();
        let public = json!({"kty":"EC", "crv":"P-256", "alg":"ES256", "use":"sig", "kid":kid,
            "x":URL_SAFE_NO_PAD.encode(&point[1..33]), "y":URL_SAFE_NO_PAD.encode(&point[33..65])});
        Ok(Self {
            issuer: issuer.into(),
            kid,
            algorithm: Algorithm::ES256,
            encoding: EncodingKey::from_ec_der(der.as_bytes()),
            public,
        })
    }

    pub fn jwks(&self) -> Value {
        json!({"keys":[self.public]})
    }

    pub fn mint(&self, claims: &Value) -> std::result::Result<String, AuthError> {
        let mut header = Header::new(self.algorithm);
        header.kid = Some(self.kid.clone());
        encode(&header, claims, &self.encoding)
            .map_err(|error| AuthError::Configuration(error.to_string()))
    }

    pub fn id_claims(&self, subject: &str, client: &str) -> Value {
        json!({"iss":self.issuer, "sub":subject, "aud":client, "token_use":"id", "exp": now() + 3600})
    }

    pub fn access_claims(&self, subject: &str, client: &str) -> Value {
        json!({"iss":self.issuer, "sub":subject, "client_id":client, "token_use":"access", "exp": now() + 3600})
    }

    /// Cognito claim policy with this test issuer's URL; inject `jwks()` or serve it over HTTP.
    pub fn config(&self, clients: Vec<String>) -> IssuerConfig {
        let mut config = IssuerConfig::cognito("test", "test_pool", clients);
        config.issuer = self.issuer.clone();
        config.jwks_url = format!("{}/jwks", self.issuer);
        config.allow_insecure_http = true;
        config.algorithms = vec![self.algorithm];
        config
    }

    pub fn access_config(&self, clients: Vec<String>) -> IssuerConfig {
        let mut config = self.config(clients);
        config.token_rules[0].kind = crate::TokenKind::Access;
        config.token_rules[0].client_claim = crate::ClientClaim::ClientId;
        config.token_rules[0].required_claims[0].1 = "access".into();
        config
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}
