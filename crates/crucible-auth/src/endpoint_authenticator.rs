use axum::http::{StatusCode, header, request::Parts};
use axum::response::IntoResponse;
use crucible_server_runtime_rust::Hook;
use crucible_server_runtime_rust::endpoint::{EndpointAdmission, EndpointGuard};
use crucible_server_runtime_rust::operation::AuthScheme;

use crate::{AuthenticateResult, Authenticator, bearer_challenge::bearer_challenge};

/// Require one explicit authentication scheme for every endpoint resource,
/// including discovery and assets which have no modeled operation.
pub struct EndpointAuthenticator<A> {
    authenticator: A,
    scheme: AuthScheme,
}

impl<A: Authenticator> EndpointAuthenticator<A> {
    pub fn new(authenticator: A, scheme: AuthScheme) -> std::result::Result<Self, String> {
        if !authenticator.supports(&scheme) {
            return Err(format!(
                "endpoint authenticator does not support {scheme:?}"
            ));
        }
        Ok(Self {
            authenticator,
            scheme,
        })
    }
}

impl<A: Authenticator> EndpointGuard for EndpointAuthenticator<A> {
    fn admit<'a>(&'a self, request: &'a Parts) -> Hook<'a, EndpointAdmission> {
        Hook::wait(async move {
            let present = self.authenticator.has_credentials(request, &self.scheme);
            let result = if present {
                self.authenticator.authenticate(request, &self.scheme).await
            } else {
                AuthenticateResult::Refused
            };
            match result {
                AuthenticateResult::Authenticated(_) => EndpointAdmission::Admit,
                AuthenticateResult::Unavailable(_) => {
                    EndpointAdmission::Respond(StatusCode::SERVICE_UNAVAILABLE.into_response())
                }
                AuthenticateResult::Refused => {
                    let mut response = StatusCode::UNAUTHORIZED.into_response();
                    if matches!(self.scheme, AuthScheme::Bearer) {
                        response.headers_mut().insert(
                            header::WWW_AUTHENTICATE,
                            bearer_challenge(present.then_some("invalid_token"), &[]),
                        );
                    }
                    EndpointAdmission::Respond(response)
                }
            }
        })
    }
}
