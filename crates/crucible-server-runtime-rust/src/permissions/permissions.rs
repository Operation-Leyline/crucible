use std::sync::Arc;

use super::session::{RefusalFactory, Session, Settings, TARGET};
use super::{
    Authorizer, Check, ErasedChecks, Listing, Mode, Obligation, ObligationOutcome,
    ObligationRecord, OperationFacts, Origin, PermissionRefusal, RequirementInfo, Requirements,
    Target, Verdict, Visibility,
};
use crate::operation::{Input, OperationInfo};
use crate::{
    Context, DecodedJob, Error, Hook, Interceptor, OperationJob, OperationMetadata, Prepare, Result,
};

/// Enforces the model's requirements (`Operation::REQUIREMENTS`) through
/// the service's [`Authorizer`], after the input is decoded and before the
/// operation runs (see the [module docs](super)).
///
/// For each operation it covers:
///
/// - **Undeclared** (no requirement metadata, which the compiler allows
///   only before a service declares any permission): refused with the
///   service's internal error and logged as a wiring error, unless the
///   operation is public (it has no authentication) or
///   [`allow_undeclared`](Self::allow_undeclared) opts out.
/// - **`requires none`** passes; **`requires pending`** passes with its
///   reason recorded, since the handler still does the checking.
/// - **Requirements**: the operation's `CHECKS` bind each applicable clause
///   to the input, the Authorizer answers them in one batch, and the first
///   verdict in declaration order that is not Allowed refuses the request.
///   Hidden is always the framework's 404; Denied is a 403, or what
///   [`refusal_factory`](Self::refusal_factory) builds. A `filters` clause
///   asks [`Authorizer::visible`] for [`visible`](super::visible), and each
///   `by handler` clause opens an obligation the handler must close.
///
/// Every request it covers gets an [`AuthorizationRecord`](super::AuthorizationRecord)
/// on its context. Register it before `PageTokens` and `Idempotency`: a
/// refused request then never claims a token, and every replay is
/// authorized against the caller's current grants.
pub struct Permissions {
    authorizer: Arc<dyn Authorizer>,
    refusal: RefusalFactory,
    allow_undeclared: bool,
    shadow: bool,
    assert_obligations: bool,
}

impl Permissions {
    pub fn new(authorizer: impl Authorizer) -> Self {
        Self {
            authorizer: Arc::new(authorizer),
            refusal: Arc::new(PermissionRefusal::default_error),
            allow_undeclared: false,
            shadow: false,
            assert_obligations: cfg!(any(debug_assertions, feature = "test-support")),
        }
    }

