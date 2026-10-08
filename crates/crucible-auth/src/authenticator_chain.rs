use crate::{AuthenticateResult, Authenticator, authentication_plan::AuthenticationPlan};
use axum::http::{HeaderValue, header};
use crucible_server_runtime_rust::{
    AsyncOperationAwareMiddleware, FrameworkError, OperationJob, Prepare, Result,
    operation::AuthScheme,
};
use std::{collections::HashMap, sync::Arc};

/// Resolves schemes and authenticators once during `prepare`, before serving requests.
pub struct AuthenticatorChain {
    authenticators: Vec<Arc<dyn Authenticator>>,
    plans: HashMap<&'static str, AuthenticationPlan>,
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
        for operation in prepare.operations() {
            let Some(metadata) = prepare.metadata(operation) else {
                continue;
            };
            let Some(schemes) = metadata.auth_schemes else {
                errors.push(format!(
                    "{}: authentication metadata missing; regenerate bindings",
                    operation.id
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
                    errors.push(format!("{}: no authenticator for {scheme:?}", operation.id));
                }
            }
            self.plans.insert(
                operation.id,
                AuthenticationPlan {
                    attempts,
                    optional: metadata.optional_auth || schemes.is_empty(),
                    bearer: schemes
                        .iter()
                        .any(|scheme| matches!(scheme, AuthScheme::Bearer)),
                },
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    async fn before(&self, job: &mut OperationJob<'_>) -> Result<()> {
        let plan = self.plans.get(job.operation().id).ok_or_else(|| {
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
                    return Err(
                        crate::authentication_unavailable::AuthenticationUnavailable(error).into(),
                    );
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
            job.response_headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(if refused {
                    "Bearer error=\"invalid_token\""
                } else {
                    "Bearer"
                }),
            );
        }
        job.context().set_attribute(FrameworkError::Unauthenticated);
        Err((self.refusal)(FrameworkError::Unauthenticated))
    }
}
