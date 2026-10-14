use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::authorizer_unavailable::AuthorizerUnavailable;
use super::{
    AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Decision, Listing, Obligation,
    ObligationOutcome, OperationFacts, PermissionRefusal, ResourceRef, Verdict, Visibility,
};
use crate::{Context, Error, OperationJob, Outcome, Result};

pub(crate) type RefusalFactory = Arc<dyn Fn(&PermissionRefusal) -> Error + Send + Sync>;

pub(crate) const TARGET: &str = "crucible_server_runtime_rust::permissions";

/// One request's authorization, which [`Permissions`](super::Permissions)
/// leaves on the request context for the handler functions, the
/// `idempotency::commit` assertion and its own `after` hook.
#[derive(Clone)]
pub(crate) struct Session(Arc<State>);

struct State {
    authorizer: Arc<dyn Authorizer>,
    refusal: RefusalFactory,
    facts: OperationFacts,
    shadow: bool,
    /// Whether an open obligation fails the request (tests and debug
    /// builds) rather than only being logged.
    assert_obligations: bool,
    inner: Mutex<Inner>,
}

struct Inner {
    record: AuthorizationRecord,
    /// Whether the obligations were settled after the operation.
    settled: bool,
}

pub(crate) struct Settings {
    pub authorizer: Arc<dyn Authorizer>,
    pub refusal: RefusalFactory,
    pub shadow: bool,
    pub assert_obligations: bool,
}

impl Session {
    pub fn new(settings: &Settings, facts: OperationFacts) -> Self {
        Self(Arc::new(State {
            authorizer: settings.authorizer.clone(),
            refusal: settings.refusal.clone(),
            facts,
            shadow: settings.shadow,
            assert_obligations: settings.assert_obligations,
            inner: Mutex::new(Inner {
                record: AuthorizationRecord {
                    operation: facts.id,
                    requirements: facts.requirements,
                    decisions: Vec::new(),
                    listing: None,
                    pending: None,
                    undeclared: false,
                    obligations: Vec::new(),
                    shadow: settings.shadow,
                },
                settled: false,
            }),
        }))
    }

    /// The request's context and session, for a handler function.
    pub fn current() -> Result<(Arc<Context>, Self)> {
        let context = Context::try_current().ok_or_else(not_installed)?;
        let session = context.attribute::<Self>().ok_or_else(not_installed)?;
        Ok((context, session))
    }

    pub fn facts(&self) -> OperationFacts {
        self.0.facts
    }

