use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(feature = "idempotency")]
use serde_json::{Value, json};

use super::authorizer_unavailable::AuthorizerUnavailable;
use super::{
    AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Decision, Listing, Obligation,
    ObligationOutcome, OperationFacts, PermissionInfo, PermissionRefusal, ResourceRef, Verdict,
    Visibility,
};
#[cfg(feature = "idempotency")]
use super::{Origin, Target};
use crate::{Context, Error, OperationJob, Outcome, Result};

pub(crate) type RefusalFactory = Arc<dyn Fn(&PermissionRefusal) -> Error + Send + Sync>;

pub(crate) type NotFoundFactory = Arc<dyn Fn(&ResourceRef) -> Error + Send + Sync>;

/// The service's own not-found error, when the model designates one: what
/// a hidden resource is answered with, built by the service.
#[derive(Clone)]
pub(crate) struct NotFound {
    /// The designated error's shape id.
    pub shape: &'static str,
    pub factory: NotFoundFactory,
    /// The service's: tells whether the factory built the designated error.
    pub classify: crate::operation::Classify,
}

/// Every permission a stored authorization may name: the service's
/// registry and every permission its operations require.
pub(crate) type Registry = Arc<Vec<&'static PermissionInfo>>;

pub(crate) const TARGET: &str = "crucible_server_runtime_rust::permissions";

/// One request's authorization, which [`Permissions`](super::Permissions)
/// leaves on the request context for the handler functions, the
/// `idempotency::commit` check, a replay's re-authorization and its own
/// `after` hook.
#[derive(Clone)]
pub(crate) struct Session(Arc<State>);

struct State {
    authorizer: Arc<dyn Authorizer>,
    refusal: RefusalFactory,
    not_found: Option<NotFound>,
    /// What a replay's stored checks are read back against.
    #[cfg_attr(not(feature = "idempotency"), allow(dead_code))]
    registry: Registry,
    facts: OperationFacts,
    shadow: bool,
    /// Whether an obligation left unsatisfied by a success fails the
    /// request (tests and debug builds) rather than only being logged.
    assert_obligations: bool,
    inner: Mutex<Inner>,
}

struct Inner {
    record: AuthorizationRecord,
    /// The handler-side checks the Authorizer allowed: what a replay of
    /// this request's success must be allowed again.
    handled: Vec<Check>,
    /// Whether the obligations were settled after the operation.
    settled: bool,
}

pub(crate) struct Settings {
    pub authorizer: Arc<dyn Authorizer>,
    pub refusal: RefusalFactory,
    pub not_found: Option<NotFound>,
    pub registry: Registry,
    pub shadow: bool,
    pub assert_obligations: bool,
}

