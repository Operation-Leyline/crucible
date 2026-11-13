//! Proving an existing `Authorizer` decides as `RoleAuthorizer` does, before
//! a service switches (docs/roles.md §7.3): [`Conformance`] enumerates every
//! role × scope × principal kind × permission × target shape of a catalogue,
//! asks the service's Authorizer and a `RoleAuthorizer` over the same single
//! binding, and reports each check they answer differently.
//!
//! ```ignore
//! struct Existing;
//! impl Subject for Existing {
//!     type Principal = Caller;
//!     type Authorizer = PolicyAuthorizer;
//!     fn seed(&self, kind: Option<&'static str>, bindings: &[RoleBinding]) -> (PolicyAuthorizer, Caller) {
//!         let caller = Caller::new(kind);
//!         (PolicyAuthorizer::with_bindings(&caller, bindings), caller)
//!     }
//! }
//! let report = Conformance::new(&leyline::roles::CATALOGUE).run(&Existing).await;
//! assert!(report.is_clean(), "{report}");
//! ```

use std::fmt;
use std::sync::Arc;

use axum::http::{HeaderValue, Request};

use super::{
    BindingSource, BothHatches, ByService, CustomRoles, PermissionSet, RoleBinding, RoleCatalogue,
    Root, ServiceGrants, Unavailable, role_authorizer,
};
use crate::Context;
use crate::permissions::{
    AuthorizationRequest, Authorizer, Check, OperationFacts, Requirements, ResourceRef, Target,
    Verdict,
};

/// The role value a run also binds, which no catalogue declares: a stored
/// binding of a role the model has since dropped, which grants nothing.
pub const UNKNOWN_ROLE: &str = "crucible.conformance.unknownRole";

/// The Authorizer under test, as a service builds it for a run.
pub trait Subject: Send + Sync {
    /// What the Authorizer reads from the request context as its caller.
    type Principal: Clone + Send + Sync + 'static;
    type Authorizer: Authorizer;

    /// The Authorizer, seeded so that the returned principal holds exactly
    /// `bindings` (none, or one) and is of kind `kind` (a wire value of the
    /// `principals` enum; `None` when the model names none). The run puts
    /// the principal on each request's context.
    fn seed(
        &self,
        kind: Option<&'static str>,
        bindings: &[RoleBinding],
    ) -> (Self::Authorizer, Self::Principal);
}

/// A conformance run over one catalogue.
#[derive(Debug, Clone, Copy)]
pub struct Conformance {
    catalogue: &'static RoleCatalogue,
    allowed_only: bool,
    by_service: bool,
}

/// What a [`Conformance`] run found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Report {
    /// Checks compared.
    pub cases: usize,
    pub disagreements: Vec<Disagreement>,
}

/// One check the Authorizer under test answered unlike `RoleAuthorizer`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Disagreement {
    pub kind: Option<&'static str>,
    /// The principal's one binding; `None` for a principal with none.
    pub binding: Option<RoleBinding>,
    pub check: Check,
    /// `RoleAuthorizer`'s verdict.
    pub expected: Verdict,
    /// The Authorizer's, or its error.
    pub actual: Result<Verdict, String>,
    /// Whether the binding's role grants the permission `by service`, so
    /// the expected verdict is the run's fixed hook answer, which a
    /// relationship the service follows may rightly overrule.
    pub by_service: bool,
}

impl Conformance {
    pub fn new(catalogue: &'static RoleCatalogue) -> Self {
        Self {
            catalogue,
            allowed_only: false,
            by_service: false,
        }
    }

    /// Compares only whether a check is allowed, not how a refusal reads
    /// (404 or 403): for a service whose refusal policy is its own.
    pub fn allowed_only(mut self) -> Self {
        self.allowed_only = true;
        self
    }

    /// What the reference `RoleAuthorizer`'s `decide_by_service` answers
    /// (by default `false`, its own default).
    pub fn by_service(mut self, answer: bool) -> Self {
        self.by_service = answer;
        self
    }

    /// Runs every case against `subject`.
    pub async fn run<S: Subject>(&self, subject: &S) -> Report {
        let catalogue = self.catalogue;
        let checks = checks(catalogue);
        let kinds: Vec<Option<&'static str>> = if catalogue.principals.is_empty() {
            vec![None]
        } else {
            catalogue.principals.iter().copied().map(Some).collect()
        };
        let mut seeds: Vec<Option<RoleBinding>> = vec![None];
        for role in catalogue
            .roles
            .iter()
            .map(|r| r.value)
            .chain(std::iter::once(UNKNOWN_ROLE))
        {
            for scope in catalogue.scopes {
                seeds.push(Some(RoleBinding::new(
                    role,
                    scope.value,
                    scope.identifiers.iter().map(|_| "a"),
                )));
            }
        }
        let mut report = Report {
            cases: 0,
            disagreements: Vec::new(),
        };
        for kind in kinds {
            for seed in &seeds {
                let bindings: Vec<RoleBinding> = seed.iter().cloned().collect();
                let reference = role_authorizer::<_, BothHatches>(
                    catalogue,
                    Fixed {
                        kind,
                        bindings: bindings.clone().into(),
                        by_service: self.by_service,
                    },
                );
                let expected = reference
                    .decide_for(&(), &checks, false)
                    .await
                    .expect("a fixed source always answers");
                let (authorizer, principal) = subject.seed(kind, &bindings);
                let actual = ask(&authorizer, principal, &checks).await;
                report.cases += checks.len();
                for (index, check) in checks.iter().enumerate() {
                    let expected = self.normalize(check, expected[index]);
                    let actual = match &actual {
                        Ok(verdicts) => Ok(self.normalize(check, verdicts[index])),
                        Err(error) => Err(error.clone()),
                    };
                    if actual.as_ref() == Ok(&expected) {
                        continue;
                    }
                    let by_service = seed.as_ref().is_some_and(|binding| {
                        catalogue.find_role(&binding.role).is_some_and(|role| {
                            catalogue
                                .bit(check.permission)
                                .is_some_and(|bit| catalogue.roles[role].by_service.contains(bit))
                        })
                    });
                    report.disagreements.push(Disagreement {
                        kind,
                        binding: seed.clone(),
                        check: check.clone(),
                        expected,
                        actual,
                        by_service,
                    });
                }
            }
        }
        report
    }

    /// A verdict as the run compares it: Hidden on a target that binds
    /// nothing is Denied (as `Permissions` answers it), and with
    /// [`allowed_only`](Self::allowed_only) every refusal is alike.
    fn normalize(&self, check: &Check, verdict: Verdict) -> Verdict {
        match verdict {
            Verdict::Hidden if self.allowed_only || check.resource.binds_nothing() => {
                Verdict::Denied
            }
            verdict => verdict,
        }
    }
}