    /// Whether an open obligation fails the request.
    fn enforces_obligations(&self) -> bool {
        self.0.assert_obligations && !self.0.shadow
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the record and leaves the new one on the context, for audit
    /// middleware that reads it after the operation.
    pub fn update<R>(
        &self,
        context: &Context,
        change: impl FnOnce(&mut AuthorizationRecord) -> R,
    ) -> R {
        let mut inner = self.lock();
        let result = change(&mut inner.record);
        context.set_attribute(inner.record.clone());
        result
    }

    /// The Authorizer's verdicts for `checks`, recorded. A Hidden verdict on
    /// a check that binds nothing of the request is Denied: there is
    /// nothing to hide.
    pub async fn decide(&self, context: &Context, checks: &[Check]) -> Result<Vec<Verdict>> {
        if let Some(check) = checks.iter().find(|check| !names_one_instance(check)) {
            return Err(Error::internal(format!(
                "{}: a check of {} on {} does not name one instance of {}",
                self.0.facts.id,
                check.permission.value,
                check.resource,
                check.permission.resource.name
            )));
        }
        if checks.is_empty() {
            return Ok(Vec::new());
        }
        let request = AuthorizationRequest::new(context, self.0.facts);
        let verdicts = self
            .0
            .authorizer
            .decide(&request, checks)
            .await
            .map_err(|error| Error::new(AuthorizerUnavailable(error)))?;
        if verdicts.len() != checks.len() {
            return Err(Error::internal(format!(
                "{}: the Authorizer answered {} verdicts for {} checks",
                self.0.facts.id,
                verdicts.len(),
                checks.len()
            )));
        }
        let verdicts: Vec<Verdict> = checks
            .iter()
            .zip(verdicts)
            .map(|(check, verdict)| self.something_to_hide(&check.resource, verdict))
            .collect();
        self.update(context, |record| {
            record
                .decisions
                .extend(
                    checks
                        .iter()
                        .zip(&verdicts)
                        .map(|(check, verdict)| Decision {
                            check: check.clone(),
                            verdict: *verdict,
                        }),
                );
        });
        Ok(verdicts)
    }

    /// `verdict`, unless it hides a resource the request binds nothing of.
    pub fn something_to_hide(&self, resource: &ResourceRef, verdict: Verdict) -> Verdict {
        if verdict == Verdict::Hidden && resource.binds_nothing() {
            tracing::warn!(
                target: TARGET,
                operation = self.0.facts.id,
                resource = %resource,
                "the Authorizer hid a resource the request binds nothing of; answering Denied, since a 404 would hide nothing"
            );
            return Verdict::Denied;
        }
        verdict
    }

    /// The error a verdict that is not Allowed is answered with: the
    /// framework's 404 for Hidden, the service's refusal for Denied.
    pub fn refuse(&self, context: &Context, check: &Check, verdict: Verdict) -> Error {
        match verdict {
            Verdict::Hidden => check.resource.not_found(),
            Verdict::Allowed | Verdict::Denied => {
                let refusal = PermissionRefusal::new(check.clone());
                context.set_attribute(refusal.clone());
                (self.0.refusal)(&refusal)
            }
        }
    }

    /// What `listing` may show, recorded.
    pub async fn visible(&self, context: &Context, listing: &Listing) -> Result<Visibility> {
        let request = AuthorizationRequest::new(context, self.0.facts);
        let visibility = self
            .0
            .authorizer
            .visible(&request, listing)
            .await
            .map_err(|error| Error::new(AuthorizerUnavailable(error)))?;
        self.update(context, |record| {
            record.listing = Some((listing.clone(), visibility.clone()));
        });
        Ok(visibility)
    }

    pub fn visibility(&self) -> Result<Visibility> {
        self.lock()
            .record
            .listing
            .as_ref()
            .map(|(_, visibility)| visibility.clone())
            .ok_or_else(|| {
                Error::internal(format!(
                    "{}: permissions::visible() needs a `filters` clause that was answered",
                    self.0.facts.id
                ))
            })
    }

    pub async fn discharge(
        &self,
        context: &Context,
        obligation: Obligation,
        resource: ResourceRef,
    ) -> Result<()> {
        self.known(&obligation)?;
        if resource.resource() != obligation.permission.resource {
            return Err(Error::internal(format!(
                "{}: obligation {} ({} on {}) was discharged against {resource}",
                self.0.facts.id,
                obligation.index,
                obligation.permission.value,
                obligation.permission.resource.name
            )));
        }
        let check = Check::obligation(&obligation, resource);
        let verdict = self.decide(context, std::slice::from_ref(&check)).await?[0];
        self.close(context, &obligation, ObligationOutcome::Discharged(verdict));
        if verdict == Verdict::Allowed {
            return Ok(());
        }
        // Normally the request has not committed, and failing releases the
        // claim. If it has (the mistake the commit-time assertion reports),
        // the commit is taken back so the claim is released anyway and the
        // caller can retry once it holds the permission.
        #[cfg(feature = "idempotency")]
        let _ = crate::idempotency::abandon();
        Err(self.refuse(context, &check, verdict))
    }

    pub fn not_applicable(
        &self,
        context: &Context,
        obligation: Obligation,
        reason: String,
    ) -> Result<()> {
        self.known(&obligation)?;
        self.close(
            context,
            &obligation,
            ObligationOutcome::NotApplicable(reason),
        );
        Ok(())
    }

    fn known(&self, obligation: &Obligation) -> Result<()> {
        let known = self
            .lock()
            .record
            .obligations
            .iter()
            .any(|record| record.obligation == *obligation);
        if known {
            return Ok(());
        }
        Err(Error::internal(format!(
            "{}: clause {} of {} ({} on {}) is not an obligation of this operation",
            self.0.facts.id,
            obligation.index,
            obligation.operation,
            obligation.permission.value,
            obligation.permission.resource.name
        )))
    }

    fn close(&self, context: &Context, obligation: &Obligation, outcome: ObligationOutcome) {
        self.update(context, |record| {
            for record in &mut record.obligations {
                if record.obligation == *obligation {
                    record.outcome = outcome.clone();
                }
            }
        });
    }

    /// The obligations still open, described for a log line.
    fn open(&self) -> Option<String> {
        let open: Vec<String> = self
            .lock()
            .record
            .obligations
            .iter()
            .filter(|record| record.outcome == ObligationOutcome::Open)
            .map(|record| {
                format!(
                    "{} on {} (clause {})",
                    record.obligation.permission.value,
                    record.obligation.permission.resource.name,
                    record.obligation.index
                )
            })
            .collect();
        (!open.is_empty()).then(|| open.join(", "))
    }

    /// Closes what a replay leaves open: the operation did not run, so its
    /// handler could not discharge anything, and the declared checks were
    /// made again.
    pub fn replayed(&self, context: &Context) {
        self.lock().settled = true;
        self.update(context, |record| {
            for record in &mut record.obligations {
                if record.outcome == ObligationOutcome::Open {
                    record.outcome = ObligationOutcome::NotApplicable(
                        "a replay: the operation did not run".to_owned(),
                    );
                }
            }
        });
    }
}

/// Whether `check` names one instance of its permission's resource.
fn names_one_instance(check: &Check) -> bool {
    check.resource.resource() == check.permission.resource && check.resource.is_complete()
}

fn not_installed() -> Error {
    Error::internal(
        "no authorization on the request: install Permissions, and call this from an operation it covers",
    )
}

/// `idempotency::commit()`'s check: the point of no return must not be
/// passed with an obligation open. An error in tests and debug builds, so
/// the step is not taken; a logged error in release builds.
#[cfg(feature = "idempotency")]
pub(crate) fn before_commit() -> Result<()> {
    let Some(session) = Context::try_current().and_then(|context| context.attribute::<Session>())
    else {
        return Ok(());
    };
    let Some(open) = session.open() else {
        return Ok(());
    };
    tracing::error!(
        target: TARGET,
        operation = session.0.facts.id,
        obligations = %open,
        "idempotency::commit() was called with a permission obligation open; discharge it, or say it does not apply, before the point of no return"
    );
    if session.enforces_obligations() {
        return Err(Error::internal(format!(
            "{}: idempotency::commit() with a permission obligation open: {open}",
            session.0.facts.id
        )));
    }
    Ok(())
}

/// After the operation: a success with an obligation still open is a
/// handler bug. Tests and debug builds fail the request; release builds
/// record it and log it, but keep the response, since whatever the handler
/// did has happened. Runs once, from whichever of `Permissions` and
/// `Idempotency` unwinds first, so a claim is never completed with a
/// success the assertion then turns into a failure.
pub(crate) fn settle(job: &mut OperationJob<'_>) {
    let context = job.context().clone();
    let Some(session) = context.attribute::<Session>() else {
        return;
    };
    {
        let mut inner = session.lock();
        if inner.settled {
            return;
        }
        inner.settled = true;
    }
    if !matches!(job.outcome(), Some(Outcome::Output(_))) {
        return;
    }
    let Some(open) = session.open() else {
        return;
    };
    session.update(&context, |record| {
        for record in &mut record.obligations {
            if record.outcome == ObligationOutcome::Open {
                record.outcome = ObligationOutcome::Undischarged;
            }
        }
    });
    tracing::error!(
        target: TARGET,
        operation = session.0.facts.id,
        obligations = %open,
        "the operation succeeded with a permission obligation open; its handler must discharge it, or say it does not apply"
    );
    if session.enforces_obligations() {
        job.fail(Error::internal(format!(
            "{}: succeeded with a permission obligation open: {open}",
            session.0.facts.id
        )));
    }
}

/// A replayed output ([`DecodedJob::replay_output`](crate::DecodedJob::replay_output))
/// settles the obligations: the handler does not run.
pub(crate) fn replayed(context: &Context) {
    if let Some(session) = context.attribute::<Session>() {
        session.replayed(context);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{HeaderValue, Request};

    use super::{Session, Settings};
    use crate::Context;
    use crate::permissions::{
        AuthorizationRequest, Authorizer, Check, Obligation, ObligationOutcome, ObligationRecord,
        OperationFacts, PermissionInfo, Requirements, ResourceInfo, ResourceRef, Tier, Verdict,
    };

    static WORKSPACE: ResourceInfo = ResourceInfo {
        name: "workspace",
        identifiers: &["workspaceId"],
    };
    static ADMIN: PermissionInfo = PermissionInfo {
        resource: &WORKSPACE,
        symbol: "admin",
        value: "workspace:admin",
        aliases: &[],
        tier: Tier::NotGrantable,
    };
    static REQUIREMENTS: Requirements = Requirements::None;

    struct Hiding;
    #[async_trait::async_trait]
    impl Authorizer for Hiding {
        async fn decide(
            &self,
            _: &AuthorizationRequest<'_>,
            checks: &[Check],
        ) -> crate::Result<Vec<Verdict>> {
            Ok(vec![Verdict::Hidden; checks.len()])
        }
    }

    fn session(assert_obligations: bool) -> Session {
        Session::new(
            &Settings {
                authorizer: Arc::new(Hiding),
                refusal: Arc::new(|refusal| refusal.default_error()),
                shadow: false,
                assert_obligations,
            },
            OperationFacts {
                id: "t#Op",
                readonly: false,
                idempotent: false,
                requirements: &REQUIREMENTS,
            },
        )
    }

    fn context() -> Arc<Context> {
        let (parts, ()) = Request::new(()).into_parts();
        Arc::new(Context::new(HeaderValue::from_static("r-1"), None, parts))
    }

    #[tokio::test]
    async fn hiding_a_resource_the_request_binds_nothing_of_is_denied() {
        let session = session(true);
        let context = context();
        let bound = Check::handler(&ADMIN, ResourceRef::new(&WORKSPACE, ["w-1"]));
        let unbound = Check::handler(&ADMIN, ResourceRef::all(&WORKSPACE, Vec::<String>::new()));
        let verdicts = session.decide(&context, &[bound, unbound]).await.unwrap();
        assert_eq!(verdicts, [Verdict::Hidden, Verdict::Denied]);
        let record = context
            .attribute::<crate::permissions::AuthorizationRecord>()
            .unwrap();
        assert_eq!(record.decisions.len(), 2);
    }

    #[tokio::test]
    async fn a_check_that_does_not_name_one_instance_is_a_wiring_error() {
        let session = session(true);
        let incomplete = Check::handler(&ADMIN, ResourceRef::new(&WORKSPACE, ["a", "b"]));
        assert!(session.decide(&context(), &[incomplete]).await.is_err());
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn commit_with_an_open_obligation_is_refused_when_asserting_and_logged_otherwise() {
        for assert_obligations in [true, false] {
            let session = session(assert_obligations);
            let context = context();
            context.set_attribute(session.clone());
            session.update(&context, |record| {
                record.obligations.push(ObligationRecord {
                    obligation: Obligation {
                        operation: "t#Op",
                        index: 0,
                        permission: &ADMIN,
                    },
                    outcome: ObligationOutcome::Open,
                });
            });
            let commit = context
                .clone()
                .scope(async { super::before_commit() })
                .await;
            assert_eq!(commit.is_err(), assert_obligations);
        }
    }
}
