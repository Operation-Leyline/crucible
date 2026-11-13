//! Who may grant or revoke a role, enforced (docs/roles.md §4.5), and
//! listings answered from role bindings (§7.2), over Leyline's catalogue
//! (`tests/models/roles-leyline`) served with the framework's
//! `RoleAuthorizer`. `PutRoleBinding` declares `assigns role at
//! Organization` and `RevokeRoleBinding` `assigns loaded at Organization`:
//! each opens an obligation its handler discharges with `roles::assign`,
//! which refuses a role the caller may not assign with the operation's 403,
//! and a success that leaves it open fails. `ListGraphHistories` filters by
//! `RoleAuthorizer::visible`, a `graphAdmin` seeing its graph's history
//! through `ServiceGrants::visible_by_service`.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::permissions::{
    self, AuthorizationRecord, ObligationOutcome, Permissions, ResourceRef, Verdict, Visibility,
};
use crucible_server_runtime_rust::roles::{
    self, BindingSource, ByService, RoleBinding, Root, ScopeRef, ServiceGrants, Unavailable,
};
use crucible_server_runtime_rust::{
    Context, Crucible, Error, Interceptor, JsonProtocol, Operations, Result,
};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/roles_leyline.rs"));

use leyline::roles::{Role, Scope};

/// Every binding, by principal: the service's store, which the handlers
/// write and the authorizer reads.
#[derive(Clone, Default)]
struct Store(Arc<Mutex<BTreeMap<String, Vec<RoleBinding>>>>);

impl Store {
    fn grant(&self, principal: &str, role: Role, scope: Scope, ids: &[&str]) -> &Self {
        self.0
            .lock()
            .unwrap()
            .entry(principal.to_owned())
            .or_default()
            .push(RoleBinding::new(
                role.as_str(),
                scope.as_str(),
                ids.iter().copied(),
            ));
        self
    }

    /// The roles `principal` holds at exactly this scope instance.
    fn held(&self, principal: &str, scope: &str, ids: &[String]) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .get(principal)
            .into_iter()
            .flatten()
            .filter(|b| b.scope == scope && b.identifiers == ids)
            .map(|b| b.role.clone())
            .collect()
    }

    /// Replaces what `principal` holds at the scope instance with `role`,
    /// or removes it.
    fn set(&self, principal: &str, scope: &str, ids: &[String], role: Option<&str>) {
        let mut bindings = self.0.lock().unwrap();
        let held = bindings.entry(principal.to_owned()).or_default();
        held.retain(|b| !(b.scope == scope && b.identifiers == ids));
        if let Some(role) = role {
            held.push(RoleBinding::new(role, scope, ids.iter().cloned()));
        }
    }
}

impl BindingSource for Store {
    type Principal = String;

    /// The caller, from a test header: authentication is not under test.
    fn principal(&self, context: &Context) -> Option<String> {
        context
            .headers()
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    fn kind<'p>(&self, principal: &'p String) -> Option<&'p str> {
        Some(if principal.starts_with("key-") {
            "apiKey"
        } else {
            "user"
        })
    }

    async fn bindings(
        &self,
        principal: &String,
        _: Root<'_>,
        _: bool,
    ) -> std::result::Result<Arc<[RoleBinding]>, Unavailable> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(principal)
            .cloned()
            .unwrap_or_default()
            .into())
    }
}

/// A graph administrator reads its graph's history, `history-of-<graph>`.
impl ServiceGrants for Store {
    async fn decide_by_service(
        &self,
        _: &String,
        grant: ByService<'_>,
    ) -> std::result::Result<bool, Unavailable> {
        let graph = grant.binding.identifiers.get(1);
        let history = grant
            .target
            .identifier("historyId")
            .and_then(|t| t.as_one());
        Ok(graph.is_some_and(|graph| history == Some(format!("history-of-{graph}").as_str())))
    }

    async fn visible_by_service(
        &self,
        _: &String,
        grant: ByService<'_>,
    ) -> std::result::Result<Visibility, Unavailable> {
        Ok(match grant.binding.identifiers.get(1) {
            Some(graph) => Visibility::Only([format!("history-of-{graph}")].into()),
            None => Visibility::All,
        })
    }
}

/// The scope instance a binding request names: the organization, and the
/// graph or history for a scope on one.
fn instance(scope: &str, organization: &str, scope_id: Option<&str>) -> Vec<String> {
    let mut ids = vec![organization.to_owned()];
    if matches!(scope, "graph" | "history") {
        ids.push(scope_id.unwrap_or_default().to_owned());
    }
    ids
}