impl Session {
    pub fn new(settings: &Settings, facts: OperationFacts) -> Self {
        Self(Arc::new(State {
            authorizer: settings.authorizer.clone(),
            refusal: settings.refusal.clone(),
            not_found: settings.not_found.clone(),
            registry: settings.registry.clone(),
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
                handled: Vec::new(),
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

    /// [`decide`](Self::decide) for checks a handler makes, remembering
    /// the allowed ones so a replay of this request's success asks for them
    /// again.
    pub async fn decide_for_handler(
        &self,
        context: &Context,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let verdicts = self.decide(context, checks).await?;
        self.lock().handled.extend(
            checks
                .iter()
                .zip(&verdicts)
                .filter(|(_, verdict)| **verdict == Verdict::Allowed)
                .map(|(check, _)| check.clone()),
        );
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
    /// not-found error for Hidden, the service's refusal for Denied.
    pub fn refuse(&self, context: &Context, check: &Check, verdict: Verdict) -> Error {
        match verdict {
            Verdict::Hidden => self.not_found(&check.resource),
            Verdict::Allowed | Verdict::Denied => {
                let refusal = PermissionRefusal::new(check.clone());
                context.set_attribute(refusal.clone());
                (self.0.refusal)(&refusal)
            }
        }
    }

    /// The 404 for `resource`, hidden or missing alike: the framework's
    /// NotFound, or the service's own error when the model designates one,
    /// built by its factory. A factory that builds anything else would make
    /// a hidden resource tell itself apart, so that is a wiring error,
    /// answered with the service's internal error.
    pub fn not_found(&self, resource: &ResourceRef) -> Error {
        let Some(not_found) = &self.0.not_found else {
            return resource.not_found();
        };
        let error = (not_found.factory)(resource);
        let built = (not_found.classify)(error.get_ref()).map(|modelled| modelled.shape_id());
        if built == Some(not_found.shape) {
            return error;
        }
        tracing::error!(
            target: TARGET,
            operation = self.0.facts.id,
            resource = %resource,
            built = built.unwrap_or("an unmodelled error"),
            expected = not_found.shape,
            "a wiring error: the not_found_factory built another error than the model's not-found error, so a hidden resource would read unlike a missing one"
        );
        Error::internal(format!(
            "{}: the not_found_factory built {}, not {}",
            self.0.facts.id,
            built.unwrap_or("an unmodelled error"),
            not_found.shape
        ))
    }

    /// Whether the service's own not-found error is built here.
    pub fn builds_not_found(&self) -> bool {
        self.0.not_found.is_some()
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
        self.still_open(&obligation)?;
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
        let verdict = self
            .decide_for_handler(context, std::slice::from_ref(&check))
            .await?[0];
        self.close(context, &obligation, ObligationOutcome::Discharged(verdict));
        if verdict == Verdict::Allowed {
            return Ok(());
        }
        // `idempotency::commit()` refuses while an obligation is open, so a
        // refusal here always comes before the point of no return: failing
        // releases the claim, and nothing is abandoned.
        Err(self.refuse(context, &check, verdict))
    }

    pub fn not_applicable(
        &self,
        context: &Context,
        obligation: Obligation,
        reason: String,
    ) -> Result<()> {
        self.still_open(&obligation)?;
        self.close(
            context,
            &obligation,
            ObligationOutcome::NotApplicable(reason),
        );
        Ok(())
    }

    /// An obligation is closed once: a second close (a `not_applicable`
    /// after a refused `discharge`, say) would overwrite what the record
    /// says happened.
    fn still_open(&self, obligation: &Obligation) -> Result<()> {
        let outcome = self
            .lock()
            .record
            .obligations
            .iter()
            .find(|record| record.obligation == *obligation)
            .map(|record| record.outcome.clone());
        let problem = match outcome {
            Some(ObligationOutcome::Open) => return Ok(()),
            Some(outcome) => format!("is already closed ({outcome:?})"),
            None => "is not an obligation of this operation".to_owned(),
        };
        Err(Error::internal(format!(
            "{}: clause {} of {} ({} on {}) {problem}",
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

    /// The obligations not satisfied, described for a log line: still open,
    /// or discharged with a refusal the handler went on past.
    fn unsatisfied(&self) -> Option<String> {
        let unsatisfied: Vec<String> = self
            .lock()
            .record
            .obligations
            .iter()
            .filter(|record| unsatisfied(&record.outcome))
            .map(|record| {
                format!(
                    "{} on {} (clause {}, {:?})",
                    record.obligation.permission.value,
                    record.obligation.permission.resource.name,
                    record.obligation.index,
                    record.outcome
                )
            })
            .collect();
        (!unsatisfied.is_empty()).then(|| unsatisfied.join(", "))
    }

    /// What a replay of this request's success must be allowed again, for
    /// the idempotency record.
    #[cfg(feature = "idempotency")]
    fn stored(&self) -> Value {
        Value::Array(
            self.lock()
                .handled
                .iter()
                .map(|check| {
                    json!({
                        "resource": check.permission.resource.name,
                        "permission": check.permission.value,
                        "identifiers": check
                            .resource
                            .identifiers()
                            .iter()
                            .map(|target| target.as_one().map_or(Value::Null, Value::from))
                            .collect::<Vec<_>>(),
                        "obligation": match check.origin {
                            Origin::Obligation(index) => Value::from(index),
                            _ => Value::Null,
                        },
                    })
                })
                .collect(),
        )
    }

    /// The checks a stored success recorded, read back against the
    /// registry; `None` when one no longer reads (a permission the model
    /// dropped), so it cannot be asked again.
    #[cfg(feature = "idempotency")]
    fn read_stored(&self, stored: &Value) -> Option<Vec<Check>> {
        stored
            .as_array()?
            .iter()
            .map(|item| {
                let resource = item.get("resource")?.as_str()?;
                let value = item.get("permission")?.as_str()?;
                let permission = self.0.registry.iter().copied().find(|permission| {
                    permission.resource.name == resource && permission.accepts(value)
                })?;
                let identifiers = item
                    .get("identifiers")?
                    .as_array()?
                    .iter()
                    .map(|target| match target {
                        Value::Null => Some(Target::All),
                        Value::String(id) => Some(Target::One(id.clone())),
                        _ => None,
                    })
                    .collect::<Option<Vec<Target>>>()?;
                let origin = match item.get("obligation") {
                    Some(Value::Number(index)) => {
                        Origin::Obligation(u16::try_from(index.as_u64()?).ok()?)
                    }
                    _ => Origin::Handler,
                };
                Some(Check {
                    permission,
                    resource: ResourceRef::new(permission.resource, identifiers),
                    origin,
                })
            })
            .collect()
    }
}

/// Whether an obligation in this state leaves a success unauthorized.
fn unsatisfied(outcome: &ObligationOutcome) -> bool {
    match outcome {
        ObligationOutcome::Open => true,
        ObligationOutcome::Discharged(verdict) => *verdict != Verdict::Allowed,
        _ => false,
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

/// `idempotency::commit()`'s check, in every build: the point of no return
/// must not be passed while an obligation is unsatisfied, so the step is not
/// taken. Shadow mode only logs it, since it refuses nothing.
#[cfg(feature = "idempotency")]
pub(crate) fn before_commit() -> Result<()> {
    let Some(session) = Context::try_current().and_then(|context| context.attribute::<Session>())
    else {
        return Ok(());
    };
    let Some(unsatisfied) = session.unsatisfied() else {
        return Ok(());
    };
    tracing::error!(
        target: TARGET,
        operation = session.0.facts.id,
        obligations = %unsatisfied,
        "idempotency::commit() was called with a permission obligation unsatisfied; discharge it, or say it does not apply, before the point of no return"
    );
    if session.0.shadow {
        return Ok(());
    }
    Err(Error::internal(format!(
        "{}: idempotency::commit() with a permission obligation unsatisfied: {unsatisfied}",
        session.0.facts.id
    )))
}

/// After the operation: a success with an obligation unsatisfied (open, or
/// discharged with a refusal the handler swallowed) is a handler bug. Tests
/// and debug builds fail the request; release builds record it and log it,
/// but keep the response, since whatever the handler did has happened. Runs
/// once, from whichever of `Permissions` and `Idempotency` unwinds first,
/// so a claim is never completed with a success the assertion then turns
/// into a failure.
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
    let Some(unsatisfied) = session.unsatisfied() else {
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
        obligations = %unsatisfied,
        "the operation succeeded with a permission obligation unsatisfied; its handler must discharge it (and stop when refused), or say it does not apply"
    );
    if session.0.assert_obligations && !session.0.shadow {
        job.fail(Error::internal(format!(
            "{}: succeeded with a permission obligation unsatisfied: {unsatisfied}",
            session.0.facts.id
        )));
    }
}

/// A replayed output ([`DecodedJob::replay_output`](crate::DecodedJob::replay_output))
/// settles the obligations: the handler does not run, and what it was
/// allowed was asked again ([`reauthorize`]).
pub(crate) fn replayed(context: &Context) {
    if let Some(session) = context.attribute::<Session>() {
        session.lock().settled = true;
        session.update(context, |record| {
            for record in &mut record.obligations {
                if record.outcome == ObligationOutcome::Open {
                    record.outcome = ObligationOutcome::Replayed;
                }
            }
        });
    }
}

/// The handler-side checks this request was allowed, for the idempotency
/// record of its success; `None` when `Permissions` does not cover it.
#[cfg(feature = "idempotency")]
pub(crate) fn stored_checks(context: &Context) -> Option<Value> {
    context
        .attribute::<Session>()
        .map(|session| session.stored())
}

/// Asks the Authorizer again for the handler-side checks a stored success
/// recorded, before the success is replayed: the first that is not Allowed
/// refuses the replay as it would have refused the request. `Ok(false)`
/// when a recorded check no longer reads, so the replay cannot be
/// authorized; `Ok(true)` when it may be served (also when `Permissions`
/// does not cover the operation now).
#[cfg(feature = "idempotency")]
pub(crate) async fn reauthorize(context: &Context, stored: &Value) -> Result<bool> {
    let Some(session) = context.attribute::<Session>() else {
        return Ok(true);
    };
    let Some(checks) = session.read_stored(stored) else {
        tracing::warn!(
            target: TARGET,
            operation = session.0.facts.id,
            "a stored success names a permission the service no longer declares, so its replay cannot be authorized"
        );
        return Ok(false);
    };
    let verdicts = session.decide(context, &checks).await?;
    if let Some((check, verdict)) = checks
        .iter()
        .zip(verdicts)
        .find(|(_, verdict)| *verdict != Verdict::Allowed)
    {
        if session.0.shadow {
            tracing::warn!(
                target: TARGET,
                operation = session.0.facts.id,
                permission = check.permission.value,
                resource = %check.resource,
                outcome = if verdict == Verdict::Hidden { "would_hide" } else { "would_deny" },
                "a replay would be refused"
            );
            return Ok(true);
        }
        return Err(session.refuse(context, check, verdict));
    }
    Ok(true)
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
        not_found: "No such workspace.",
    };
    static ADMIN: PermissionInfo = PermissionInfo {
        resource: &WORKSPACE,
        symbol: "admin",
        value: "workspace:admin",
        aliases: &[],
        tier: Tier::NotGrantable,
        denied: "This principal does not hold workspace:admin on that workspace.",
    };
    static REQUIREMENTS: Requirements = Requirements::None;
    const OBLIGATION: Obligation = Obligation {
        operation: "t#Op",
        index: 0,
        permission: &ADMIN,
    };

    struct Answering(Verdict);
    #[async_trait::async_trait]
    impl Authorizer for Answering {
        async fn decide(
            &self,
            _: &AuthorizationRequest<'_>,
            checks: &[Check],
        ) -> crate::Result<Vec<Verdict>> {
            Ok(vec![self.0; checks.len()])
        }
    }

    fn session(verdict: Verdict, shadow: bool) -> Session {
        Session::new(
            &Settings {
                authorizer: Arc::new(Answering(verdict)),
                refusal: Arc::new(|refusal| refusal.default_error()),
                not_found: None,
                registry: Arc::new(vec![&ADMIN]),
                shadow,
                assert_obligations: true,
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

    fn with_obligation(session: &Session, context: &Context) {
        context.set_attribute(session.clone());
        session.update(context, |record| {
            record.obligations.push(ObligationRecord {
                obligation: OBLIGATION,
                outcome: ObligationOutcome::Open,
            });
        });
    }

    #[tokio::test]
    async fn hiding_a_resource_the_request_binds_nothing_of_is_denied() {
        let session = session(Verdict::Hidden, false);
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
        let session = session(Verdict::Allowed, false);
        let incomplete = Check::handler(&ADMIN, ResourceRef::new(&WORKSPACE, ["a", "b"]));
        assert!(session.decide(&context(), &[incomplete]).await.is_err());
    }

    #[tokio::test]
    async fn an_obligation_is_closed_once() {
        let session = session(Verdict::Denied, false);
        let context = context();
        with_obligation(&session, &context);
        let workspace = ResourceRef::new(&WORKSPACE, ["w-1"]);
        assert!(
            session
                .discharge(&context, OBLIGATION, workspace.clone())
                .await
                .is_err()
        );
        // Neither a second discharge nor not_applicable rewrites the refusal.
        assert!(
            session
                .discharge(&context, OBLIGATION, workspace)
                .await
                .is_err()
        );
        assert!(
            session
                .not_applicable(&context, OBLIGATION, "never mind".to_owned())
                .is_err()
        );
        let record = context
            .attribute::<crate::permissions::AuthorizationRecord>()
            .unwrap();
        assert_eq!(
            record.obligations[0].outcome,
            ObligationOutcome::Discharged(Verdict::Denied)
        );
        assert!(session.unsatisfied().is_some());
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn allowed_handler_checks_are_stored_and_read_back() {
        let session = session(Verdict::Allowed, false);
        let context = context();
        with_obligation(&session, &context);
        session
            .discharge(&context, OBLIGATION, ResourceRef::new(&WORKSPACE, ["w-1"]))
            .await
            .unwrap();
        let stored = session.stored();
        assert_eq!(
            stored,
            serde_json::json!([{
                "resource": "workspace",
                "permission": "workspace:admin",
                "identifiers": ["w-1"],
                "obligation": 0,
            }])
        );
        let read = session.read_stored(&stored).unwrap();
        assert_eq!(
            read,
            [Check::obligation(
                &OBLIGATION,
                ResourceRef::new(&WORKSPACE, ["w-1"])
            )]
        );
        // A permission the registry no longer has cannot be asked again.
        let dropped = serde_json::json!([{
            "resource": "workspace", "permission": "workspace:gone",
            "identifiers": ["w-1"], "obligation": null,
        }]);
        assert!(session.read_stored(&dropped).is_none());
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn commit_with_an_obligation_open_is_refused_in_every_build_but_shadow() {
        for (shadow, refused) in [(false, true), (true, false)] {
            let session = session(Verdict::Allowed, shadow);
            let context = context();
            with_obligation(&session, &context);
            let commit = context
                .clone()
                .scope(async { super::before_commit() })
                .await;
            assert_eq!(commit.is_err(), refused, "shadow {shadow}");
        }
    }
}