    /// How a Denied verdict is answered: by default
    /// [`PermissionRefusal::default_error`], a 403 `ForbiddenException`
    /// naming the missing permission unless it is internal. A service with
    /// its own error builds it here from
    /// [`PermissionRefusal::public_permission`]; the error must be declared
    /// by the operation or the service. Named like
    /// `AuthenticatorChain::refusal_factory`, `ScopeEnforcer::refusal_factory`
    /// and `Idempotency::refusal_factory`. A Hidden verdict never reaches it:
    /// a 404 of the service's own would tell a hidden resource from a missing
    /// one.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(&PermissionRefusal) -> Error + Send + Sync + 'static,
    ) -> Self {
        self.refusal = Arc::new(factory);
        self
    }

    /// Lets operations without requirement metadata run, each pass logged,
    /// instead of refusing them. For a service that installs an Authorizer
    /// before its model declares permissions; `requires pending` is the
    /// per-operation way.
    pub fn allow_undeclared(mut self) -> Self {
        self.allow_undeclared = true;
        self
    }

    /// Evaluates and records everything, and refuses nothing: what would
    /// have been refused is logged (`would_deny`, `would_hide`,
    /// `would_refuse_undeclared`) and the record says `shadow`. For moving
    /// a service's own checks onto the model one operation at a time, with
    /// the log showing that both agree. Handler checks
    /// ([`require`](super::require), [`discharge`](super::discharge)) still
    /// refuse: the handler asked for them.
    pub fn shadow(mut self) -> Self {
        self.shadow = true;
        self
    }

    /// Whether an obligation left open fails the request: at
    /// `idempotency::commit()` (the step is not taken) and on a success
    /// (answered as an internal failure). On in debug builds and with the
    /// `test-support` feature, so tests catch a handler that forgets one;
    /// off in release builds, which log it and record it as `Undischarged`.
    pub fn assert_obligations(mut self, assert: bool) -> Self {
        self.assert_obligations = assert;
        self
    }

    fn settings(&self) -> Settings {
        Settings {
            authorizer: self.authorizer.clone(),
            refusal: self.refusal.clone(),
            shadow: self.shadow,
            assert_obligations: self.assert_obligations,
        }
    }

    fn undeclared(
        &self,
        context: &Context,
        session: &Session,
        operation: &OperationInfo,
        metadata: &OperationMetadata,
    ) -> Result<()> {
        if !public(metadata) && !self.allow_undeclared {
            if !self.shadow {
                tracing::error!(
                    target: TARGET,
                    operation = operation.id,
                    "a wiring error: the operation has no requirement metadata, so it was refused; declare its requirements (or `requires pending`) and regenerate, or install Permissions with allow_undeclared()"
                );
                return Err(Error::internal(format!(
                    "{}: no requirement metadata, refused by Permissions",
                    operation.id
                )));
            }
            tracing::warn!(
                target: TARGET,
                operation = operation.id,
                outcome = "would_refuse_undeclared",
                "the operation has no requirement metadata"
            );
        } else if self.allow_undeclared && !public(metadata) {
            tracing::info!(
                target: TARGET,
                operation = operation.id,
                "the operation has no requirement metadata and passed (allow_undeclared)"
            );
        }
        session.update(context, |record| record.undeclared = true);
        Ok(())
    }

    async fn enforce(
        &self,
        job: &mut DecodedJob<'_>,
        session: Session,
        requirements: &'static [RequirementInfo],
    ) -> Result<()> {
        let operation = job.operation();
        let context = job.context().clone();
        let (checks, listing) = bind(operation, requirements, job.metadata().checks, job.input())?;

        match session.decide(&context, &checks).await {
            Ok(verdicts) => {
                // The first refusal in declaration order, so batched and
                // sequential checking refuse a request alike.
                if let Some((check, verdict)) = checks
                    .iter()
                    .zip(verdicts)
                    .find(|(_, verdict)| *verdict != Verdict::Allowed)
                {
                    self.refuse(&context, &session, check, verdict)?;
                }
            }
            Err(error) if self.shadow => shadow_failure(operation, &error),
            Err(error) => return Err(error),
        }

        if let Some((index, listing)) = listing {
            match session.visible(&context, &listing).await {
                Ok(Visibility::Hidden) => {
                    let verdict = session.something_to_hide(&listing.resource, Verdict::Hidden);
                    let check = Check::declared(index, listing.permission, listing.resource);
                    self.refuse(&context, &session, &check, verdict)?;
                }
                Ok(_) => {}
                Err(error) if self.shadow => shadow_failure(operation, &error),
                Err(error) => return Err(error),
            }
        }

        let obligations: Vec<ObligationRecord> = requirements
            .iter()
            .enumerate()
            .filter(|(_, requirement)| requirement.mode == Mode::Handler)
            .map(|(index, requirement)| ObligationRecord {
                obligation: Obligation {
                    operation: operation.id,
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    permission: requirement.permission,
                },
                outcome: ObligationOutcome::Open,
            })
            .collect();
        session.update(&context, |record| record.obligations = obligations);
        Ok(())
    }

    /// Refuses `check`, or in shadow mode logs that it would have.
    fn refuse(
        &self,
        context: &Context,
        session: &Session,
        check: &Check,
        verdict: Verdict,
    ) -> Result<()> {
        if self.shadow {
            tracing::warn!(
                target: TARGET,
                operation = session.facts().id,
                permission = check.permission.value,
                resource = %check.resource,
                outcome = if verdict == Verdict::Hidden { "would_hide" } else { "would_deny" },
                "the declared requirements would refuse this request"
            );
            return Ok(());
        }
        Err(session.refuse(context, check, verdict))
    }
}