/// The operations not under test answer an error.
macro_rules! unused {
    ($builder:expr, $($operation:ident),+) => {
        $builder$(.add::<leyline::$operation, _, _>(Arc::new(()), |_, _| async {
            Err(Error::internal("not under test"))
        }))+
    };
}

fn operations(store: Store) -> Operations {
    let store = Arc::new(store);
    let builder = unused!(
        Operations::builder(&leyline::SERVICE),
        CreateBackup,
        CreateGraph,
        DeleteGraph,
        GetGraph,
        GetGraphHistory,
        GetOrganization,
        GetUsage,
        ListAuditEvents,
        ListRoles,
        RestoreGraphHistory,
        WriteGraph
    );
    builder
        .add::<leyline::PutRoleBinding, _, _>(
            store.clone(),
            |store, input: leyline::PutRoleBindingInput| async move {
                let scope = input.scope.as_str();
                let ids = instance(scope, &input.organization_id, input.scope_id.as_deref());
                // A handler that forgets the check: its success must fail.
                if input.principal != "forget" {
                    let held = store.held(&input.principal, scope, &ids);
                    // A replacement: the role granted and the role it
                    // replaces must each be one the caller may assign.
                    roles::assign(
                        leyline::PutRoleBinding::ASSIGNMENT,
                        roles::replaced(&held, &[input.role.as_str()]),
                        ScopeRef::new(scope, &ids),
                    )
                    .await?;
                }
                store.set(&input.principal, scope, &ids, Some(input.role.as_str()));
                Ok(())
            },
        )
        .add::<leyline::RevokeRoleBinding, _, _>(
            store.clone(),
            |store, input: leyline::RevokeRoleBindingInput| async move {
                let scope = input.scope.as_str();
                let ids = instance(scope, &input.organization_id, input.scope_id.as_deref());
                // `assigns loaded`: the roles revoked are the stored ones.
                let held = store.held(&input.principal, scope, &ids);
                roles::assign(
                    leyline::RevokeRoleBinding::ASSIGNMENT,
                    &held,
                    ScopeRef::new(scope, &ids),
                )
                .await?;
                store.set(&input.principal, scope, &ids, None);
                Ok(())
            },
        )
        .add::<leyline::ListGraphHistories, _, _>(
            store,
            |_, _: leyline::ListGraphHistoriesInput| async move {
                let visible = permissions::visible()?;
                Ok(leyline::ListGraphHistoriesOutput {
                    history_ids: ["h-1", "h-2", "history-of-g-1", "history-of-g-2"]
                        .into_iter()
                        .filter(|id| visible.shows(id))
                        .map(str::to_owned)
                        .collect(),
                })
            },
        )
        .build()
        .unwrap()
}

fn not_found(_: &ResourceRef) -> Error {
    leyline::ResourceNotFound {
        message: "No such resource.".to_owned(),
    }
    .into()
}

/// Keeps what authorization recorded.
#[derive(Clone, Default)]
struct Audit(Arc<Mutex<Vec<AuthorizationRecord>>>);

impl Audit {
    fn last(&self) -> AuthorizationRecord {
        self.0.lock().unwrap().last().cloned().unwrap()
    }
}

impl Interceptor for Audit {
    fn after<'a>(
        &'a self,
        job: &'a mut crucible_server_runtime_rust::OperationJob<'_>,
    ) -> crucible_server_runtime_rust::Hook<'a, ()> {
        if let Some(record) = job.context().attribute::<AuthorizationRecord>() {
            self.0.lock().unwrap().push(record);
        }
        crucible_server_runtime_rust::Hook::Ready(())
    }
}

struct Harness {
    service: Crucible,
    store: Store,
    audit: Audit,
}

fn serve() -> Harness {
    let store = Store::default();
    let audit = Audit::default();
    let service = Crucible::builder(operations(store.clone()))
        .protocol(JsonProtocol)
        .on_rejection(|_| {
            Box::new(leyline::ResourceNotFound {
                message: "invalid".to_owned(),
            })
        })
        .interceptor(audit.clone())
        .interceptor(
            Permissions::new(leyline::roles::authorizer(store.clone()))
                .not_found_factory(not_found),
        )
        .build()
        .unwrap();
    Harness {
        service,
        store,
        audit,
    }
}

