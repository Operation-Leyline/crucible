//! `RoleAuthorizer` as an `Authorizer` (through a request, as `Permissions`
//! asks it) and `ShadowAuthorizer`, over a hand-written catalogue. The
//! generated catalogues are exercised by `tests/roles_*.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::http::{HeaderValue, Request};

use super::{
    BindingSource, PermissionSet, RoleAuthorizer, RoleBinding, RoleCatalogue, RoleInfo, Root,
    ScopeInfo, ScopeKind, ShadowAuthorizer, Unavailable,
};
use crate::Context;
use crate::permissions::{
    AuthorizationRequest, Authorizer, Check, OperationFacts, PermissionInfo, Requirements,
    ResourceInfo, ResourceRef, Tier, Verdict,
};

static ORG: ResourceInfo = ResourceInfo {
    name: "org",
    identifiers: &["orgId"],
    not_found: "No such org.",
};
static DOC: ResourceInfo = ResourceInfo {
    name: "doc",
    identifiers: &["orgId", "docId"],
    not_found: "No such doc.",
};
static ORG_READ: PermissionInfo = PermissionInfo {
    resource: &ORG,
    symbol: "read",
    value: "org:read",
    aliases: &[],
    tier: Tier::Public,
    denied: "",
};
static DOC_READ: PermissionInfo = PermissionInfo {
    resource: &DOC,
    symbol: "read",
    value: "doc:read",
    aliases: &[],
    tier: Tier::Public,
    denied: "",
};
static CATALOGUE: RoleCatalogue = RoleCatalogue {
    roles: &[RoleInfo {
        symbol: "reader",
        value: "reader",
        description: "",
        internal: false,
        scopes: &[0],
        holders: &[],
        assigned_by: &[],
        grants: PermissionSet::from_words(&[0b11]),
        by_service: PermissionSet::EMPTY,
    }],
    scopes: &[ScopeInfo {
        symbol: "org",
        value: "org",
        description: "",
        internal: false,
        kind: ScopeKind::Instance,
        resource: "Org",
        identifiers: &["orgId"],
        instances: &[&ORG, &DOC],
        every: &[&DOC],
    }],
    permissions: &[&DOC_READ, &ORG_READ],
    principals: &[],
    custom: false,
    grantable: PermissionSet::from_words(&[0b11]),
};
static NONE: Requirements = Requirements::None;

#[derive(Clone)]
struct Caller(Arc<[RoleBinding]>);

#[derive(Default)]
struct Source {
    fresh: Arc<Mutex<Vec<bool>>>,
    fail: Arc<AtomicBool>,
}

impl BindingSource for Source {
    type Principal = Caller;

    async fn bindings(
        &self,
        caller: &Caller,
        _: Root<'_>,
        fresh: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(Unavailable::new("down"));
        }
        self.fresh.lock().unwrap().push(fresh);
        Ok(caller.0.clone())
    }
}

/// Answers every check alike, or fails.
struct Fixed(Option<Verdict>);

