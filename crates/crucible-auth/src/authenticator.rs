use crate::AuthenticateResult;
use axum::http::request::Parts;
use crucible_server_runtime_rust::operation::AuthScheme;

#[async_trait::async_trait]
pub trait Authenticator: Send + Sync + 'static {
    /// Called once per modeled scheme at service construction.
    fn supports(&self, scheme: &AuthScheme) -> bool;
    /// Whether this authenticator's credential source is present. Absence permits optional auth.
    fn has_credentials(&self, request: &Parts, scheme: &AuthScheme) -> bool;
    /// The selected static scheme includes API-key header/query placement and prefix.
    async fn authenticate(&self, request: &Parts, scheme: &AuthScheme) -> AuthenticateResult;
}
