use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::RoleBinding;
use crate::Result;
use crate::permissions::{
    AuthorizationRequest, Authorizer, Check, Listing, Verdict, Visibility, loggable,
};

const TARGET: &str = "crucible_server_runtime_rust::roles::shadow";

/// An [`Authorizer`] that enforces `Primary`'s answers and runs
/// `Candidate` beside it, logging and counting every check the two answer
/// differently (docs/roles.md §7.3): how a service moves from its own
/// Authorizer to `RoleAuthorizer`, switching only once production runs
/// clean.
///
/// Both are asked concurrently. The primary's answer, or its error, is
/// what the request gets; a candidate error is logged and counted, never
/// returned. Each disagreement is a `warn` line under the target
/// `crucible_server_runtime_rust::roles::shadow` naming the operation, the
/// permission, the (escaped, shortened) resource and both verdicts;
/// [`ShadowCounts`] keeps the totals for a metrics exporter.
pub struct ShadowAuthorizer<P, C> {
    primary: P,
    candidate: C,
    counts: Arc<ShadowCounts>,
}

/// What a [`ShadowAuthorizer`] has compared so far.
#[derive(Debug, Default)]
pub struct ShadowCounts {
    compared: AtomicU64,
    disagreements: AtomicU64,
    candidate_failures: AtomicU64,
}

impl ShadowCounts {
    /// Checks (and listings) both answered.
    pub fn compared(&self) -> u64 {
        self.compared.load(Ordering::Relaxed)
    }

    /// Of those, the ones answered differently.
    pub fn disagreements(&self) -> u64 {
        self.disagreements.load(Ordering::Relaxed)
    }

    /// Batches the candidate could not answer (an error, or a verdict
    /// count that is not the checks').
    pub fn candidate_failures(&self) -> u64 {
        self.candidate_failures.load(Ordering::Relaxed)
    }
}

impl<P: Authorizer, C: Authorizer> ShadowAuthorizer<P, C> {
    pub fn new(primary: P, candidate: C) -> Self {
        Self {
            primary,
            candidate,
            counts: Arc::default(),
        }
    }

    /// The running totals, shared: take them before installing the
    /// authorizer in `Permissions`.
    pub fn counts(&self) -> Arc<ShadowCounts> {
        self.counts.clone()
    }

    fn compare(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
        primary: &[Verdict],
        candidate: Result<Vec<Verdict>>,
    ) {
        let operation = request.operation.id;
        let candidate = match candidate {
            Ok(verdicts) if verdicts.len() == checks.len() => verdicts,
            Ok(verdicts) => {
                self.counts
                    .candidate_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    target: TARGET,
                    operation,
                    checks = checks.len(),
                    verdicts = verdicts.len(),
                    "the candidate Authorizer answered a different number of verdicts"
                );
                return;
            }
            Err(error) => {
                self.counts
                    .candidate_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    target: TARGET,
                    operation,
                    error = %error,
                    "the candidate Authorizer could not decide"
                );
                return;
            }
        };
        for ((check, primary), candidate) in checks.iter().zip(primary).zip(candidate) {
            self.counts.compared.fetch_add(1, Ordering::Relaxed);
            if *primary == candidate {
                continue;
            }
            self.counts.disagreements.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                target: TARGET,
                operation,
                permission = check.permission.value,
                resource_type = check.permission.resource.name,
                resource = %loggable(&check.resource),
                primary = ?primary,
                candidate = ?candidate,
                "the Authorizers disagree"
            );
        }
    }
}

#[async_trait::async_trait]
impl<P: Authorizer, C: Authorizer> Authorizer for ShadowAuthorizer<P, C> {
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let (primary, candidate) = futures_util::future::join(
            self.primary.decide(request, checks),
            self.candidate.decide(request, checks),
        )
        .await;
        let primary = primary?;
        self.compare(request, checks, &primary, candidate);
        Ok(primary)
    }

    async fn decide_naming_bindings(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
        allowed_by: &mut Vec<Option<RoleBinding>>,
    ) -> Result<Vec<Verdict>> {
        let (primary, candidate) = futures_util::future::join(
            self.primary
                .decide_naming_bindings(request, checks, allowed_by),
            self.candidate.decide(request, checks),
        )
        .await;
        let primary = primary?;
        self.compare(request, checks, &primary, candidate);
        Ok(primary)
    }

    async fn visible(
        &self,
        request: &AuthorizationRequest<'_>,
        listing: &Listing,
    ) -> Result<Visibility> {
        let (primary, candidate) = futures_util::future::join(
            self.primary.visible(request, listing),
            self.candidate.visible(request, listing),
        )
        .await;
        let primary = primary?;
        match candidate {
            Ok(candidate) => {
                self.counts.compared.fetch_add(1, Ordering::Relaxed);
                if candidate != primary {
                    self.counts.disagreements.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        target: TARGET,
                        operation = request.operation.id,
                        permission = listing.permission.value,
                        resource = %loggable(&listing.resource),
                        primary = ?primary,
                        candidate = ?candidate,
                        "the Authorizers disagree on a listing"
                    );
                }
            }
            Err(error) => {
                self.counts
                    .candidate_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    target: TARGET,
                    operation = request.operation.id,
                    error = %error,
                    "the candidate Authorizer could not answer a listing"
                );
            }
        }
        Ok(primary)
    }
}