#[async_trait::async_trait]
impl Authorizer for Fixed {
    async fn decide(
        &self,
        _: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> crate::Result<Vec<Verdict>> {
        match self.0 {
            Some(verdict) => Ok(vec![verdict; checks.len()]),
            None => Err(crate::Error::internal("down")),
        }
    }
}

fn context(caller: Option<Caller>) -> Context {
    let (parts, ()) = Request::new(()).into_parts();
    let context = Context::new(HeaderValue::from_static("r-1"), None, parts);
    if let Some(caller) = caller {
        context.set_attribute(caller);
    }
    context
}

fn facts(readonly: bool) -> OperationFacts {
    OperationFacts {
        id: "t#Op",
        readonly,
        idempotent: readonly,
        requirements: &NONE,
    }
}

fn reader() -> Caller {
    Caller(vec![RoleBinding::new("reader", "org", ["o-1"])].into())
}

fn checks() -> [Check; 2] {
    [
        Check::handler(&DOC_READ, ResourceRef::new(&DOC, ["o-1", "d-1"])),
        Check::handler(&ORG_READ, ResourceRef::new(&ORG, ["o-2"])),
    ]
}

#[tokio::test]
async fn it_decides_for_the_principal_on_the_request() {
    let source = Source::default();
    let fresh = source.fresh.clone();
    let authorizer = RoleAuthorizer::new(&CATALOGUE, source);
    let context = context(Some(reader()));
    for readonly in [true, false] {
        let request = AuthorizationRequest::new(&context, facts(readonly));
        let verdicts = authorizer.decide(&request, &checks()).await.unwrap();
        assert_eq!(verdicts, [Verdict::Allowed, Verdict::Hidden]);
    }
    // Two roots per batch; a write reads fresh.
    assert_eq!(*fresh.lock().unwrap(), [false, false, true, true]);
    // It names the binding that allowed a check.
    let request = AuthorizationRequest::new(&context, facts(true));
    let mut allowed_by = Vec::new();
    authorizer
        .decide_naming_bindings(&request, &checks(), &mut allowed_by)
        .await
        .unwrap();
    assert_eq!(
        allowed_by,
        [Some(RoleBinding::new("reader", "org", ["o-1"])), None]
    );
}

#[tokio::test]
async fn no_principal_and_no_bindings_are_errors_not_allows() {
    let source = Source::default();
    let fail = source.fail.clone();
    let authorizer = RoleAuthorizer::new(&CATALOGUE, source);
    let anonymous = context(None);
    let request = AuthorizationRequest::new(&anonymous, facts(true));
    assert!(authorizer.decide(&request, &checks()).await.is_err());
    fail.store(true, Ordering::Relaxed);
    let context = context(Some(reader()));
    let request = AuthorizationRequest::new(&context, facts(true));
    let error = authorizer.decide(&request, &checks()).await.unwrap_err();
    assert!(error.downcast_ref::<Unavailable>().is_some());
    let mut allowed_by = Vec::new();
    assert!(
        authorizer
            .decide_naming_bindings(&request, &checks(), &mut allowed_by)
            .await
            .is_err()
    );
    assert!(allowed_by.is_empty());
}

#[tokio::test]
async fn shadow_enforces_the_primary_and_counts_disagreements() {
    let context = context(Some(reader()));
    let request = AuthorizationRequest::new(&context, facts(true));
    // The candidate denies both: it disagrees on the allowed one.
    let shadow = ShadowAuthorizer::new(
        RoleAuthorizer::new(&CATALOGUE, Source::default()),
        Fixed(Some(Verdict::Hidden)),
    );
    let counts = shadow.counts();
    assert_eq!(
        shadow.decide(&request, &checks()).await.unwrap(),
        [Verdict::Allowed, Verdict::Hidden]
    );
    assert_eq!((counts.compared(), counts.disagreements()), (2, 1));
    // The primary's bindings are named through it.
    let mut allowed_by = Vec::new();
    shadow
        .decide_naming_bindings(&request, &checks(), &mut allowed_by)
        .await
        .unwrap();
    assert!(allowed_by[0].is_some());
    assert_eq!(counts.disagreements(), 2);
    // A failing candidate is counted, never answered.
    let shadow = ShadowAuthorizer::new(Fixed(Some(Verdict::Denied)), Fixed(None));
    let counts = shadow.counts();
    assert_eq!(
        shadow.decide(&request, &checks()).await.unwrap(),
        [Verdict::Denied; 2]
    );
    assert_eq!((counts.compared(), counts.candidate_failures()), (0, 1));
    // A failing primary fails the request, whatever the candidate says.
    let shadow = ShadowAuthorizer::new(Fixed(None), Fixed(Some(Verdict::Allowed)));
    assert!(shadow.decide(&request, &checks()).await.is_err());
}

#[test]
fn the_catalogue_reads_its_tables() {
    assert_eq!(CATALOGUE.bit(&ORG_READ), Some(1));
    assert_eq!(CATALOGUE.bits_of(&DOC), 0..1);
    assert_eq!(CATALOGUE.bits_of(&ORG), 1..2);
    assert_eq!(CATALOGUE.allows(0, &ORG_READ), super::Grant::Yes);
    assert_eq!(CATALOGUE.allows(1, &ORG_READ), super::Grant::No);
    assert!(CATALOGUE.valid_for(0, 0, None));
    assert!(!CATALOGUE.valid_for(0, 1, None));
    assert!(CATALOGUE.may_assign([], 0));
    let scope = &CATALOGUE.scopes[0];
    let at = |id: &str| vec![id.to_owned()];
    assert!(scope.covers(&at("o-1"), &ResourceRef::new(&DOC, ["o-1", "d-1"])));
    assert!(scope.covers(&at("o-1"), &ResourceRef::all(&DOC, ["o-1"])));
    assert!(!scope.covers(&at("o-1"), &ResourceRef::all(&ORG, Vec::<String>::new())));
    assert!(!scope.covers(&at("o-2"), &ResourceRef::new(&DOC, ["o-1", "d-1"])));
    // A binding with the wrong count of identifiers, or an incomplete
    // target, covers nothing.
    assert!(!scope.covers(&[], &ResourceRef::new(&ORG, ["o-1"])));
    assert!(!scope.covers(&at("o-1"), &ResourceRef::new(&DOC, ["o-1"])));
}