/// Whether the operation is public: it has no authentication, so it carries
/// no requirements and there is no principal to authorize.
fn public(metadata: &OperationMetadata) -> bool {
    metadata.auth_schemes.is_some_and(<[_]>::is_empty)
}

fn shadow_failure(operation: &OperationInfo, error: &Error) {
    tracing::warn!(
        target: TARGET,
        operation = operation.id,
        cause = %error.chain(),
        outcome = "would_fail",
        "the Authorizer could not decide the declared requirements"
    );
}

/// A wiring error: generated descriptions that disagree with each other.
/// Logged, and answered with the service's internal error.
fn wiring(operation: &OperationInfo, problem: String) -> Error {
    tracing::error!(target: TARGET, operation = operation.id, "a wiring error: {problem}");
    Error::internal(format!("{}: {problem}", operation.id))
}

/// The declared `Check` clauses of a request, and its `filters` listing
/// with the clause's position.
type Bound = (Vec<Check>, Option<(u16, Listing)>);

/// The declared checks of this request, in clause order, and the `filters`
/// listing if the operation has one. Fails closed on anything generated
/// code should not produce, in particular an unconditional clause that
/// bound nothing.
fn bind(
    operation: &OperationInfo,
    requirements: &'static [RequirementInfo],
    checks: Option<ErasedChecks>,
    input: &dyn Input,
) -> Result<Bound> {
    let mut bound = Vec::new();
    if requirements
        .iter()
        .any(|requirement| requirement.mode != Mode::Handler)
    {
        let Some(checks) = checks else {
            return Err(wiring(
                operation,
                "its requirements have no CHECKS function to bind them; regenerate the bindings"
                    .to_owned(),
            ));
        };
        if !checks(input, &mut bound) {
            return Err(wiring(
                operation,
                "the decoded input is not the operation's, so its requirements cannot be bound"
                    .to_owned(),
            ));
        }
    }
    let mut seen = vec![false; requirements.len()];
    for check in &bound {
        let Origin::Declared(index) = check.origin else {
            return Err(wiring(
                operation,
                format!("CHECKS produced a check from {:?}", check.origin),
            ));
        };
        let Some(requirement) = requirements.get(usize::from(index)) else {
            return Err(wiring(
                operation,
                format!("CHECKS bound clause {index}, which the operation does not declare"),
            ));
        };
        if requirement.mode == Mode::Handler {
            return Err(wiring(
                operation,
                format!("CHECKS bound clause {index}, which the handler checks"),
            ));
        }
        if requirement.permission != check.permission {
            return Err(wiring(
                operation,
                format!(
                    "CHECKS bound clause {index} to {}, but it requires {}",
                    check.permission.value, requirement.permission.value
                ),
            ));
        }
        if check.resource.resource() != check.permission.resource || !check.resource.is_complete() {
            return Err(wiring(
                operation,
                format!(
                    "clause {index} binds {}, which does not name one instance of {}",
                    check.resource, check.permission.resource.name
                ),
            ));
        }
        if requirement.mode == Mode::Filter
            && check.resource.identifiers().last() != Some(&Target::All)
        {
            return Err(wiring(
                operation,
                format!(
                    "the `filters` clause {index} must bind the listed resource's own identifier to `*`"
                ),
            ));
        }
        if std::mem::replace(&mut seen[usize::from(index)], true) {
            return Err(wiring(
                operation,
                format!("CHECKS bound clause {index} twice"),
            ));
        }
    }
    if let Some((index, requirement)) =
        requirements
            .iter()
            .enumerate()
            .find(|(index, requirement)| {
                requirement.mode != Mode::Handler && !requirement.conditional && !seen[*index]
            })
    {
        return Err(wiring(
            operation,
            format!(
                "CHECKS bound nothing for clause {index} ({} on {}), which always applies",
                requirement.permission.value, requirement.permission.resource.name
            ),
        ));
    }
    bound.sort_by_key(|check| match check.origin {
        Origin::Declared(index) => index,
        _ => u16::MAX,
    });
    let mut listing = None;
    let mut checks = Vec::with_capacity(bound.len());
    for check in bound {
        let Origin::Declared(index) = check.origin else {
            continue;
        };
        if requirements[usize::from(index)].mode == Mode::Filter {
            if listing.is_some() {
                return Err(wiring(
                    operation,
                    "more than one `filters` clause".to_owned(),
                ));
            }
            listing = Some((
                index,
                Listing {
                    permission: check.permission,
                    resource: check.resource,
                },
            ));
        } else {
            checks.push(check);
        }
    }
    Ok((checks, listing))
}

