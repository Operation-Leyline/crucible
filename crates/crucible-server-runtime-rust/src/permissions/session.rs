use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(feature = "idempotency")]
use serde_json::{Value, json};

use super::authorizer_unavailable::AuthorizerUnavailable;
use super::{
    AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Decision, Listing, Mode,
    Obligation, ObligationOutcome, OperationFacts, PermissionInfo, PermissionRefusal, Requirements,
    ResourceRef, Verdict, Visibility,
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

use super::loggable;

/// One request's authorization, which [`Permissions`](super::Permissions)
/// leaves on the request context for the handler functions, the
/// `idempotency::commit` check, a replay's re-authorization and its own
/// `after` hook.
#[derive(Clone)]
pub(crate) struct Session(Arc<State>);

struct State {
    /// The interceptor's, shared by every request it covers.
    settings: Arc<Settings>,
    facts: OperationFacts,
    inner: Mutex<Inner>,
}

struct Inner {
    record: AuthorizationRecord,
    /// The handler-side checks the Authorizer allowed: what a replay of
    /// this request's success must be allowed again. One per permission
    /// and resource.
    handled: Vec<Check>,
    /// Whether the obligations were settled after the operation.
    settled: bool,
    /// The obligations a `discharge` is asking the Authorizer about: taken
    /// before it awaits, so a concurrent `discharge` or `not_applicable`
    /// of the same obligation is refused rather than racing it.
    discharging: Vec<Obligation>,
    /// Whether the handler read the listing's visibility
    /// ([`visible`](super::visible)).
    listing_read: bool,
    /// Shadow mode only: the operation's `filters` listing could not be
    /// answered (an Authorizer error, a wiring error), so the handler is
    /// shown nothing rather than refused.
    listing_unanswered: bool,
}

#[derive(Clone)]
pub(crate) struct Settings {
    pub authorizer: Arc<dyn Authorizer>,
    pub refusal: RefusalFactory,
    pub not_found: Option<NotFound>,
    pub registry: Registry,
    pub shadow: bool,
    pub assert_obligations: bool,
}

/// What a `filters` listing showed, normalized for comparing a stored
/// success's with a replay's.
#[cfg(feature = "idempotency")]
#[derive(Debug, PartialEq, Eq)]
enum Shown {
    All,
    Nothing,
    /// A digest of the ids, so a long listing does not grow the record.
    Only(String),
}

impl Session {
    pub fn new(settings: Arc<Settings>, facts: OperationFacts) -> Self {
        let shadow = settings.shadow;
        Self(Arc::new(State {
            settings,
            facts,
            inner: Mutex::new(Inner {
                record: AuthorizationRecord {
                    operation: facts.id,
                    requirements: facts.requirements,
                    decisions: Vec::new(),
                    listing: None,
                    pending: None,
                    undeclared: false,
                    obligations: Vec::new(),
                    shadow,
                },
                handled: Vec::new(),
                settled: false,
                discharging: Vec::new(),
                listing_read: false,
                listing_unanswered: false,
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

    pub fn shadow(&self) -> bool {
        self.0.settings.shadow
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the record without publishing it: the steps of one hook
    /// change it several times and publish once ([`publish`](Self::publish)).
    pub fn edit<R>(&self, change: impl FnOnce(&mut AuthorizationRecord) -> R) -> R {
        change(&mut self.lock().record)
    }

    /// Leaves the record on the context, for audit middleware that reads it
    /// after the operation: once per hook or handler call, not per change.
    pub fn publish(&self, context: &Context) {
        let record = self.lock().record.clone();
        context.set_attribute(record);
    }

    /// Changes the record and publishes it.
    pub fn update<R>(
        &self,
        context: &Context,
        change: impl FnOnce(&mut AuthorizationRecord) -> R,
    ) -> R {
        let result = self.edit(change);
        self.publish(context);
        result
    }

    /// The Authorizer's verdicts for `checks`, recorded (not published). A
    /// Hidden verdict on a check that binds nothing of the request is
    /// Denied: there is nothing to hide.
    pub async fn decide(&self, context: &Context, checks: &[Check]) -> Result<Vec<Verdict>> {
        if let Some(check) = checks.iter().find(|check| !names_one_instance(check)) {
            return Err(Error::internal(format!(
                "{}: a check of {} on {} does not name one instance of {}",
                self.0.facts.id,
                check.permission.value,
                loggable(&check.resource),
                check.permission.resource.name
            )));
        }
        if checks.is_empty() {
            return Ok(Vec::new());
        }
        let request = AuthorizationRequest::new(context, self.0.facts);
        let verdicts = self
            .0
            .settings
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
        self.edit(|record| {
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
    /// again, and publishing the record.
    pub async fn decide_for_handler(
        &self,
        context: &Context,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let verdicts = self.decide(context, checks).await?;
        remember_allowed(&mut self.lock(), checks, &verdicts);
        self.publish(context);
        Ok(verdicts)
    }

    /// `verdict`, unless it hides a resource the request binds nothing of.
    pub fn something_to_hide(&self, resource: &ResourceRef, verdict: Verdict) -> Verdict {
        if verdict == Verdict::Hidden && resource.binds_nothing() {
            tracing::warn!(
                target: TARGET,
                operation = self.0.facts.id,
                resource = %loggable(resource),
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
                (self.0.settings.refusal)(&refusal)
            }
        }
    }

    /// The 404 for `resource`, hidden or missing alike: the framework's
    /// NotFound, or the service's own error when the model designates one,
    /// built by its factory. A factory that builds anything else would make
    /// a hidden resource tell itself apart, so that is a wiring error,
    /// answered with the service's internal error.
    pub fn not_found(&self, resource: &ResourceRef) -> Error {
        let Some(not_found) = &self.0.settings.not_found else {
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
            resource = %loggable(resource),
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
        self.0.settings.not_found.is_some()
    }

    /// What `listing` may show, recorded (not published).
    pub async fn visible(&self, context: &Context, listing: &Listing) -> Result<Visibility> {
        let request = AuthorizationRequest::new(context, self.0.facts);
        let visibility = self
            .0
            .settings
            .authorizer
            .visible(&request, listing)
            .await
            .map_err(|error| Error::new(AuthorizerUnavailable(error)))?;
        self.edit(|record| {
            record.listing = Some((listing.clone(), visibility.clone()));
        });
        Ok(visibility)
    }

    /// Shadow mode: the operation's listing could not be answered, so
    /// [`visibility`](Self::visibility) shows nothing instead of failing the
    /// handler that reads it.
    pub fn listing_unanswered(&self) {
        if self.shadow() && has_filters(self.0.facts.requirements) {
            self.lock().listing_unanswered = true;
        }
    }

    /// What the handler's [`visible`](super::visible) answers, read.
    pub fn visibility(&self) -> Result<Visibility> {
        let mut inner = self.lock();
        inner.listing_read = true;
        shown(&inner).ok_or_else(|| {
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
        if resource.resource() != obligation.permission.resource {
            return Err(Error::internal(format!(
                "{}: obligation {} ({} on {}) was discharged against {}",
                self.0.facts.id,
                obligation.index,
                obligation.permission.value,
                obligation.permission.resource.name,
                loggable(&resource)
            )));
        }
        // Taken before the Authorizer is asked, and given back if the
        // future is dropped or fails: while it is held, no other close of
        // this obligation can land, and `idempotency::commit()` still sees
        // it open.
        let reservation = self.reserve(obligation)?;
        let check = Check::obligation(&obligation, resource);
        let verdict = self.decide(context, std::slice::from_ref(&check)).await?[0];
        {
            let mut inner = self.lock();
            remember_allowed(&mut inner, std::slice::from_ref(&check), &[verdict]);
            close(
                &mut inner,
                &obligation,
                ObligationOutcome::Discharged(verdict),
            );
        }
        drop(reservation);
        self.publish(context);
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
        {
            // Checked and closed under one lock, so nothing closes it between.
            let mut inner = self.lock();
            self.still_open(&inner, &obligation)?;
            close(
                &mut inner,
                &obligation,
                ObligationOutcome::NotApplicable(reason),
            );
        }
        self.publish(context);
        Ok(())
    }

    /// Marks `obligation` as being discharged, if it is open and no other
    /// discharge holds it.
    fn reserve(&self, obligation: Obligation) -> Result<Reservation<'_>> {
        let mut inner = self.lock();
        self.still_open(&inner, &obligation)?;
        inner.discharging.push(obligation);
        Ok(Reservation {
            session: self,
            obligation,
        })
    }

    /// An obligation is closed once: a second close (a `not_applicable`
    /// after a refused `discharge`, say, or one while a `discharge` of it is
    /// still asking) would overwrite what the record says happened.
    fn still_open(&self, inner: &Inner, obligation: &Obligation) -> Result<()> {
        let outcome = inner
            .record
            .obligations
            .iter()
            .find(|record| record.obligation == *obligation)
            .map(|record| &record.outcome);
        let problem = match outcome {
            Some(ObligationOutcome::Open) if inner.discharging.contains(obligation) => {
                "is being discharged by another call".to_owned()
            }
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

    /// What is not satisfied, described for a log line: obligations still
    /// open or discharged with a refusal the handler went on past, and a
    /// listing that shows less than everything which the handler never
    /// read, so never filtered to.
    fn unsatisfied(&self) -> Option<String> {
        let inner = self.lock();
        let mut unsatisfied: Vec<String> = inner
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
        if !inner.listing_read
            && let Some(shown) = shown(&inner)
            && shown != Visibility::All
        {
            let shown = match shown {
                Visibility::Only(ids) => format!("Only({} ids)", ids.len()),
                other => format!("{other:?}"),
            };
            unsatisfied.push(format!(
                "the `filters` listing ({shown}), never read with permissions::visible()"
            ));
        }
        (!unsatisfied.is_empty()).then(|| unsatisfied.join(", "))
    }

    /// What a replay of this request's success must be allowed again, for
    /// the idempotency record: the handler-side checks, and what the
    /// listing showed.
    #[cfg(feature = "idempotency")]
    fn stored(&self) -> Value {
        let inner = self.lock();
        let checks: Vec<Value> = inner
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
            .collect();
        let listing = match shown(&inner).as_ref().map(Shown::of) {
            None => Value::Null,
            Some(Shown::All) => json!("all"),
            Some(Shown::Nothing) => json!("none"),
            Some(Shown::Only(digest)) => json!({ "only": digest }),
        };
        json!({ "checks": checks, "listing": listing })
    }

    /// The checks a stored success recorded, read back against the
    /// registry, and what its listing showed (`All` for a record that does
    /// not say: the broadest it could have been); `None` when a check no
    /// longer reads (a permission the model dropped), so it cannot be asked
    /// again. A record from before listings were kept is a bare array of
    /// checks.
    #[cfg(feature = "idempotency")]
    fn read_stored(&self, stored: &Value) -> Option<(Vec<Check>, Shown)> {
        let (checks, listing) = match stored {
            Value::Array(checks) => (checks, &Value::Null),
            Value::Object(stored) => (
                stored.get("checks")?.as_array()?,
                stored.get("listing").unwrap_or(&Value::Null),
            ),
            _ => return None,
        };
        let listing = match listing {
            Value::Null => Shown::All,
            Value::String(shown) if shown == "all" => Shown::All,
            Value::String(shown) if shown == "none" => Shown::Nothing,
            Value::Object(only) => Shown::Only(only.get("only")?.as_str()?.to_owned()),
            _ => return None,
        };
        let checks = checks
            .iter()
            .map(|item| {
                let resource = item.get("resource")?.as_str()?;
                let value = item.get("permission")?.as_str()?;
                let permission = self
                    .0
                    .settings
                    .registry
                    .iter()
                    .copied()
                    .find(|permission| {
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
            .collect::<Option<Vec<Check>>>()?;
        Some((checks, listing))
    }
}

/// A `discharge` in progress: gives the obligation back when dropped, so a
/// discharge that failed or was cancelled leaves it open, not stuck.
struct Reservation<'a> {
    session: &'a Session,
    obligation: Obligation,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.session
            .lock()
            .discharging
            .retain(|obligation| *obligation != self.obligation);
    }
}

#[cfg(feature = "idempotency")]
impl Shown {
    fn of(visibility: &Visibility) -> Self {
        match visibility {
            Visibility::All => Self::All,
            Visibility::Only(ids) if !ids.is_empty() => Self::Only(digest(ids)),
            _ => Self::Nothing,
        }
    }

    /// Whether a replay that shows `self` may be served a success that
    /// showed `stored`: the replay shows everything, the stored success
    /// showed nothing, or both show exactly the same ids. Only a digest is
    /// kept, so a current listing that is a strict superset is refused too.
    fn covers(&self, stored: &Self) -> bool {
        *self == Self::All || *stored == Self::Nothing || self == stored
    }
}

/// A listing's ids, as a digest the idempotency record keeps.
#[cfg(feature = "idempotency")]
fn digest(ids: &BTreeSet<String>) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"crucible permissions listing v1\0");
    for id in ids {
        // Length-prefixed, so no two sets digest alike by concatenation.
        digest.update((id.len() as u64).to_le_bytes());
        digest.update(id.as_bytes());
    }
    crate::idempotency::hex(&digest.finalize())
}

/// What the handler's `visible()` answers: the Authorizer's answer, or in
/// shadow mode, when there was none, nothing.
fn shown(inner: &Inner) -> Option<Visibility> {
    match &inner.record.listing {
        Some((_, visibility)) => Some(visibility.clone()),
        None if inner.listing_unanswered => Some(Visibility::Only(BTreeSet::new())),
        None => None,
    }
}

/// Keeps the allowed handler checks for a replay to ask again, each
/// permission and resource once.
fn remember_allowed(inner: &mut Inner, checks: &[Check], verdicts: &[Verdict]) {
    for (check, verdict) in checks.iter().zip(verdicts) {
        let known = inner
            .handled
            .iter()
            .any(|kept| kept.permission == check.permission && kept.resource == check.resource);
        if *verdict == Verdict::Allowed && !known {
            inner.handled.push(check.clone());
        }
    }
}

/// Closes `obligation` with `outcome`, if it is still open.
fn close(inner: &mut Inner, obligation: &Obligation, outcome: ObligationOutcome) {
    for record in &mut inner.record.obligations {
        if record.obligation == *obligation && record.outcome == ObligationOutcome::Open {
            record.outcome = outcome.clone();
        }
    }
    inner
        .discharging
        .retain(|discharging| discharging != obligation);
}

/// Whether the operation has a `filters` clause.
fn has_filters(requirements: &Requirements) -> bool {
    matches!(requirements, Requirements::List(requirements)
        if requirements.iter().any(|requirement| requirement.mode == Mode::Filter))
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
/// must not be passed while an obligation is unsatisfied (or a listing that
/// shows less than everything unread), so the step is not taken. Shadow
/// mode only logs it, since it refuses nothing.
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
        "idempotency::commit() was called with a permission obligation unsatisfied; discharge it (or read the listing's visibility), or say it does not apply, before the point of no return"
    );
    if session.shadow() {
        return Ok(());
    }
    Err(Error::internal(format!(
        "{}: idempotency::commit() with a permission obligation unsatisfied: {unsatisfied}",
        session.0.facts.id
    )))
}

/// After the operation: a success with an obligation unsatisfied (open, or
/// discharged with a refusal the handler swallowed), or with a listing that
/// shows less than everything the handler never read, is a handler bug.
/// It fails the request in every build unless
/// `Permissions::assert_obligations(false)` opted out, or the interceptor
/// only shadows; then it is recorded and logged, and the response kept.
/// Runs once, from whichever of `Permissions` and `Idempotency` unwinds
/// first, so a claim is never completed with a success the assertion then
/// turns into a failure.
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
        "the operation succeeded with a permission obligation unsatisfied; its handler must discharge it (and stop when refused) or say it does not apply, and filter a listing to permissions::visible()"
    );
    if session.0.settings.assert_obligations && !session.shadow() {
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

/// What a replay of this request's success must be allowed again, for the
/// idempotency record of its success; `None` when `Permissions` does not
/// cover it.
#[cfg(feature = "idempotency")]
pub(crate) fn stored_checks(context: &Context) -> Option<Value> {
    context
        .attribute::<Session>()
        .map(|session| session.stored())
}

/// Before a stored success is replayed: asks the Authorizer again for the
/// handler-side checks it recorded, the first that is not Allowed refusing
/// the replay as it would have refused the request, and requires the
/// listing, if the operation filters one, to show now everything, or the
/// same ids the stored success showed (only a digest of them is kept, so a
/// set that grew is refused too). `Ok(false)` when the replay cannot be
/// authorized: a recorded check no longer reads, or the listing is not
/// covered (so the caller is told to send a new token, rather than served
/// what it may no longer see), also when the stored success showed only
/// some ids and the request now has no listing to compare.
/// `Ok(true)` when it may be served, also when `Permissions` does
/// not cover the operation now. Shadow mode refuses nothing here either:
/// what would have been refused, or could not be decided, is logged.
#[cfg(feature = "idempotency")]
pub(crate) async fn reauthorize(context: &Context, stored: &Value) -> Result<bool> {
    let Some(session) = context.attribute::<Session>() else {
        return Ok(true);
    };
    let result = session.reauthorize(context, stored).await;
    session.publish(context);
    match result {
        Ok(true) => Ok(true),
        Ok(false) | Err(_) if session.shadow() => Ok(true),
        result => result,
    }
}

#[cfg(feature = "idempotency")]
impl Session {
    /// Logs what shadow mode let through.
    fn would_fail(&self, error: &Error, what: &str) {
        tracing::warn!(
            target: TARGET,
            operation = self.0.facts.id,
            cause = %error.chain(),
            outcome = "would_fail",
            "{what}"
        );
    }

    async fn reauthorize(&self, context: &Context, stored: &Value) -> Result<bool> {
        let outcome = if self.shadow() {
            "would_refuse"
        } else {
            "refused"
        };
        let Some((checks, listing)) = self.read_stored(stored) else {
            tracing::warn!(
                target: TARGET,
                operation = self.0.facts.id,
                outcome,
                "a stored success names a permission the service no longer declares, so its replay cannot be authorized"
            );
            return Ok(false);
        };
        let verdicts = match self.decide(context, &checks).await {
            Ok(verdicts) => verdicts,
            Err(error) => {
                if self.shadow() {
                    self.would_fail(&error, "a replay's handler checks could not be decided");
                }
                return Err(error);
            }
        };
        if let Some((check, verdict)) = checks
            .iter()
            .zip(verdicts)
            .find(|(_, verdict)| *verdict != Verdict::Allowed)
        {
            if self.shadow() {
                tracing::warn!(
                    target: TARGET,
                    operation = self.0.facts.id,
                    permission = check.permission.value,
                    resource = %loggable(&check.resource),
                    outcome = if verdict == Verdict::Hidden { "would_hide" } else { "would_deny" },
                    "a replay would be refused"
                );
                return Ok(true);
            }
            return Err(self.refuse(context, check, verdict));
        }
        // No listing now (the clause became conditional and did not apply,
        // or the model dropped it): what the caller may see is unknown, so
        // a stored restricted listing is not served.
        let uncovered = match shown(&self.lock()).as_ref().map(Shown::of) {
            Some(now) => !now.covers(&listing),
            None => matches!(listing, Shown::Only(_)),
        };
        if uncovered {
            tracing::warn!(
                target: TARGET,
                operation = self.0.facts.id,
                outcome,
                "the caller's listing no longer covers what the stored success shows, so it is not replayed"
            );
            return Ok(false);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context as TaskContext, Poll};

    use axum::http::{HeaderValue, Request};

    use super::{Session, Settings};
    use crate::Context;
    use crate::permissions::{
        AuthorizationRequest, Authorizer, Check, Listing, Mode, Obligation, ObligationOutcome,
        ObligationRecord, OperationFacts, PermissionInfo, RequirementInfo, Requirements,
        ResourceInfo, ResourceRef, Tier, Verdict, Visibility,
    };

    static WORKSPACE: ResourceInfo = ResourceInfo {
        name: "workspace",
        identifiers: &["workspaceId"],
        not_found: "No such workspace.",
    };
    static INBOX: ResourceInfo = ResourceInfo {
        name: "inbox",
        identifiers: &["workspaceId", "inboxId"],
        not_found: "No such inbox.",
    };
    static ADMIN: PermissionInfo = PermissionInfo {
        resource: &WORKSPACE,
        symbol: "admin",
        value: "workspace:admin",
        aliases: &[],
        tier: Tier::NotGrantable,
        denied: "This principal does not hold workspace:admin on that workspace.",
    };
    static READ: PermissionInfo = PermissionInfo {
        resource: &INBOX,
        symbol: "read",
        value: "inbox:read",
        aliases: &[],
        tier: Tier::Public,
        denied: "This principal does not hold inbox:read on that inbox.",
    };
    static REQUIREMENTS: Requirements = Requirements::None;
    static FILTERS: Requirements = Requirements::List(&[RequirementInfo {
        mode: Mode::Filter,
        permission: &READ,
        conditional: false,
    }]);
    const OBLIGATION: Obligation = Obligation {
        operation: "t#Op",
        index: 0,
        permission: &ADMIN,
    };

    /// Answers every check alike, after yielding once, so a test can hold a
    /// call in flight.
    struct Answering(Verdict);
    #[async_trait::async_trait]
    impl Authorizer for Answering {
        async fn decide(
            &self,
            _: &AuthorizationRequest<'_>,
            checks: &[Check],
        ) -> crate::Result<Vec<Verdict>> {
            YieldOnce(false).await;
            Ok(vec![self.0; checks.len()])
        }
    }

    /// Cannot tell.
    struct Failing;
    #[async_trait::async_trait]
    impl Authorizer for Failing {
        async fn decide(
            &self,
            _: &AuthorizationRequest<'_>,
            _: &[Check],
        ) -> crate::Result<Vec<Verdict>> {
            Err(crate::Error::internal("the grants table is unreachable"))
        }
    }

    struct YieldOnce(bool);
    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    fn settings(authorizer: impl Authorizer, shadow: bool) -> Arc<Settings> {
        Arc::new(Settings {
            authorizer: Arc::new(authorizer),
            refusal: Arc::new(|refusal| refusal.default_error()),
            not_found: None,
            registry: Arc::new(vec![&ADMIN, &READ]),
            shadow,
            assert_obligations: true,
        })
    }

    fn facts(requirements: &'static Requirements) -> OperationFacts {
        OperationFacts {
            id: "t#Op",
            readonly: false,
            idempotent: false,
            requirements,
        }
    }

    fn session(verdict: Verdict, shadow: bool) -> Session {
        Session::new(settings(Answering(verdict), shadow), facts(&REQUIREMENTS))
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

    fn outcome(context: &Context) -> ObligationOutcome {
        context
            .attribute::<crate::permissions::AuthorizationRecord>()
            .unwrap()
            .obligations[0]
            .outcome
            .clone()
    }

    fn listing(session: &Session, visibility: Visibility) {
        session.edit(|record| {
            record.listing = Some((
                Listing {
                    permission: &READ,
                    resource: ResourceRef::all(&INBOX, ["w-1"]),
                },
                visibility,
            ));
        });
    }

    fn only(ids: &[&str]) -> Visibility {
        Visibility::Only(ids.iter().map(|id| (*id).to_owned()).collect())
    }

    #[tokio::test]
    async fn hiding_a_resource_the_request_binds_nothing_of_is_denied() {
        let session = session(Verdict::Hidden, false);
        let context = context();
        let bound = Check::handler(&ADMIN, ResourceRef::new(&WORKSPACE, ["w-1"]));
        let unbound = Check::handler(&ADMIN, ResourceRef::all(&WORKSPACE, Vec::<String>::new()));
        let verdicts = session
            .decide_for_handler(&context, &[bound, unbound])
            .await
            .unwrap();
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
        assert_eq!(
            outcome(&context),
            ObligationOutcome::Discharged(Verdict::Denied)
        );
        assert!(session.unsatisfied().is_some());
    }

    #[tokio::test]
    async fn a_discharge_in_flight_holds_its_obligation() {
        let session = session(Verdict::Denied, false);
        let context = context();
        with_obligation(&session, &context);
        let workspace = ResourceRef::new(&WORKSPACE, ["w-1"]);
        let mut discharge = Box::pin(session.discharge(&context, OBLIGATION, workspace.clone()));
        assert!(futures_util::poll!(&mut discharge).is_pending());
        // While the Authorizer is asked, nothing else closes it: a
        // not_applicable cannot slip past the refusal on its way...
        assert!(
            session
                .not_applicable(&context, OBLIGATION, "never mind".to_owned())
                .is_err()
        );
        // ...nor a second discharge race it...
        assert!(
            session
                .discharge(&context, OBLIGATION, workspace.clone())
                .await
                .is_err()
        );
        // ...and it is still unsatisfied, so commit would refuse.
        assert!(session.unsatisfied().is_some());
        assert!(discharge.await.is_err());
        assert_eq!(
            outcome(&context),
            ObligationOutcome::Discharged(Verdict::Denied)
        );

        // A discharge dropped mid-call gives the obligation back.
        let session = self::session(Verdict::Allowed, false);
        let context = self::context();
        with_obligation(&session, &context);
        {
            let mut discharge = Box::pin(session.discharge(&context, OBLIGATION, workspace));
            assert!(futures_util::poll!(&mut discharge).is_pending());
        }
        session
            .not_applicable(&context, OBLIGATION, "not this case".to_owned())
            .unwrap();
        assert_eq!(
            outcome(&context),
            ObligationOutcome::NotApplicable("not this case".to_owned())
        );
    }

    #[test]
    fn a_listing_that_shows_less_than_everything_must_be_read() {
        for (visibility, read, unsatisfied) in [
            (Visibility::All, false, false),
            (only(&["i-1"]), false, true),
            (only(&["i-1"]), true, false),
        ] {
            let session = session(Verdict::Allowed, false);
            listing(&session, visibility.clone());
            if read {
                assert_eq!(session.visibility().unwrap(), visibility);
            }
            assert_eq!(
                session.unsatisfied().is_some(),
                unsatisfied,
                "{visibility:?}, read {read}"
            );
        }
    }

    #[test]
    fn shadow_shows_nothing_of_a_listing_it_could_not_answer() {
        let shadow = Session::new(settings(Failing, true), facts(&FILTERS));
        shadow.listing_unanswered();
        assert_eq!(
            shadow.visibility().unwrap(),
            Visibility::Only(BTreeSet::new())
        );
        // Enforcing, there is no listing to read: the request was refused.
        let enforcing = Session::new(settings(Failing, false), facts(&FILTERS));
        enforcing.listing_unanswered();
        assert!(enforcing.visibility().is_err());
    }

    #[test]
    fn identifiers_are_escaped_and_cut_short_for_logs() {
        let long = "x".repeat(100);
        let resource = ResourceRef::new(&INBOX, ["w\n1\u{202e}", long.as_str()]);
        let logged = crate::permissions::loggable(&resource);
        assert!(logged.starts_with("inbox(w\\n1\\u{202e}, xxx"), "{logged}");
        assert!(logged.ends_with("...)"), "{logged}");
        assert!(logged.len() < 100, "{logged}");
        let many = ResourceRef::new(&INBOX, vec!["a"; 20]);
        assert_eq!(
            crate::permissions::loggable(&many),
            "inbox(a, a, a, a, a, a, a, a, ...)"
        );
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn allowed_handler_checks_are_stored_once_and_read_back() {
        let session = session(Verdict::Allowed, false);
        let context = context();
        with_obligation(&session, &context);
        session
            .discharge(&context, OBLIGATION, ResourceRef::new(&WORKSPACE, ["w-1"]))
            .await
            .unwrap();
        // The same permission on the same resource again: kept once.
        let again = Check::handler(&ADMIN, ResourceRef::new(&WORKSPACE, ["w-1"]));
        session
            .decide_for_handler(&context, &[again.clone(), again])
            .await
            .unwrap();
        let stored = session.stored();
        assert_eq!(
            stored,
            serde_json::json!({
                "checks": [{
                    "resource": "workspace",
                    "permission": "workspace:admin",
                    "identifiers": ["w-1"],
                    "obligation": 0,
                }],
                "listing": null,
            })
        );
        let (read, _) = session.read_stored(&stored).unwrap();
        assert_eq!(
            read,
            [Check::obligation(
                &OBLIGATION,
                ResourceRef::new(&WORKSPACE, ["w-1"])
            )]
        );
        // A record from before listings were kept is a bare array.
        let legacy = stored["checks"].clone();
        assert_eq!(session.read_stored(&legacy).unwrap().0, read);
        // A permission the registry no longer has cannot be asked again.
        let dropped = serde_json::json!([{
            "resource": "workspace", "permission": "workspace:gone",
            "identifiers": ["w-1"], "obligation": null,
        }]);
        assert!(session.read_stored(&dropped).is_none());
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn a_replay_must_show_no_more_of_a_listing_than_the_caller_sees_now() {
        let stored = |visibility: Option<Visibility>| {
            let session = session(Verdict::Allowed, false);
            if let Some(visibility) = visibility {
                listing(&session, visibility);
            }
            session.stored()
        };
        let cases = [
            // (stored, now, replayed)
            (Some(Visibility::All), Some(Visibility::All), true),
            (Some(Visibility::All), Some(only(&["i-1"])), false),
            (Some(only(&["i-1", "i-2"])), Some(only(&["i-1"])), false),
            (
                Some(only(&["i-1", "i-2"])),
                Some(only(&["i-2", "i-1"])),
                true,
            ),
            // Only a digest is kept: a set that grew is refused as well.
            (Some(only(&["i-1"])), Some(only(&["i-1", "i-2"])), false),
            (Some(only(&["i-1"])), Some(Visibility::All), true),
            (Some(only(&[])), Some(only(&["i-1"])), true),
            // A record that does not say what its listing showed.
            (None, Some(only(&["i-1"])), false),
            (None, Some(Visibility::All), true),
            // No listing now: what the caller may see is unknown.
            (Some(only(&["i-1"])), None, false),
            (Some(Visibility::All), None, true),
        ];
        for (then, now, replayed) in cases {
            let record = stored(then.clone());
            for shadow in [false, true] {
                let session = session(Verdict::Allowed, shadow);
                let context = context();
                context.set_attribute(session.clone());
                if let Some(now) = now.clone() {
                    listing(&session, now);
                }
                let answer = super::reauthorize(&context, &record).await.unwrap();
                // Shadow mode logs what it would refuse, and refuses nothing.
                assert_eq!(
                    answer,
                    replayed || shadow,
                    "{then:?} then, {now:?} now, shadow {shadow}"
                );
            }
        }
    }

    #[cfg(feature = "idempotency")]
    #[tokio::test]
    async fn shadow_replays_what_it_cannot_reauthorize() {
        let record = serde_json::json!({
            "checks": [{
                "resource": "workspace", "permission": "workspace:admin",
                "identifiers": ["w-1"], "obligation": null,
            }],
            "listing": null,
        });
        let gone = serde_json::json!([{
            "resource": "workspace", "permission": "workspace:gone",
            "identifiers": ["w-1"], "obligation": null,
        }]);
        for shadow in [false, true] {
            let context = context();
            // The Authorizer cannot tell.
            context.set_attribute(Session::new(
                settings(Failing, shadow),
                facts(&REQUIREMENTS),
            ));
            let answer = super::reauthorize(&context, &record).await;
            assert_eq!(answer.is_ok(), shadow, "shadow {shadow}");
            // It says Denied.
            context.set_attribute(session(Verdict::Denied, shadow));
            let answer = super::reauthorize(&context, &record).await;
            assert_eq!(answer.is_ok(), shadow, "shadow {shadow}");
            // A permission the model dropped.
            assert_eq!(
                super::reauthorize(&context, &gone).await.unwrap(),
                shadow,
                "shadow {shadow}"
            );
        }
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
