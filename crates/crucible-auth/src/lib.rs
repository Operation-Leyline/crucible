//! Explicitly installed authentication. Merely depending on this crate changes no service policy.
mod auth_error;
mod authenticate_result;
mod authentication_plan;
mod authentication_unavailable;
mod authenticator;
mod authenticator_chain;
mod client_claim;
mod issuer_config;
mod jwks_cache;
mod jwt_authenticator;
mod principal;
mod signing_key;
mod token_kind;
mod token_rule;

pub use auth_error::AuthError;
pub use authenticate_result::AuthenticateResult;
pub use authenticator::Authenticator;
pub use authenticator_chain::AuthenticatorChain;
pub use client_claim::ClientClaim;
pub use issuer_config::IssuerConfig;
pub use jsonwebtoken::Algorithm;
pub use jwt_authenticator::JwtAuthenticator;
pub use principal::Principal;
pub use token_kind::TokenKind;
pub use token_rule::TokenRule;

#[cfg(feature = "test-support")]
pub mod test_support;
