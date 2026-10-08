use serde_json::{Value, json};

/// An RFC 8414 authorization server metadata document the service serves for
/// an identity provider whose own discovery document MCP clients reject.
///
/// Cognito, for one, publishes no `code_challenge_methods_supported`, and MCP
/// clients (Claude Code, claude.ai) refuse an authorization server that does
/// not advertise `S256`. Protected resource metadata names the service as the
/// authorization server; this document points at the provider's real
/// endpoints. Tokens are still issued and verified by the provider.
///
/// No `registration_endpoint` is advertised: Crucible does not implement
/// dynamic client registration, so clients use a pre-registered client id.
#[derive(Clone, Debug)]
pub struct AuthorizationServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: Option<String>,
    scopes_supported: Vec<String>,
}

impl AuthorizationServerMetadata {
    /// `issuer` is the public URL clients use for the service's authorization
    /// server identity, for example `https://api.example.com`. The document is
    /// served at its RFC 8414 well-known URL: `/.well-known/oauth-authorization-server`
    /// followed by the issuer's path, if any.
    pub fn new(
        issuer: impl Into<String>,
        authorization_endpoint: impl Into<String>,
        token_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            authorization_endpoint: authorization_endpoint.into(),
            token_endpoint: token_endpoint.into(),
            revocation_endpoint: None,
            scopes_supported: Vec::new(),
        }
    }

    /// Endpoints of a Cognito user pool domain, such as `auth.example.com` or
    /// `PREFIX.auth.us-east-1.amazoncognito.com` (host name only).
    pub fn cognito(issuer: impl Into<String>, user_pool_domain: &str) -> Self {
        let domain = user_pool_domain.trim_end_matches('/');
        let mut metadata = Self::new(
            issuer,
            format!("https://{domain}/oauth2/authorize"),
            format!("https://{domain}/oauth2/token"),
        );
        metadata.revocation_endpoint = Some(format!("https://{domain}/oauth2/revoke"));
        metadata
    }

    pub fn revocation_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.revocation_endpoint = Some(endpoint.into());
        self
    }

    pub fn scopes_supported(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes_supported = scopes.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn issuer(&self) -> &str {
        &self.issuer
    }

    /// The RFC 8414 well-known path for this issuer (path insertion, section 3.1).
    pub(crate) fn well_known_path(&self) -> Result<String, String> {
        for (name, url) in [
            ("issuer", Some(&self.issuer)),
            ("authorization_endpoint", Some(&self.authorization_endpoint)),
            ("token_endpoint", Some(&self.token_endpoint)),
            ("revocation_endpoint", self.revocation_endpoint.as_ref()),
        ] {
            let Some(url) = url else { continue };
            let host = url
                .strip_prefix("https://")
                .and_then(|r| r.split('/').next());
            if host.is_none_or(|h| h.is_empty() || h.contains(['?', '#'])) {
                return Err(format!(
                    "authorization server metadata {name} must be an https URL"
                ));
            }
        }
        // RFC 8414 section 2: an issuer carries no query or fragment.
        if self.issuer.contains(['?', '#']) {
            return Err(
                "authorization server metadata issuer must have no query or fragment".into(),
            );
        }
        let rest = &self.issuer["https://".len()..];
        let path = rest
            .find('/')
            .map_or("", |i| &rest[i..])
            .trim_end_matches('/');
        Ok(format!("/.well-known/oauth-authorization-server{path}"))
    }

    /// Serialized once at service construction; serving it does no per-request work.
    pub(crate) fn document(&self) -> String {
        let mut document = json!({
            "issuer": self.issuer,
            "authorization_endpoint": self.authorization_endpoint,
            "token_endpoint": self.token_endpoint,
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported":
                ["none", "client_secret_post", "client_secret_basic"],
        });
        let fields = document.as_object_mut().expect("object literal");
        if let Some(endpoint) = &self.revocation_endpoint {
            fields.insert("revocation_endpoint".into(), Value::from(endpoint.as_str()));
        }
        if !self.scopes_supported.is_empty() {
            fields.insert("scopes_supported".into(), json!(self.scopes_supported));
        }
        document.to_string()
    }
}