impl Interceptor for Permissions {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        let metadata = *job.metadata();
        let operation = job.operation();
        let session = Session::new(
            &self.settings(),
            OperationFacts {
                id: operation.id,
                readonly: operation.readonly,
                idempotent: operation.idempotent,
                requirements: metadata.requirements,
            },
        );
        let context = job.context().clone();
        context.set_attribute(session.clone());
        match metadata.requirements {
            Requirements::Undeclared => {
                Hook::Ready(self.undeclared(&context, &session, operation, &metadata))
            }
            Requirements::None => {
                session.update(&context, |_| ());
                Hook::Ready(Ok(()))
            }
            Requirements::Pending { reason } => {
                tracing::debug!(
                    target: TARGET,
                    operation = operation.id,
                    reason,
                    "the operation's requirements are pending; its handler checks"
                );
                session.update(&context, |record| record.pending = Some(reason));
                Hook::Ready(Ok(()))
            }
            Requirements::List(requirements) => {
                Hook::wait(self.enforce(job, session, requirements))
            }
        }
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        super::settle(job);
        Hook::Ready(())
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        if prepare.builtin("permissions", "idempotency") {
            tracing::warn!(
                target: TARGET,
                "Permissions is registered after Idempotency, so a refused request claims its idempotency token and a replay is answered without authorization; register Permissions first"
            );
        }
        let mut problems = Vec::new();
        let mut undeclared = Vec::new();
        for operation in prepare.operations() {
            let Some(metadata) = prepare.metadata(operation) else {
                continue;
            };
            match metadata.requirements {
                Requirements::Undeclared if !public(metadata) => undeclared.push(operation.name),
                Requirements::Pending { reason } if reason.trim().is_empty() => {
                    problems.push(format!("{}: a pending reason is empty", operation.name));
                }
                Requirements::List(requirements) => {
                    if metadata.checks.is_none()
                        && requirements
                            .iter()
                            .any(|requirement| requirement.mode != Mode::Handler)
                    {
                        problems.push(format!(
                            "{}: its requirements have no CHECKS function to bind them; regenerate the bindings",
                            operation.name
                        ));
                    }
                    if requirements
                        .iter()
                        .filter(|requirement| requirement.mode == Mode::Filter)
                        .count()
                        > 1
                    {
                        problems.push(format!(
                            "{}: more than one `filters` clause",
                            operation.name
                        ));
                    }
                }
                _ => {}
            }
        }
        if !undeclared.is_empty() && !self.allow_undeclared {
            tracing::warn!(
                target: TARGET,
                operations = %undeclared.join(", "),
                "these operations have no requirement metadata, and Permissions refuses them with an internal error; declare their requirements (or `requires pending`), or opt out with allow_undeclared()"
            );
        }
        if problems.is_empty() {
            return Ok(());
        }
        Err(format!("Permissions: {}", problems.join("; ")))
    }
}