impl Report {
    /// Whether the two agreed on every case.
    pub fn is_clean(&self) -> bool {
        self.disagreements.is_empty()
    }
}

/// The count, then the first disagreements, one per line.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} of {} cases disagree",
            self.disagreements.len(),
            self.cases
        )?;
        for disagreement in self.disagreements.iter().take(20) {
            write!(f, "\n  {disagreement}")?;
        }
        if self.disagreements.len() > 20 {
            write!(f, "\n  ..")?;
        }
        Ok(())
    }
}

impl fmt::Display for Disagreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.binding {
            Some(binding) => write!(f, "{binding}")?,
            None => f.write_str("no binding")?,
        }
        if let Some(kind) = self.kind {
            write!(f, " held by {kind}")?;
        }
        write!(
            f,
            ": {} on {}: expected {:?}, got ",
            self.check.permission.value, self.check.resource, self.expected
        )?;
        match &self.actual {
            Ok(verdict) => write!(f, "{verdict:?}")?,
            Err(error) => write!(f, "an error ({error})")?,
        }
        if self.by_service {
            f.write_str(" (by service)")?;
        }
        Ok(())
    }
}

/// Every check a run asks: each permission of the catalogue, on each
/// instance of its resource whose identifiers are `a` or `b`, and on every
/// instance under each such parent.
fn checks(catalogue: &RoleCatalogue) -> Vec<Check> {
    let mut out = Vec::new();
    for permission in catalogue.permissions {
        let resource = permission.resource;
        let count = resource.identifiers.len();
        for ids in vectors(count) {
            out.push(Check::handler(*permission, ResourceRef::new(resource, ids)));
        }
        for parents in vectors(count.saturating_sub(1)) {
            out.push(Check::handler(
                *permission,
                ResourceRef::all(resource, parents),
            ));
        }
    }
    out
}

/// Every vector of `count` identifiers over `a` and `b`.
fn vectors(count: usize) -> Vec<Vec<Target>> {
    let mut out = vec![Vec::new()];
    for _ in 0..count {
        out = out
            .into_iter()
            .flat_map(|prefix: Vec<Target>| {
                ["a", "b"].map(|id| {
                    let mut next = prefix.clone();
                    next.push(Target::from(id));
                    next
                })
            })
            .collect();
    }
    out
}

/// The Authorizer under test's verdicts, through a request as
/// `Permissions` would build it, with the principal on its context.
async fn ask<A: Authorizer, P: Clone + Send + Sync + 'static>(
    authorizer: &A,
    principal: P,
    checks: &[Check],
) -> Result<Vec<Verdict>, String> {
    static NONE: Requirements = Requirements::None;
    let (parts, ()) = Request::new(()).into_parts();
    let context = Context::new(HeaderValue::from_static("conformance"), None, parts);
    context.set_attribute(principal);
    let facts = OperationFacts {
        id: "crucible.roles#Conformance",
        readonly: true,
        idempotent: true,
        requirements: &NONE,
    };
    let request = AuthorizationRequest::new(&context, facts);
    match authorizer.decide(&request, checks).await {
        Ok(verdicts) if verdicts.len() == checks.len() => Ok(verdicts),
        Ok(verdicts) => Err(format!(
            "{} verdicts for {} checks",
            verdicts.len(),
            checks.len()
        )),
        Err(error) => Err(error.to_string()),
    }
}

/// The reference's source: one principal's fixed bindings.
struct Fixed {
    kind: Option<&'static str>,
    bindings: Arc<[RoleBinding]>,
    by_service: bool,
}

impl BindingSource for Fixed {
    type Principal = ();

    fn kind<'p>(&self, _: &'p ()) -> Option<&'p str> {
        self.kind
    }

    async fn bindings(
        &self,
        _: &(),
        _: Root<'_>,
        _: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        Ok(self.bindings.clone())
    }
}

impl ServiceGrants for Fixed {
    async fn decide_by_service(&self, _: &(), _: ByService<'_>) -> Result<bool, Unavailable> {
        Ok(self.by_service)
    }
}

/// A role the catalogue does not declare grants nothing in the reference.
impl CustomRoles for Fixed {
    async fn custom_role(
        &self,
        _: &(),
        _: Root<'_>,
        _: &str,
    ) -> Result<Option<PermissionSet>, Unavailable> {
        Ok(None)
    }
}
