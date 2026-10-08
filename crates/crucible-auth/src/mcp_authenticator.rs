use axum::http::{StatusCode, header, request::Parts};
use axum::response::IntoResponse;
use crucible_server_runtime_rust::mcp::{EndpointAdmission, EndpointGuard, McpAuthorization};
use crucible_server_runtime_rust::{Context, Hook, operation::AuthScheme};

use crate::{AuthError, AuthenticateResult, Authenticator, JwtAuthenticator, TokenKind};

/// Access-token admission confined to the MCP endpoint. Does not install API middleware.
pub struct McpAuthenticator {
    resource: String,
    authenticators: Vec<JwtAuthenticator>,
    required_scope: String,
    allow_unbound: bool,
}

impl McpAuthenticator {
    /// Requires access-token verifiers, an MCP resource URL and a service-owned scope.
    /// Audience binding is required unless explicitly relaxed for Cognito compatibility.
    pub fn new(
        resource: impl Into<String>,
        authenticators: Vec<JwtAuthenticator>,
        required_scope: impl Into<String>,
    ) -> Result<Self, AuthError> {
        let resource = resource.into();
        let required_scope = required_scope.into();
        if authenticators.is_empty() || authenticators.iter().any(|a| !a.mcp_access_policy()) {
            return Err(AuthError::Configuration(
                "MCP requires Authorization-header access-token authenticators".into(),
            ));
        }
        // A built-in scope such as openid cannot distinguish this service from another.
        if !required_scope
            .split_once('/')
            .is_some_and(|(server, scope)| !server.is_empty() && !scope.is_empty())
            || !required_scope
                .bytes()
                .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
        {
            return Err(AuthError::Configuration(
                "MCP requires a service-owned resource-server scope (identifier/scope)".into(),
            ));
        }
        Ok(Self {
            resource,
            authenticators,
            required_scope,
            allow_unbound: false,
        })
    }

    /// Permit tokens without `aud` using client_id allowlists and the service-owned scope.
    /// Present audiences must still equal the MCP resource. This is not RFC 8707 binding.
    pub fn allow_unbound_cognito_tokens(mut self) -> Self {
        self.allow_unbound = true;
        self
    }
}

impl EndpointGuard for McpAuthenticator {
    fn admit<'a>(&'a self, request: &'a Parts) -> Hook<'a, EndpointAdmission> {
        Hook::wait(async move {
            for authenticator in &self.authenticators {
                match authenticator
                    .authenticate(request, &AuthScheme::Bearer)
                    .await
                {
                    AuthenticateResult::Refused => continue,
                    AuthenticateResult::Unavailable(_) => {
                        return EndpointAdmission::Respond(
                            StatusCode::SERVICE_UNAVAILABLE.into_response(),
                        );
                    }
                    AuthenticateResult::Authenticated(principal) => {
                        let audience_matches = match principal.claims.get("aud") {
                            None => self.allow_unbound,
                            Some(audience) => audience.as_str() == Some(self.resource.as_str()),
                        };
                        if principal.token_kind != TokenKind::Access || !audience_matches {
                            continue;
                        }
                        if !principal.scopes.contains(&self.required_scope) {
                            return EndpointAdmission::Respond(
                                (
                                    StatusCode::FORBIDDEN,
                                    [(
                                        header::WWW_AUTHENTICATE,
                                        crate::bearer_challenge::bearer_challenge(
                                            Some("insufficient_scope"),
                                            &[&self.required_scope],
                                        ),
                                    )],
                                )
                                    .into_response(),
                            );
                        }
                        Context::current().set_attribute(principal);
                        return EndpointAdmission::Admit;
                    }
                }
            }
            let error = request
                .headers
                .contains_key(header::AUTHORIZATION)
                .then_some("invalid_token");
            EndpointAdmission::Respond(
                (
                    StatusCode::UNAUTHORIZED,
                    [(
                        header::WWW_AUTHENTICATE,
                        crate::bearer_challenge::bearer_challenge(error, &[&self.required_scope]),
                    )],
                )
                    .into_response(),
            )
        })
    }
}

impl McpAuthorization for McpAuthenticator {
    fn resource(&self) -> &str {
        &self.resource
    }
    fn authorization_servers(&self) -> Vec<&str> {
        self.authenticators
            .iter()
            .map(JwtAuthenticator::issuer)
            .collect()
    }
    fn required_scopes(&self) -> Vec<&str> {
        vec![&self.required_scope]
    }
}