impl Harness {
    async fn call(
        &self,
        method: &str,
        path: &str,
        caller: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-caller", caller)
            .header("content-type", "application/json");
        let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
        let response = self.service.handle(request.body(body).unwrap()).await;
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    /// `caller` binds `principal` to `role` at `scope` in `organization`.
    async fn put(
        &self,
        caller: &str,
        organization: &str,
        principal: &str,
        role: Role,
        scope: Scope,
    ) -> (u16, Value) {
        self.call(
            "PUT",
            &format!("/v1/organizations/{organization}/role-bindings/{principal}"),
            caller,
            Some(json!({ "role": role.as_str(), "scope": scope.as_str() })),
        )
        .await
    }

    async fn revoke(&self, caller: &str, organization: &str, principal: &str) -> (u16, Value) {
        self.call(
            "POST",
            &format!("/v1/organizations/{organization}/role-bindings/{principal}/revoke"),
            caller,
            Some(json!({ "scope": "organization" })),
        )
        .await
    }

    fn holds(&self, principal: &str, organization: &str) -> Vec<String> {
        self.store
            .held(principal, "organization", &[organization.to_owned()])
    }
}

/// The operation's 403: `roleBinding:manage` is internal, so it names no
/// permission.
fn forbidden() -> Value {
    json!({
        "__type": "ForbiddenException",
        "message": "This principal may not perform this operation.",
    })
}

#[tokio::test]
async fn an_access_admin_may_not_write_an_owner_binding_an_owner_may() {
    let harness = serve();
    harness
        .store
        .grant("ada", Role::AccessAdmin, Scope::Organization, &["o-1"])
        .grant("olga", Role::Owner, Scope::Organization, &["o-1"]);

    // The access administrator holds `roleBinding:manage`, so the declared
    // check passes; `owner` is `assignedBy [owner]`, so the assignment is
    // refused with the operation's 403, and nothing is written.
    let (status, body) = harness
        .put("ada", "o-1", "bob", Role::Owner, Scope::Organization)
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body, forbidden());
    assert!(harness.holds("bob", "o-1").is_empty());
    let record = harness.audit.last();
    let assignment = record.assignment.unwrap();
    assert_eq!(
        assignment.outcome,
        ObligationOutcome::Discharged(Verdict::Denied)
    );
    assert_eq!(assignment.roles, [("owner".to_owned(), false)]);
    assert_eq!(
        assignment.at,
        Some(("organization".to_owned(), vec!["o-1".to_owned()]))
    );

    // A role no `assignedBy` restricts is whoever `requires` admits.
    let (status, body) = harness
        .put("ada", "o-1", "bob", Role::GraphCreator, Scope::Organization)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(harness.holds("bob", "o-1"), ["graphCreator"]);

    // An owner of the same organization assigns `owner`.
    let (status, body) = harness
        .put("olga", "o-1", "carol", Role::Owner, Scope::Organization)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(harness.holds("carol", "o-1"), ["owner"]);
    assert_eq!(
        harness.audit.last().assignment.unwrap().outcome,
        ObligationOutcome::Discharged(Verdict::Allowed)
    );
}

#[tokio::test]
async fn an_owner_of_one_organization_assigns_nothing_in_another() {
    let harness = serve();
    // An owner of o-1 who administers access in o-2: the declared check
    // passes in o-2, and ownership of o-1 admits nothing there.
    harness
        .store
        .grant("olga", Role::Owner, Scope::Organization, &["o-1"])
        .grant("olga", Role::AccessAdmin, Scope::Organization, &["o-2"]);
    let (status, body) = harness
        .put("olga", "o-2", "bob", Role::Owner, Scope::Organization)
        .await;
    assert_eq!(status, 403, "{body}");
    assert!(harness.holds("bob", "o-2").is_empty());
    // An API key holding an owner binding is no owner (`holders [USER]`).
    harness
        .store
        .grant("key-1", Role::Owner, Scope::Organization, &["o-1"])
        .grant(
            "key-1",
            Role::GraphGroupAdmin,
            Scope::Organization,
            &["o-1"],
        );
    let (status, _) = harness
        .put("key-1", "o-1", "bob", Role::Owner, Scope::Organization)
        .await;
    // Its owner binding is inert, so it lacks `roleBinding:manage` too:
    // refused before the handler, as a declared check.
    assert_eq!(status, 403);
    assert!(harness.audit.last().assignment.unwrap().outcome == ObligationOutcome::Open);
}

