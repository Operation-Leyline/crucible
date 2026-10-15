use std::sync::{Arc, Mutex};

use super::loggable;
use super::session::{NotFound, NotFoundFactory, Session, Settings, TARGET};
use super::{
    Authorizer, Check, ErasedChecks, Listing, Mode, Obligation, ObligationOutcome,
    ObligationRecord, OperationFacts, Origin, PermissionInfo, PermissionRefusal, RequirementInfo,
    Requirements, ResourceRef, Target, Verdict, Visibility,
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
///   model marks the operation public (`Operation::PUBLIC`) or
///   [`allow_undeclared`](Self::allow_undeclared) opts out.
/// - **`requires none`** passes; **`requires pending`** passes with its
///   reason recorded, since the handler still does the checking.
/// - **Requirements**: the operation's `CHECKS` bind each applicable clause
///   to the input, the Authorizer answers them in one batch, and the first
///   verdict in declaration order that is not Allowed refuses the request.
///   Hidden is the framework's 404, or the service's own when the model
///   designates one (what [`not_found_factory`](Self::not_found_factory)
///   builds); Denied is a 403, or what
///   [`refusal_factory`](Self::refusal_factory) builds. A `filters` clause
///   asks [`Authorizer::visible`] for [`visible`](super::visible) (a Hidden
///   or Denied answer refuses like a verdict on that clause), and each
///   `by handler` clause opens an obligation the handler must close.
///
/// Every request it covers gets an [`AuthorizationRecord`](super::AuthorizationRecord)
/// on its context. Every served operation that declares requirements must be
/// covered (scope it with `only` or `except` and the build fails naming the
/// operations left out); [`shadow`](Self::shadow) is the explicit way to
/// record without refusing. Register it before `PageTokens` and
/// `Idempotency` (the other order fails the build): a refused request then
/// never has its page token opened or claims its idempotency token, and a
/// replay is authorized again, its declared checks and the handler checks
/// its success was allowed.
pub struct Permissions {
    /// What every request's session shares, one `Arc` per request: the
    /// Authorizer, the factories, and (once the service is built) the
    /// model's not-found error and the registry.
    settings: Arc<Settings>,
    not_found_factory: Option<NotFoundFactory>,
    allow_undeclared: bool,
    undeclared: UndeclaredOperations,
}

/// The operations a built service serves without requirement metadata,
/// which [`Permissions`] refuses (or, with
/// [`allow_undeclared`](Permissions::allow_undeclared), lets through):
/// what the build logs, for a test or a startup check to assert on. Cheap
/// to clone; every clone reads the same report.
#[derive(Clone, Debug, Default)]
pub struct UndeclaredOperations(Arc<Mutex<Option<Vec<&'static str>>>>);

impl UndeclaredOperations {
    /// The operations' names, current version's first, then those only an
    /// older API version serves; `None` until a service with the
    /// `Permissions` this came from is built. Public operations are never
    /// listed.
    pub fn get(&self) -> Option<Vec<&'static str>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Permissions {
    pub fn new(authorizer: impl Authorizer) -> Self {
        Self {
            settings: Arc::new(Settings {
                authorizer: Arc::new(authorizer),
                refusal: Arc::new(PermissionRefusal::default_error),
                not_found: None,
                registry: Arc::default(),
                shadow: false,
                assert_obligations: true,
            }),
            not_found_factory: None,
            allow_undeclared: false,
            undeclared: UndeclaredOperations::default(),
        }
    }

    /// The report of the operations the built service serves without
    /// requirement metadata, filled when the service is built. Take it
    /// before registering `Permissions`.
    pub fn undeclared_operations(&self) -> UndeclaredOperations {
        self.undeclared.clone()
    }

    /// How a Denied verdict is answered: by default
    /// [`PermissionRefusal::default_error`], a 403 `ForbiddenException`
    /// naming the missing permission unless it is internal. A service with
    /// its own error builds it here from
    /// [`PermissionRefusal::public_permission`]; the error must be declared
    /// by the operation or the service. Named like
    /// `AuthenticatorChain::refusal_factory`
    /// and `Idempotency::refusal_factory`. A Hidden verdict never reaches it:
    /// it is the not-found error ([`not_found_factory`](Self::not_found_factory)).
    /// A `filters` listing the Authorizer answers [`Visibility::Denied`]
    /// does, as a refusal of the clause's permission.
    pub fn refusal_factory(
        mut self,
        factory: impl Fn(&PermissionRefusal) -> Error + Send + Sync + 'static,
    ) -> Self {
        Arc::make_mut(&mut self.settings).refusal = Arc::new(factory);
        self
    }

    /// How a Hidden verdict (and a hidden `filters` listing) is answered
    /// when the model designates the service's own not-found error
    /// (`notFound ResourceNotFoundException` in the `.cru` service): the
    /// factory builds that error from the refused check's resource, its
    /// type and identifiers, never the permission. A declared check's
    /// identifiers are the ones the request named; a handler check's are
    /// what the handler bound, so echo only an identifier the caller sent.
    /// Build it with the same function the handlers use for a genuinely
    /// missing resource, so the two cannot be told apart;
    /// [`permissions::not_found`](super::not_found), a generated
    /// reference's `not_found()` and `ResourceRef::not_found` returned from
    /// a handler all go through it.
    ///
    /// Required when the model designates one, and refused when it does
    /// not (the framework's `NotFoundException` is then the 404): either
    /// mismatch fails the build. So does a factory that builds another
    /// error: the build asks it once per resource the model declares
    /// permissions on. Should it still build another one for a request, that
    /// is answered as an internal error and logged as a wiring error.
    pub fn not_found_factory(
        mut self,
        factory: impl Fn(&ResourceRef) -> Error + Send + Sync + 'static,
    ) -> Self {
        self.not_found_factory = Some(Arc::new(factory));
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
    /// `would_refuse_undeclared`, and `would_fail` for an Authorizer error
    /// or a wiring error) and the record says `shadow`. An unsatisfied
    /// obligation at `idempotency::commit()` is logged, not refused. For
    /// moving a service's own checks onto the model one operation at a
    /// time, with the log showing that both agree. Nothing it decides
    /// refuses, with one exception: during an Authorizer outage (or a wiring
    /// error) a read-only `filters` listing is answered as empty, so
    /// [`visible`](super::visible) shows nothing rather than failing the
    /// handler, and such a success is never replayed; an operation that
    /// changes state fails instead, as it would without shadow mode, since
    /// on an empty listing it would be a no-op success. A replay is
    /// otherwise served whatever its re-authorization says.
    /// Handler checks ([`require`](super::require),
    /// [`discharge`](super::discharge)) still refuse: the handler asked for
    /// them.
    pub fn shadow(mut self) -> Self {
        Arc::make_mut(&mut self.settings).shadow = true;
        self
    }

    /// Whether a success with an obligation unsatisfied (left open, or
    /// discharged with a refusal the handler went on past), or with a
    /// `filters` listing that shows less than everything the handler never
    /// read with [`visible`](super::visible), fails the request as an
    /// internal failure. On by default, in every build: a handler that
    /// forgets a check must not serve its success. `false` is the
    /// deliberate opt-out: the request is then logged and recorded
    /// (`Undischarged`) but keeps its response. Shadow mode never fails it.
    /// `idempotency::commit()` refuses an unsatisfied obligation whatever
    /// this says: the step after it has not happened yet.
    pub fn assert_obligations(mut self, assert: bool) -> Self {
        Arc::make_mut(&mut self.settings).assert_obligations = assert;
        self
    }

    fn undeclared(
        &self,
        context: &Context,
        session: &Session,
        operation: &OperationInfo,
        metadata: &OperationMetadata,
    ) -> Result<()> {
        if !public(metadata) && !self.allow_undeclared {
            if !self.settings.shadow {
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
        let (checks, listing) =
            match bind(operation, requirements, job.metadata().checks, job.input()) {
                Ok(bound) => bound,
                // Shadow mode refuses nothing, its own wiring errors included:
                // the obligations still open, so a handler written for
                // enforcing mode closes them unchanged, and a read-only
                // listing shows nothing (one that changes state fails).
                Err(error) if self.settings.shadow => {
                    shadow_failure(operation, &error);
                    session.listing_unanswered(error)?;
                    open_obligations(&session, operation, requirements);
                    return Ok(());
                }
                Err(error) => return Err(error),
            };

        // The first refusal of a `Check` clause, by clause position.
        let mut refusal = match session.decide(&context, &checks).await {
            Ok(verdicts) => checks
                .iter()
                .zip(verdicts)
                .find(|(_, verdict)| *verdict != Verdict::Allowed)
                .map(|(check, verdict)| (check.clone(), verdict)),
            Err(error) if self.settings.shadow => {
                shadow_failure(operation, &error);
                None
            }
            Err(error) => return Err(error),
        };

        // The listing's own refusal competes by its clause position, so
        // batched and sequential checking refuse a request alike: a Hidden
        // listing declared first is a 404 even when a later clause is
        // Denied, and a Denied one a 403 even when a later clause is
        // Hidden. It is not asked when an earlier clause already refused
        // (unless shadowing, which records everything).
        if let Some((index, listing)) = listing {
            let earlier = refusal
                .as_ref()
                .is_some_and(|(check, _)| clause(check) < index);
            if self.settings.shadow || !earlier {
                match session.visible(&context, &listing).await {
                    Ok(Visibility::Hidden) if !earlier => {
                        let verdict = session.something_to_hide(&listing.resource, Verdict::Hidden);
                        let check = Check::declared(index, listing.permission, listing.resource);
                        refusal = Some((check, verdict));
                    }
                    Ok(Visibility::Denied) if !earlier => {
                        let check = Check::declared(index, listing.permission, listing.resource);
                        refusal = Some((check, Verdict::Denied));
                    }
                    Ok(_) => {}
                    Err(error) if self.settings.shadow => {
                        shadow_failure(operation, &error);
                        session.listing_unanswered(error)?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        if let Some((check, verdict)) = refusal {
            self.refuse(&context, &session, &check, verdict)?;
        }

        open_obligations(&session, operation, requirements);
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
        if self.settings.shadow {
            tracing::warn!(
                target: TARGET,
                operation = session.facts().id,
                permission = check.permission.value,
                resource = %loggable(&check.resource),
                outcome = if verdict == Verdict::Hidden { "would_hide" } else { "would_deny" },
                "the declared requirements would refuse this request"
            );
            return Ok(());
        }
        Err(session.refuse(context, check, verdict))
    }
}

/// Asks the not-found factory once for every resource the model declares
/// permissions on, one instance and (below a root) every instance under a
/// parent, as a hidden listing names it: a factory that builds anything but
/// the designated error fails the build, rather than the first hidden
/// request.
fn probe_not_found(
    service: &crate::operation::ServiceInfo,
    shape: &'static str,
    factory: &NotFoundFactory,
) -> std::result::Result<(), String> {
    let mut resources: Vec<&'static super::ResourceInfo> = Vec::new();
    for permission in service.permissions {
        if !resources.contains(&permission.resource) {
            resources.push(permission.resource);
        }
    }
    for resource in resources {
        let one = ResourceRef::new(resource, resource.identifiers.iter().copied());
        let mut probes = vec![one];
        if let Some((_, parents)) = resource.identifiers.split_last()
            && !parents.is_empty()
        {
            probes.push(ResourceRef::all(resource, parents.iter().copied()));
        }
        for probe in probes {
            let error = factory(&probe);
            let built = (service.classify)(error.get_ref()).map(|modelled| modelled.shape_id());
            if built != Some(shape) {
                return Err(format!(
                    "Permissions: the not_found_factory builds {} for {probe}, not `{shape}`, the model's not-found error: a hidden resource would read unlike a missing one",
                    built.map_or_else(
                        || format!("an unmodelled error ({})", error.chain()),
                        |id| format!("`{id}`")
                    )
                ));
            }
        }
    }
    Ok(())
}

/// Opens the operation's `by handler` obligations on its record.
fn open_obligations(
    session: &Session,
    operation: &OperationInfo,
    requirements: &'static [RequirementInfo],
) {
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
    session.edit(|record| record.obligations = obligations);
}

/// Whether the model marks the operation public: it has no authentication,
/// so it carries no requirements and there is no principal to authorize.
/// Never inferred from an empty list of schemes, which also describes a
/// service authenticated outside the model.
fn public(metadata: &OperationMetadata) -> bool {
    metadata.public
}

/// A declared check's clause position.
fn clause(check: &Check) -> u16 {
    match check.origin {
        Origin::Declared(index) => index,
        _ => u16::MAX,
    }
}

fn shadow_failure(operation: &OperationInfo, error: &Error) {
    tracing::warn!(
        target: TARGET,
        operation = operation.id,
        cause = %error.chain(),
        outcome = "would_fail",
        "the declared requirements could not be decided"
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
                    loggable(&check.resource),
                    check.permission.resource.name
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
    bound.sort_by_key(clause);
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
            self.settings.clone(),
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
            // Every step records; the record is published once, refused
            // or not, for audit middleware that reads it afterwards.
            Requirements::List(requirements) => Hook::wait(async move {
                let result = self.enforce(job, session.clone(), requirements).await;
                session.publish(&context);
                result
            }),
        }
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        super::settle(job);
        Hook::Ready(())
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let after_page_tokens = prepare.registered("page_tokens");
        if prepare.builtin("permissions", "idempotency") {
            return Err(
                "Permissions is registered after Idempotency, so a refused request would claim its idempotency token and a replay would be answered without authorization; register Permissions first".to_owned(),
            );
        }
        if after_page_tokens {
            return Err(
                "Permissions is registered after PageTokens, so a request it refuses would have its pagination token opened first, and a token that does not open answered before the refusal; register Permissions first".to_owned(),
            );
        }
        // A designated not-found error and its factory come together: one
        // without the other would answer a hidden resource unlike a missing
        // one.
        let service = prepare.service();
        let not_found = match (service.not_found, &self.not_found_factory) {
            (Some(shape), Some(factory)) => {
                probe_not_found(service, shape, factory)?;
                Some(NotFound {
                    shape,
                    factory: factory.clone(),
                    classify: service.classify,
                })
            }
            (None, None) => None,
            (Some(shape), None) => {
                return Err(format!(
                    "Permissions: the model answers a missing resource with `{shape}` (`notFound` in the service), so a hidden one must be too: give Permissions a not_found_factory that builds it, with the same function the handlers use"
                ));
            }
            (None, Some(_)) => {
                return Err(
                    "Permissions: a not_found_factory needs the model to designate the service's own not-found error (`notFound <Error>` in the service); without one, a hidden resource is the framework's NotFoundException".to_owned(),
                );
            }
        };
        // What a stored authorization may name: the service's registry and
        // every permission its operations require.
        let mut registry: Vec<&'static PermissionInfo> = prepare.service().permissions.to_vec();
        let mut problems = Vec::new();
        let mut undeclared = Vec::new();
        prepare.authorize_scope();
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
                    for requirement in requirements.iter() {
                        if !registry.contains(&requirement.permission) {
                            registry.push(requirement.permission);
                        }
                    }
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
        *self.undeclared.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(undeclared.clone());
        if !undeclared.is_empty() && !self.allow_undeclared {
            tracing::warn!(
                target: TARGET,
                operations = %undeclared.join(", "),
                "these operations have no requirement metadata, and Permissions refuses them with an internal error; declare their requirements (or `requires pending`), or opt out with allow_undeclared()"
            );
        }
        let settings = Arc::make_mut(&mut self.settings);
        settings.not_found = not_found;
        settings.registry = Arc::new(registry);
        if problems.is_empty() {
            return Ok(());
        }
        Err(format!("Permissions: {}", problems.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::{Authorizer, Permissions};
    use crate::permissions::{AuthorizationRequest, Check, Verdict};

    struct Nobody;
    #[async_trait::async_trait]
    impl Authorizer for Nobody {
        async fn decide(
            &self,
            _: &AuthorizationRequest<'_>,
            checks: &[Check],
        ) -> crate::Result<Vec<Verdict>> {
            Ok(vec![Verdict::Denied; checks.len()])
        }
    }

    #[test]
    fn obligations_are_asserted_by_default_in_every_build() {
        // Not a debug-build default: a release build that forgets a
        // discharge must fail the request too, unless it opts out.
        let permissions = Permissions::new(Nobody);
        assert!(permissions.settings.assert_obligations);
        let opted_out = permissions.assert_obligations(false);
        assert!(!opted_out.settings.assert_obligations);
    }
}
