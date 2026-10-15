use crate::{Principal, ScopelessPolicy, bearer_challenge::bearer_challenge};
use crucible_server_runtime_rust::{DecodedJob, Error, FrameworkError, Hook, Interceptor, Result};
use std::sync::Arc;

/// Refuses calls whose [`Principal`] lacks a scope the model requires.
///
/// Opt-in: install it with `Crucible::builder(..).interceptor(..)` after an
/// authenticator chain. Each operation requires **all** of its
/// `@crucible.auth#scopes` (OpenAPI security-requirement semantics); a scope
/// matches literally against the space-separated `scope` claim (RFC 6749
/// section 3.3), with no wildcard or hierarchy. Operations that declare none are
/// untouched.
///
/// It runs after the protocol has decoded and validated the input, so a caller
/// without the scope may still learn of a malformed request before the 403.
pub struct ScopeEnforcer {
    scopeless: ScopelessPolicy,
    refusal: Arc<dyn Fn(FrameworkError) -> Error + Send + Sync>,
}

impl ScopeEnforcer {
    /// The policy for scope-less tokens is a required choice, not a default.
    pub fn new(scopeless: ScopelessPolicy) -> Self {
        Self {
            scopeless,
            refusal: Arc::new(Into::into),
        }
    }

    /// Every principal, ID tokens and API keys included, must hold the
    /// required scopes. Neither carries any, so both are refused (403) on
    /// every operation that declares scopes.
    pub fn strict() -> Self {
        Self::new(ScopelessPolicy::Refuse)
    }

    /// Exempt ID-token principals (web sessions); access tokens are checked.
    /// An API key carries no scopes, so under this policy an API key caller
    /// is refused (403) on every operation that declares scopes. To admit API
    /// keys there, exempt them too and authorize them another way:
    /// `ScopeEnforcer::new(ScopelessPolicy::Exempt(vec![TokenKind::Id, TokenKind::ApiKey]))`.
    pub fn exempt_id_tokens() -> Self {
        Self::new(ScopelessPolicy::Exempt(vec![crate::TokenKind::Id]))
    }

    /// Substitute a service's modeled refusal, as
    /// [`AuthenticatorChain::refusal_factory`](crate::AuthenticatorChain::refusal_factory)
    /// does. Receives `FrameworkError::Forbidden`, or `Unauthenticated` when
    /// the operation needs scopes but no chain ran.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(FrameworkError) -> Error + Send + Sync + 'static,
    ) -> Self {
        self.refusal = Arc::new(factory);
        self
    }

    fn refuse(&self, job: &mut DecodedJob<'_>, error: FrameworkError) -> Error {
        let metadata = *job.metadata();
        if metadata.bearer_auth {
            let challenge = match error {
                // RFC 6750 3.1: the 403 names the scopes the operation needs.
                FrameworkError::Forbidden => {
                    bearer_challenge(Some("insufficient_scope"), metadata.scopes)
                }
                _ => bearer_challenge(None, metadata.service_scopes),
            };
            job.response_headers_mut()
                .insert(axum::http::header::WWW_AUTHENTICATE, challenge);
        }
        job.context().set_attribute(error.clone());
        (self.refusal)(error)
    }
}

impl Interceptor for ScopeEnforcer {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        let metadata = *job.metadata();
        if metadata.scopes.is_empty() {
            return Hook::Ready(Ok(()));
        }
        let Some(principal) = job.context().attribute::<Principal>() else {
            // A scoped operation is authenticated, so a missing principal means
            // no authenticator ran: fail closed rather than serve it.
            return Hook::Ready(Err(self.refuse(job, FrameworkError::Unauthenticated)));
        };
        if self.scopeless.exempts(&principal.token_kind)
            || metadata
                .scopes
                .iter()
                .all(|scope| principal.scopes.contains(*scope))
        {
            return Hook::Ready(Ok(()));
        }
        Hook::Ready(Err(self.refuse(job, FrameworkError::Forbidden)))
    }
}
