use crate::{
    AuthenticateResult, Authenticator, authentication_plan::AuthenticationPlan,
    bearer_challenge::bearer_challenge,
};
use axum::http::header;
use crucible_server_runtime_rust::{
    AsyncOperationAwareMiddleware, FrameworkError, OpaqueError, OperationJob, Prepare, Result,
    operation::{AuthScheme, OperationInfo},
};
use std::{collections::HashMap, sync::Arc};

/// Resolves schemes and authenticators once during `prepare`, before serving requests.
pub struct AuthenticatorChain {
    authenticators: Vec<Arc<dyn Authenticator>>,
    /// Plans by operation id, then by the description a request carries:
    /// one per API version that serves the operation, usually one.
    plans: HashMap<&'static str, Vec<(&'static OperationInfo, AuthenticationPlan)>>,
    refusal: Arc<dyn Fn(FrameworkError) -> crucible_server_runtime_rust::Error + Send + Sync>,
}

impl AuthenticatorChain {
    pub fn new(authenticators: Vec<Arc<dyn Authenticator>>) -> Self {
        Self {
            authenticators,
            plans: HashMap::new(),
            refusal: Arc::new(Into::into),
        }
    }

    /// Substitute a service's modeled refusal while retaining a typed marker on Context.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(FrameworkError) -> crucible_server_runtime_rust::Error + Send + Sync + 'static,
    ) -> Self {
        self.refusal = Arc::new(factory);
        self
    }
}

#[crucible_server_runtime_rust::async_trait]
impl AsyncOperationAwareMiddleware for AuthenticatorChain {
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        self.plans.clear();
        let mut errors = Vec::new();
        // One plan per served entry, not per operation id: an older API
        // version's entry carries its own release's schemes, and a request
        // at that version is authenticated by them.
        for served in prepare.served_operations() {
            let operation = served.operation();
            let metadata = served.metadata();
            let label = match served.api_version() {
                Some(version) => format!("{} (API version {version})", operation.id),
                None => operation.id.to_owned(),
            };
            let Some(schemes) = metadata.auth_schemes else {
                errors.push(format!(
                    "{label}: authentication metadata missing; regenerate bindings"
                ));
                continue;
            };
            let mut attempts = Vec::new();
            for scheme in schemes {
                let before = attempts.len();
                for (index, authenticator) in self.authenticators.iter().enumerate() {
                    if authenticator.supports(scheme) {
                        attempts.push((index, scheme));
                    }
                }
                if before == attempts.len() {
                    errors.push(format!("{label}: no authenticator for {scheme:?}"));
                }
            }
            self.plans.entry(operation.id).or_default().push((
                operation,
                AuthenticationPlan {
                    attempts,
                    optional: schemes.is_empty(),
                    bearer: schemes
                        .iter()
                        .any(|scheme| matches!(scheme, AuthScheme::Bearer)),
                },
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    async fn before(&self, job: &mut OperationJob<'_>) -> Result<()> {
        let operation = job.operation();
        let plan = self
            .plans
            .get(operation.id)
            .and_then(|plans| {
                plans
                    .iter()
                    .find(|(description, _)| std::ptr::eq(*description, operation))
            })
            .map(|(_, plan)| plan)
            .ok_or_else(|| {
                crucible_server_runtime_rust::Error::internal("authentication plan missing")
            })?;
        let mut refused = false;
        for (index, scheme) in &plan.attempts {
            if !self.authenticators[*index].has_credentials(job.request(), scheme) {
                continue;
            }
            match self.authenticators[*index]
                .authenticate(job.request(), scheme)
                .await
            {
                AuthenticateResult::Refused => refused = true,
                AuthenticateResult::Unavailable(error) => {
                    // Opaque: a service rule for its own store client's errors
                    // must not turn a provider outage into a modelled 4xx.
                    return Err(OpaqueError::new(
                        crate::authentication_unavailable::AuthenticationUnavailable(error),
                    )
                    .into());
                }
                AuthenticateResult::Authenticated(principal) => {
                    job.context().set_attribute(principal);
                    return Ok(());
                }
            }
        }
        if plan.optional && !refused {
            return Ok(());
        }
        if plan.bearer {
            // RFC 6750 3: a bare challenge; the model declares no scopes.
            let challenge = bearer_challenge(refused.then_some("invalid_token"), &[]);
            job.response_headers_mut()
                .insert(header::WWW_AUTHENTICATE, challenge);
        }
        job.context().set_attribute(FrameworkError::Unauthenticated);
        Err((self.refusal)(FrameworkError::Unauthenticated))
    }
}