#[tokio::test]
async fn revoking_an_owner_binding_is_checked_against_the_stored_role() {
    let harness = serve();
    harness
        .store
        .grant("ada", Role::AccessAdmin, Scope::Organization, &["o-1"])
        .grant("olga", Role::Owner, Scope::Organization, &["o-1"])
        .grant("bob", Role::Owner, Scope::Organization, &["o-1"])
        .grant("carol", Role::GraphCreator, Scope::Organization, &["o-1"]);
    // `assigns loaded`: the access administrator may not revoke an owner.
    let (status, body) = harness.revoke("ada", "o-1", "bob").await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body, forbidden());
    assert_eq!(harness.holds("bob", "o-1"), ["owner"]);
    assert_eq!(
        harness.audit.last().assignment.unwrap().roles,
        [("owner".to_owned(), false)]
    );
    // ...and may revoke a role no `assignedBy` restricts.
    let (status, _) = harness.revoke("ada", "o-1", "carol").await;
    assert_eq!(status, 200);
    assert!(harness.holds("carol", "o-1").is_empty());
    // An owner revokes an owner.
    let (status, _) = harness.revoke("olga", "o-1", "bob").await;
    assert_eq!(status, 200);
    assert!(harness.holds("bob", "o-1").is_empty());
}

#[tokio::test]
async fn a_replacement_checks_the_role_it_removes_and_the_role_it_adds() {
    let harness = serve();
    harness
        .store
        .grant("ada", Role::AccessAdmin, Scope::Organization, &["o-1"])
        .grant("bob", Role::Owner, Scope::Organization, &["o-1"])
        .grant("carol", Role::GraphCreator, Scope::Organization, &["o-1"]);
    // Replacing bob's `owner` with `auditViewer` revokes `owner`: refused,
    // though `auditViewer` alone is the access administrator's to grant.
    let (status, _) = harness
        .put("ada", "o-1", "bob", Role::AuditViewer, Scope::Organization)
        .await;
    assert_eq!(status, 403);
    assert_eq!(harness.holds("bob", "o-1"), ["owner"]);
    assert_eq!(
        harness.audit.last().assignment.unwrap().roles,
        [
            ("owner".to_owned(), false),
            ("auditViewer".to_owned(), true)
        ]
    );
    // Replacing carol's `graphCreator` with `owner` grants `owner`: refused.
    let (status, _) = harness
        .put("ada", "o-1", "carol", Role::Owner, Scope::Organization)
        .await;
    assert_eq!(status, 403);
    assert_eq!(harness.holds("carol", "o-1"), ["graphCreator"]);
    // Replacing it with `auditViewer` changes two roles neither of which an
    // owner alone assigns: allowed.
    let (status, _) = harness
        .put(
            "ada",
            "o-1",
            "carol",
            Role::AuditViewer,
            Scope::Organization,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(harness.holds("carol", "o-1"), ["auditViewer"]);
}

#[tokio::test]
async fn a_success_with_the_assignment_undischarged_fails() {
    let harness = serve();
    harness
        .store
        .grant("ada", Role::AccessAdmin, Scope::Organization, &["o-1"]);
    // The handler writes without `roles::assign`: the write happened, but
    // the success is refused as an internal error, as an undischarged `by
    // handler` clause's is.
    let (status, body) = harness
        .put("ada", "o-1", "forget", Role::Owner, Scope::Organization)
        .await;
    assert_eq!(status, 500, "{body}");
    let text = body.to_string();
    assert!(!text.contains("assign"), "the 500 leaks the cause: {text}");
    assert_eq!(
        harness.audit.last().assignment.unwrap().outcome,
        ObligationOutcome::Undischarged
    );
}

#[tokio::test]
async fn a_graph_admin_lists_its_graphs_history_through_the_service() {
    let harness = serve();
    harness
        .store
        .grant("gina", Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"])
        .grant("hank", Role::HistoryReader, Scope::History, &["o-1", "h-2"])
        .grant("olga", Role::Owner, Scope::Organization, &["o-1"])
        .grant("uma", Role::UsageViewer, Scope::Organization, &["o-1"]);
    let list = |caller: &'static str| {
        let harness = &harness;
        async move {
            harness
                .call("GET", "/v1/organizations/o-1/histories", caller, None)
                .await
        }
    };
    let (status, body) = list("gina").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["historyIds"], json!(["history-of-g-1"]));
    let (_, body) = list("hank").await;
    assert_eq!(body["historyIds"], json!(["h-2"]));
    let (_, body) = list("olga").await;
    assert_eq!(
        body["historyIds"],
        json!(["h-1", "h-2", "history-of-g-1", "history-of-g-2"])
    );
    // A binding that covers the organization and grants no history read:
    // denied; none at all: hidden, the model's not-found error.
    assert_eq!(list("uma").await.0, 403);
    assert_eq!(list("nobody").await.0, 404);
}
