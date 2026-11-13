//! Leyline's role policy (docs/roles.md §8), its `policy.rs` tests ported
//! onto the catalogue Crucible generates from `tests/models/roles-leyline`,
//! and `RoleAuthorizer` deciding from it. The generated bindings are the
//! checked-in golden (`crucible-codegen-rust`'s golden test keeps it equal
//! to what the generator writes), compiled here.
#![cfg(feature = "roles")]

mod support;

use std::sync::Arc;

use std::collections::BTreeSet;

use crucible_server_runtime_rust::permissions::{Check, Listing, ResourceRef, Verdict, Visibility};
use crucible_server_runtime_rust::roles::conformance::{Conformance, Subject};
use crucible_server_runtime_rust::roles::{
    BindingSource, ByService, Grant, RoleBinding, Root, ScopeKind, Unavailable,
};
use crucible_server_runtime_rust::roles::{
    RoleAuthorizer, RoleCatalogue, RoleInfo, ScopeRef, ServiceGrants, ServiceHatch,
};

include!("../../crucible-codegen-rust/tests/golden/roles_leyline.rs");

use leyline::permissions::{
    GraphHistoryPermission, GraphHistoryRef, GraphPermission, GraphRef, OrganizationPermission,
    OrganizationRef, Permission,
};
use leyline::roles::{CATALOGUE, Role, Scope};

const USER: &str = "user";
const API_KEY: &str = "apiKey";

fn graph_permissions() -> impl Iterator<Item = GraphPermission> {
    GraphPermission::ALL.iter().copied()
}

#[test]
fn graph_roles_are_hierarchical() {
    let grants = |role: Role| -> Vec<GraphPermission> {
        graph_permissions()
            .filter(|p| role.allows(*p) == Grant::Yes)
            .collect()
    };
    let reader = grants(Role::GraphReader);
    let writer = grants(Role::GraphWriter);
    let admin = grants(Role::GraphAdmin);
    assert_eq!(reader, [GraphPermission::Read]);
    assert!(reader.iter().all(|p| writer.contains(p)) && writer.len() > reader.len());
    assert!(writer.iter().all(|p| admin.contains(p)) && admin.len() > writer.len());
    // Each binds at the same scopes.
    for role in [Role::GraphReader, Role::GraphWriter, Role::GraphAdmin] {
        assert_eq!(
            role.scopes().collect::<Vec<_>>(),
            [Scope::AllGraphs, Scope::Graph]
        );
    }
}

#[test]
fn role_scope_combinations_are_closed() {
    use Role::*;
    use Scope::*;
    let expected: &[(Role, &[Scope])] = &[
        (Owner, &[Organization]),
        (AccessAdmin, &[Organization]),
        (GraphGroupAdmin, &[Organization]),
        (UsageViewer, &[Organization]),
        (AuditViewer, &[Organization]),
        (AuditAdmin, &[Organization]),
        (BackupViewer, &[Organization]),
        (BackupAdmin, &[Organization]),
        (GraphCreator, &[Organization]),
        (GraphReader, &[AllGraphs, Graph]),
        (GraphWriter, &[AllGraphs, Graph]),
        (GraphAdmin, &[AllGraphs, Graph]),
        (HistoryReader, &[AllHistories, History]),
        (HistoryAdmin, &[AllHistories, History]),
    ];
    assert_eq!(Role::ALL.len(), expected.len());
    for (role, scopes) in expected {
        for scope in Scope::ALL {
            assert_eq!(
                role.valid_for(*scope, USER),
                scopes.contains(scope),
                "{role} at {scope}"
            );
        }
    }
}

#[test]
fn api_keys_cannot_become_human_administrators() {
    let privileged = [Role::Owner, Role::AccessAdmin, Role::AuditAdmin];
    for role in Role::ALL {
        let scope = role.scopes().next().unwrap();
        assert!(role.valid_for(scope, USER), "{role}");
        assert_eq!(
            role.valid_for(scope, API_KEY),
            !privileged.contains(role),
            "{role}"
        );
        if privileged.contains(role) {
            assert_eq!(role.holders(), [USER]);
            // Only an owner assigns them.
            assert_eq!(role.assigned_by().collect::<Vec<_>>(), [Role::Owner]);
        } else {
            assert!(role.holders().is_empty());
            assert!(role.assigned_by().next().is_none());
        }
    }
}

#[test]
fn the_owner_holds_every_organization_permission_and_no_graph_permission() {
    for permission in OrganizationPermission::ALL {
        assert_eq!(Role::Owner.allows(*permission), Grant::Yes, "{permission}");
    }
    for permission in graph_permissions() {
        assert_eq!(Role::Owner.allows(permission), Grant::No, "{permission}");
    }
    // Every history, retired ones too, through the organization.
    for permission in GraphHistoryPermission::ALL {
        assert_eq!(Role::Owner.allows(*permission), Grant::Yes);
    }
}

#[test]
fn organization_roles_never_authorize_graph_actions_but_create() {
    for role in Role::ALL {
        if !role.scopes().all(|s| s == Scope::Organization) {
            continue;
        }
        for permission in graph_permissions() {
            let expected = if *role == Role::GraphCreator && permission == GraphPermission::Create {
                Grant::Yes
            } else {
                Grant::No
            };
            assert_eq!(role.allows(permission), expected, "{role} {permission}");
        }
    }
}

#[tokio::test]
async fn graph_creator_only_creates_and_only_at_organization_scope() {
    let granted: Vec<Permission> = Permission::ALL
        .iter()
        .copied()
        .filter(|p| Role::GraphCreator.allows(*p) != Grant::No)
        .collect();
    assert_eq!(granted, [Permission::Graph(GraphPermission::Create)]);
    assert_eq!(
        Role::GraphCreator.scopes().collect::<Vec<_>>(),
        [Scope::Organization]
    );
    let authorizer = authorizer();
    let caller = caller(
        USER,
        [binding(Role::GraphCreator, Scope::Organization, &["o-1"])],
    );
    let verdicts = authorizer
        .decide_for(
            &caller,
            &[
                check(GraphPermission::Create, GraphRef::all("o-1")),
                check(GraphPermission::Create, GraphRef::all("o-2")),
                check(GraphPermission::Read, GraphRef::new("o-1", "g-1")),
            ],
            true,
        )
        .await
        .unwrap();
    // Another organization is hidden; a graph it may not read, denied.
    assert_eq!(
        verdicts,
        [Verdict::Allowed, Verdict::Hidden, Verdict::Denied]
    );
    // At a graph scope it is not valid, so grants nothing there.
    assert!(!Role::GraphCreator.valid_for(Scope::Graph, USER));
}

#[tokio::test]
async fn roles_invalid_where_they_are_bound_grant_nothing() {
    let authorizer = authorizer();
    let owner_read = || {
        check(
            OrganizationPermission::OrganizationRead,
            OrganizationRef::new("o-1"),
        )
    };
    // `owner` held by an API key.
    let key = caller(
        API_KEY,
        [binding(Role::Owner, Scope::Organization, &["o-1"])],
    );
    assert_eq!(
        authorizer
            .decide_for(&key, &[owner_read()], false)
            .await
            .unwrap(),
        [Verdict::Hidden]
    );
    // The same binding held by a user.
    let user = caller(USER, [binding(Role::Owner, Scope::Organization, &["o-1"])]);
    assert_eq!(
        authorizer
            .decide_for(&user, &[owner_read()], false)
            .await
            .unwrap(),
        [Verdict::Allowed]
    );
    // An organization role bound at a graph scope.
    let misbound = caller(
        USER,
        [
            binding(Role::UsageViewer, Scope::Graph, &["o-1", "g-1"]),
            binding(Role::GraphReader, Scope::Organization, &["o-1"]),
        ],
    );
    let verdicts = authorizer
        .decide_for(
            &misbound,
            &[
                check(
                    OrganizationPermission::UsageRead,
                    OrganizationRef::new("o-1"),
                ),
                check(GraphPermission::Read, GraphRef::new("o-1", "g-1")),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(verdicts, [Verdict::Hidden, Verdict::Hidden]);
    // A stored role or scope the model no longer has.
    let stale = caller(
        USER,
        [
            RoleBinding::new("graphOwner", "graph", ["o-1", "g-1"]),
            RoleBinding::new("graphReader", "graphGroup", ["o-1", "g-1"]),
        ],
    );
    assert_eq!(
        authorizer
            .decide_for(
                &stale,
                &[check(GraphPermission::Read, GraphRef::new("o-1", "g-1"))],
                false
            )
            .await
            .unwrap(),
        [Verdict::Hidden]
    );
    assert_eq!(Role::parse("graphOwner"), None);
    assert_eq!(Role::parse("graphReader"), Some(Role::GraphReader));
}

#[tokio::test]
async fn bindings_cover_their_instance_and_everything_beneath_it() {
    let authorizer = authorizer();
    let reader = caller(
        USER,
        [binding(Role::GraphReader, Scope::Graph, &["o-1", "g-1"])],
    );
    let all = caller(
        USER,
        [binding(Role::GraphWriter, Scope::AllGraphs, &["o-1"])],
    );
    let checks = [
        check(GraphPermission::Read, GraphRef::new("o-1", "g-1")),
        check(GraphPermission::Read, GraphRef::new("o-1", "g-2")),
        check(GraphPermission::Write, GraphRef::new("o-1", "g-1")),
        check(GraphPermission::Read, GraphRef::new("o-2", "g-1")),
        check(GraphPermission::Read, GraphRef::all("o-1")),
    ];
    use Verdict::*;
    assert_eq!(
        authorizer
            .decide_for(&reader, &checks, false)
            .await
            .unwrap(),
        // One graph never covers every graph.
        [Allowed, Hidden, Denied, Hidden, Hidden]
    );
    assert_eq!(
        authorizer.decide_for(&all, &checks, false).await.unwrap(),
        [Allowed, Allowed, Allowed, Hidden, Allowed]
    );
}

#[tokio::test]
async fn the_decision_names_the_binding_that_allowed_it() {
    let authorizer = authorizer();
    let allowing = binding(Role::GraphWriter, Scope::AllGraphs, &["o-1"]);
    let caller = caller(
        USER,
        [
            binding(Role::UsageViewer, Scope::Organization, &["o-1"]),
            allowing.clone(),
        ],
    );
    let explained = authorizer
        .explain_for(
            &caller,
            &[
                check(GraphPermission::Write, GraphRef::new("o-1", "g-9")),
                check(GraphPermission::Manage, GraphRef::new("o-1", "g-9")),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        explained,
        [
            (Verdict::Allowed, Some(allowing.clone())),
            (Verdict::Denied, None)
        ]
    );
    assert_eq!(allowing.to_string(), "graphWriter at allGraphs(o-1)");
}

#[tokio::test]
async fn graph_admin_reads_a_history_only_as_the_service_decides() {
    assert_eq!(
        Role::GraphAdmin.allows(GraphHistoryPermission::Read),
        Grant::ByService
    );
    assert_eq!(
        Role::GraphAdmin.allows(GraphHistoryPermission::Restore),
        Grant::No
    );
    let admin = caller(
        USER,
        [binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"])],
    );
    let history = |id: &str| {
        check(
            GraphHistoryPermission::Read,
            GraphHistoryRef::new("o-1", id),
        )
    };
    let checks = [history("history-of-g-1"), history("history-of-g-2")];
    // A hook that relates nothing refuses: a `by service` grant is never a
    // silent allow (and a source without `ServiceGrants` cannot build
    // Leyline's authorizer at all: the `ServiceGrants` doctest). GraphHistory
    // is beside Graph, so no graph binding covers it: the refusal is Hidden.
    let refusing = leyline::roles::authorizer(Memory::default());
    assert_eq!(
        refusing.decide_for(&admin, &checks, false).await.unwrap(),
        [Verdict::Hidden, Verdict::Hidden]
    );
    // A hook that follows the relationship: the history of the binding's
    // graph. It is asked though the binding does not cover the history.
    let deciding = authorizer();
    assert_eq!(
        deciding.explain_for(&admin, &checks, false).await.unwrap(),
        [
            (
                Verdict::Allowed,
                Some(binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"]))
            ),
            (Verdict::Hidden, None)
        ]
    );
    // A model grant answers first: the hook is not asked for the owner.
    let owner = caller(USER, [binding(Role::Owner, Scope::Organization, &["o-1"])]);
    assert_eq!(
        refusing.decide_for(&owner, &checks, false).await.unwrap(),
        [Verdict::Allowed, Verdict::Allowed]
    );
}

/// `filters read on GraphHistory`: the histories of one organization.
fn histories(organization: &str) -> Listing {
    Listing::new(
        GraphHistoryPermission::Read,
        ResourceRef::from(GraphHistoryRef::all(organization)),
    )
}

#[tokio::test]
async fn a_listing_shows_all_some_or_none_of_the_collection() {
    let authorizer = authorizer();
    let visible = |caller: Caller| {
        let authorizer = &authorizer;
        async move {
            authorizer
                .visible_for(&caller, &histories("o-1"), false)
                .await
                .unwrap()
        }
    };
    let only = |ids: &[&str]| Visibility::Only(ids.iter().map(|id| (*id).to_owned()).collect());
    // A binding that covers the whole collection and grants the
    // permission: every history, those made later included.
    assert_eq!(
        visible(caller(
            USER,
            [binding(Role::Owner, Scope::Organization, &["o-1"])]
        ))
        .await,
        Visibility::All
    );
    assert_eq!(
        visible(caller(
            USER,
            [binding(Role::HistoryReader, Scope::AllHistories, &["o-1"])]
        ))
        .await,
        Visibility::All
    );
    // Bindings on single histories: those, unioned.
    assert_eq!(
        visible(caller(
            USER,
            [
                binding(Role::HistoryReader, Scope::History, &["o-1", "h-1"]),
                binding(Role::HistoryAdmin, Scope::History, &["o-1", "h-2"]),
                // Another organization's history is not in this listing.
                binding(Role::HistoryReader, Scope::History, &["o-2", "h-3"]),
            ]
        ))
        .await,
        only(&["h-1", "h-2"])
    );
    // All absorbs the instances.
    assert_eq!(
        visible(caller(
            USER,
            [
                binding(Role::HistoryReader, Scope::History, &["o-1", "h-1"]),
                binding(Role::HistoryReader, Scope::AllHistories, &["o-1"]),
            ]
        ))
        .await,
        Visibility::All
    );
    // Nothing reaches the collection: hidden, as a check on it would be...
    assert_eq!(visible(caller(USER, [])).await, Visibility::Hidden);
    assert_eq!(
        visible(caller(
            USER,
            [binding(Role::GraphReader, Scope::Graph, &["o-1", "g-1"])]
        ))
        .await,
        Visibility::Hidden
    );
    // ...and denied where a binding covers it but grants nothing on it.
    assert_eq!(
        visible(caller(
            USER,
            [binding(Role::UsageViewer, Scope::Organization, &["o-1"])]
        ))
        .await,
        Visibility::Denied
    );
    // A role the principal's kind may not hold shows nothing and covers
    // nothing.
    assert_eq!(
        visible(caller(
            API_KEY,
            [binding(Role::Owner, Scope::Organization, &["o-1"])]
        ))
        .await,
        Visibility::Hidden
    );
}

#[tokio::test]
async fn graph_admin_lists_histories_through_visible_by_service() {
    let authorizer = authorizer();
    let admin = caller(
        USER,
        [
            binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"]),
            binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-2"]),
            binding(Role::HistoryReader, Scope::History, &["o-1", "h-9"]),
        ],
    );
    // The hook names each graph's history; the bindings name theirs; the
    // listing shows the union.
    assert_eq!(
        authorizer
            .visible_for(&admin, &histories("o-1"), false)
            .await
            .unwrap(),
        Visibility::Only(BTreeSet::from([
            "h-9".to_owned(),
            "history-of-g-1".to_owned(),
            "history-of-g-2".to_owned()
        ]))
    );
    // An `allGraphs` binding: the hook answers every history.
    let all = caller(
        USER,
        [binding(Role::GraphAdmin, Scope::AllGraphs, &["o-1"])],
    );
    assert_eq!(
        authorizer
            .visible_for(&all, &histories("o-1"), false)
            .await
            .unwrap(),
        Visibility::All
    );
    // A hook that relates nothing: the graph bindings cover no history, so
    // the listing is hidden, never silently shown.
    let refusing = leyline::roles::authorizer(Memory::default());
    let graph_only = caller(
        USER,
        [binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"])],
    );
    assert_eq!(
        refusing
            .visible_for(&graph_only, &histories("o-1"), false)
            .await
            .unwrap(),
        Visibility::Hidden
    );
    // The hook is not asked once a binding shows the whole collection.
    let source = Memory {
        relate: true,
        ..Memory::default()
    };
    let asked = source.asked.clone();
    let owner_too = leyline::roles::authorizer(source);
    let both = caller(
        USER,
        [
            binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"]),
            binding(Role::Owner, Scope::Organization, &["o-1"]),
        ],
    );
    assert_eq!(
        owner_too
            .visible_for(&both, &histories("o-1"), false)
            .await
            .unwrap(),
        Visibility::All
    );
    assert!(asked.lock().unwrap().is_empty());
    // A hook that cannot tell fails the listing; it never shows it.
    let failing = leyline::roles::authorizer(Memory {
        relate: true,
        fail_by_service: true,
        ..Memory::default()
    });
    assert!(
        failing
            .visible_for(&graph_only, &histories("o-1"), false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn may_assign_values_judges_each_role_from_one_read() {
    let authorizer = authorizer();
    let org = vec!["o-1".to_owned()];
    let at = ScopeRef::new("organization", &org);
    let roles = ["owner", "accessAdmin", "graphCreator", "retired"];
    let admin = caller(
        USER,
        [binding(Role::AccessAdmin, Scope::Organization, &["o-1"])],
    );
    // An access administrator assigns what no `assignedBy` restricts, never
    // an owner-assigned role; a value the catalogue lacks never.
    assert_eq!(
        authorizer
            .may_assign_values(&admin, &roles, at, true)
            .await
            .unwrap(),
        [false, false, true, false]
    );
    let owner = caller(USER, [binding(Role::Owner, Scope::Organization, &["o-1"])]);
    assert_eq!(
        authorizer
            .may_assign_values(&owner, &roles, at, true)
            .await
            .unwrap(),
        [true, true, true, false]
    );
    // In another organization, the owner is no owner.
    let other = vec!["o-2".to_owned()];
    assert_eq!(
        authorizer
            .may_assign_values(&owner, &roles, ScopeRef::new("organization", &other), true)
            .await
            .unwrap(),
        [false, false, true, false]
    );
    // A scope the catalogue lacks, or a malformed instance: nothing.
    assert_eq!(
        authorizer
            .may_assign_values(&owner, &roles, ScopeRef::new("nowhere", &org), true)
            .await
            .unwrap(),
        [false; 4]
    );
}

#[tokio::test]
async fn capabilities_are_the_same_walk_for_every_permission_of_the_resource() {
    let authorizer = authorizer();
    let caller = caller(
        USER,
        [
            binding(Role::GraphWriter, Scope::AllGraphs, &["o-1"]),
            binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"]),
        ],
    );
    let at = |graph: &str| ResourceRef::from(GraphRef::new("o-1", graph));
    let one = authorizer
        .capabilities(&caller, &at("g-1"), false)
        .await
        .unwrap();
    assert_eq!(
        one.permissions().map(|p| p.value).collect::<Vec<_>>(),
        ["graph:read", "graph:write", "graph:manage", "graph:create"]
    );
    let other = authorizer
        .capabilities(&caller, &at("g-2"), false)
        .await
        .unwrap();
    assert!(other.covered);
    assert_eq!(
        other.permissions().map(|p| p.value).collect::<Vec<_>>(),
        ["graph:read", "graph:write"]
    );
    // The `by service` grant, through the hook.
    let history = ResourceRef::from(GraphHistoryRef::new("o-1", "history-of-g-1"));
    let history = authorizer
        .capabilities(&caller, &history, false)
        .await
        .unwrap();
    assert!(!history.covered);
    assert_eq!(
        history.permissions().map(|p| p.value).collect::<Vec<_>>(),
        ["history:read"]
    );
}

#[tokio::test]
async fn an_unavailable_source_never_allows() {
    let caller = caller(USER, [binding(Role::Owner, Scope::Organization, &["o-1"])]);
    let checks = [check(
        OrganizationPermission::OrganizationRead,
        OrganizationRef::new("o-1"),
    )];
    let failing = leyline::roles::authorizer(Memory {
        fail_bindings: true,
        ..Memory::default()
    });
    assert!(failing.decide_for(&caller, &checks, false).await.is_err());
    assert!(
        failing
            .capabilities(&caller, &OrganizationRef::new("o-1").into(), false)
            .await
            .is_err()
    );
    // The `by service` hook failing fails the check it was asked for.
    let admin = caller_with(
        USER,
        [binding(Role::GraphAdmin, Scope::Graph, &["o-1", "g-1"])],
    );
    let failing = leyline::roles::authorizer(Memory {
        fail_by_service: true,
        relate: true,
        ..Memory::default()
    });
    let history = [check(
        GraphHistoryPermission::Read,
        GraphHistoryRef::new("o-1", "history-of-g-1"),
    )];
    assert!(failing.decide_for(&admin, &history, false).await.is_err());
    // A graph check never asks it, so it is answered.
    assert_eq!(
        failing
            .decide_for(
                &admin,
                &[check(GraphPermission::Read, GraphRef::new("o-1", "g-1"))],
                false
            )
            .await
            .unwrap(),
        [Verdict::Allowed]
    );
}

#[tokio::test]
async fn one_read_per_root_per_batch() {
    let memory = Memory {
        relate: true,
        ..Memory::default()
    };
    let reads = memory.reads.clone();
    let authorizer = leyline::roles::authorizer(memory);
    let caller = caller(
        USER,
        [binding(Role::GraphReader, Scope::AllGraphs, &["o-1"])],
    );
    let verdicts = authorizer
        .decide_for(
            &caller,
            &[
                check(GraphPermission::Read, GraphRef::new("o-1", "g-1")),
                check(GraphPermission::Read, GraphRef::new("o-1", "g-2")),
                check(GraphPermission::Read, GraphRef::new("o-2", "g-1")),
                check(GraphPermission::Read, GraphRef::new("o-1", "g-3")),
            ],
            true,
        )
        .await
        .unwrap();
    assert_eq!(verdicts[..2], [Verdict::Allowed, Verdict::Allowed]);
    let reads = reads.lock().unwrap().clone();
    assert_eq!(reads, [("o-1".to_owned(), true), ("o-2".to_owned(), true)]);
}

#[tokio::test]
async fn a_malformed_binding_never_reaches_the_hook() {
    let memory = Memory {
        relate: true,
        ..Memory::default()
    };
    let asked = memory.asked.clone();
    let authorizer = leyline::roles::authorizer(memory);
    // A graph binding with an extra identifier, one with too few, one at a
    // scope the model does not have, and one under another organization:
    // none is well formed under o-1, so none is asked about (and none would
    // relate, though `history-of-g-1` names the graph of the first).
    let caller = caller(
        USER,
        [
            RoleBinding::new("graphAdmin", "graph", ["o-1", "g-1", "extra"]),
            RoleBinding::new("graphAdmin", "graph", ["o-1"]),
            RoleBinding::new("graphAdmin", "graphGroup", ["o-1", "g-1"]),
            RoleBinding::new("graphAdmin", "graph", ["o-2", "g-1"]),
        ],
    );
    let history = check(
        GraphHistoryPermission::Read,
        GraphHistoryRef::new("o-1", "history-of-g-1"),
    );
    assert_eq!(
        authorizer
            .decide_for(&caller, &[history], false)
            .await
            .unwrap(),
        [Verdict::Hidden]
    );
    let capabilities = authorizer
        .capabilities(
            &caller,
            &GraphHistoryRef::new("o-1", "history-of-g-1").into(),
            false,
        )
        .await
        .unwrap();
    assert!(capabilities.allowed.is_empty());
    // Nor does a malformed binding allow anything outright.
    let graph = check(GraphPermission::Read, GraphRef::new("o-1", "g-1"));
    assert_eq!(
        authorizer
            .decide_for(&caller, &[graph], false)
            .await
            .unwrap(),
        [Verdict::Hidden]
    );
    assert!(
        asked.lock().unwrap().is_empty(),
        "{:?}",
        asked.lock().unwrap()
    );
}

#[tokio::test]
async fn only_an_owner_of_the_same_organization_assigns_owner() {
    let at = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    let org_1 = at(&["o-1"]);
    let graph_1 = at(&["o-1", "g-1"]);
    let owner = [binding(Role::Owner, Scope::Organization, &["o-1"])];
    let organization = |ids| ScopeRef::new("organization", ids);
    // The catalogue's helper, pure.
    assert!(Role::Owner.may_assign(&owner, Some(USER), organization(&org_1)));
    assert!(Role::AccessAdmin.may_assign(&owner, Some(USER), organization(&org_1)));
    let other = at(&["o-2"]);
    assert!(!Role::Owner.may_assign(&owner, Some(USER), organization(&other)));
    // An owner binding invalid for its kind, or bound where owner is not.
    assert!(!Role::Owner.may_assign(&owner, Some(API_KEY), organization(&org_1)));
    let misbound = [binding(Role::Owner, Scope::Graph, &["o-1", "g-1"])];
    assert!(!Role::Owner.may_assign(&misbound, Some(USER), organization(&org_1)));
    // Another role assigns nothing assigned by owners.
    let admin = [binding(Role::AccessAdmin, Scope::Organization, &["o-1"])];
    assert!(!Role::Owner.may_assign(&admin, Some(USER), organization(&org_1)));
    // A role without `assignedBy`: whoever the operation's `requires`
    // admits, owner or not.
    assert!(Role::GraphReader.may_assign(&[], Some(USER), ScopeRef::new("graph", &graph_1)));
    // A malformed scope instance admits nothing.
    assert!(!Role::GraphReader.may_assign(&owner, Some(USER), ScopeRef::new("graph", &org_1)));

    // Through the authorizer, which reads the assigner's bindings itself.
    let authorizer = authorizer();
    let user = caller(USER, owner.clone());
    assert!(
        authorizer
            .may_assign(&user, Role::Owner.index(), organization(&org_1), true)
            .await
            .unwrap()
    );
    assert!(
        !authorizer
            .may_assign(&user, Role::Owner.index(), organization(&other), true)
            .await
            .unwrap()
    );
    let key = caller(API_KEY, owner);
    assert!(
        !authorizer
            .may_assign(&key, Role::AuditAdmin.index(), organization(&org_1), true)
            .await
            .unwrap()
    );
}

#[test]
fn scopes_state_their_kind_and_coverage() {
    assert_eq!(Scope::AllGraphs.kind(), ScopeKind::Every);
    assert_eq!(Scope::AllGraphs.info().identifiers, ["organizationId"]);
    assert_eq!(
        Scope::Graph.info().identifiers,
        ["organizationId", "graphId"]
    );
    assert_eq!(Scope::parse("allHistories"), Some(Scope::AllHistories));
    assert_eq!(Scope::parse("graphGroup"), None);
    assert_eq!(
        Scope::Organization.description(),
        "The organization itself: membership, keys, usage, audit and ownership."
    );
    assert_eq!(Role::GraphAdmin.as_str(), "graphAdmin");
    assert_eq!(CATALOGUE.principals, [USER, API_KEY]);
}

/// The catalogue the generator wrote is the one the differential test lays
/// out from the IR, so what that test proves holds for generated code.
#[test]
fn the_generated_catalogue_is_the_ir_s() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/roles-leyline");
    let service = crucible_lang::load_service(&dir).unwrap();
    support::roles::same(&CATALOGUE, support::roles::catalogue(&service)).unwrap();
}

/// A `RoleAuthorizer` over `catalogue`, as a conformance subject would
/// build its own Authorizer.
struct Seeded {
    catalogue: &'static RoleCatalogue,
}

impl Subject for Seeded {
    type Principal = Caller;
    type Authorizer = RoleAuthorizer<Memory, ServiceHatch>;

    fn seed(
        &self,
        kind: Option<&'static str>,
        bindings: &[RoleBinding],
    ) -> (Self::Authorizer, Caller) {
        (
            crucible_server_runtime_rust::roles::role_authorizer(self.catalogue, Memory::default()),
            Caller {
                kind: kind.unwrap_or_default(),
                bindings: bindings.to_vec().into(),
            },
        )
    }
}

#[tokio::test]
async fn conformance_of_the_authorizer_with_itself_is_clean() {
    let report = Conformance::new(&CATALOGUE)
        .run(&Seeded {
            catalogue: &CATALOGUE,
        })
        .await;
    assert!(report.is_clean(), "{report}");
    // Two kinds × (no binding + 15 roles × 5 scopes) × every check.
    assert!(report.cases > 10_000, "{}", report.cases);
}

#[tokio::test]
async fn conformance_reports_a_seeded_disagreement() {
    // An Authorizer whose graphReader also writes.
    let write = CATALOGUE.bit(GraphPermission::Write.info()).unwrap();
    let roles: Vec<RoleInfo> = CATALOGUE
        .roles
        .iter()
        .map(|role| {
            let mut grants = role.grants.clone();
            if role.value == "graphReader" {
                grants.insert(write);
            }
            RoleInfo {
                symbol: role.symbol,
                value: role.value,
                description: role.description,
                internal: role.internal,
                scopes: role.scopes,
                holders: role.holders,
                assigned_by: role.assigned_by,
                grants,
                by_service: role.by_service.clone(),
            }
        })
        .collect();
    let lenient: &'static RoleCatalogue = Box::leak(Box::new(RoleCatalogue {
        roles: Box::leak(roles.into_boxed_slice()),
        scopes: CATALOGUE.scopes,
        permissions: CATALOGUE.permissions,
        principals: CATALOGUE.principals,
        custom: CATALOGUE.custom,
        grantable: CATALOGUE.grantable.clone(),
    }));
    let report = Conformance::new(&CATALOGUE)
        .run(&Seeded { catalogue: lenient })
        .await;
    assert!(!report.is_clean());
    for disagreement in &report.disagreements {
        let binding = disagreement.binding.as_ref().unwrap();
        assert_eq!(binding.role, "graphReader", "{disagreement}");
        assert_eq!(disagreement.check.permission.value, "graph:write");
        assert_eq!(disagreement.expected, Verdict::Denied);
        assert_eq!(disagreement.actual, Ok(Verdict::Allowed));
        assert!(!disagreement.by_service);
    }
    assert!(
        report.to_string().contains(
            "graphReader at allGraphs(a) held by user: graph:write on graph(a, a): expected Denied, got Allowed"
        ),
        "{report}"
    );
}

#[tokio::test]
async fn conformance_flags_cases_a_by_service_answer_decides() {
    // The reference says yes to every `by service` grant; the subject's
    // source keeps the default, no.
    let report = Conformance::new(&CATALOGUE)
        .by_service(true)
        .allowed_only()
        .run(&Seeded {
            catalogue: &CATALOGUE,
        })
        .await;
    assert!(!report.is_clean());
    assert!(report.disagreements.iter().all(|d| d.by_service
        && d.binding.as_ref().unwrap().role == "graphAdmin"
        && d.check.permission.value == "history:read"));
}

// The test source: bindings held in memory by the principal, and a `by
// service` rule that relates a graph to the history named after it.

#[derive(Clone)]
struct Caller {
    kind: &'static str,
    bindings: Arc<[RoleBinding]>,
}

fn caller<const N: usize>(kind: &'static str, bindings: [RoleBinding; N]) -> Caller {
    Caller {
        kind,
        bindings: bindings.to_vec().into(),
    }
}

fn caller_with<const N: usize>(kind: &'static str, bindings: [RoleBinding; N]) -> Caller {
    caller(kind, bindings)
}

fn binding(role: Role, scope: Scope, identifiers: &[&str]) -> RoleBinding {
    RoleBinding::new(role.as_str(), scope.as_str(), identifiers.iter().copied())
}

fn check(permission: impl Into<Permission>, resource: impl Into<ResourceRef>) -> Check {
    Check::handler(permission.into(), resource.into())
}

fn authorizer() -> RoleAuthorizer<Memory, ServiceHatch> {
    leyline::roles::authorizer(Memory {
        relate: true,
        ..Memory::default()
    })
}

#[derive(Default)]
struct Memory {
    /// Answer `by service` grants: a graph binding reaches the history
    /// named `history-of-<graph>`.
    relate: bool,
    fail_bindings: bool,
    fail_by_service: bool,
    reads: Arc<std::sync::Mutex<Vec<(String, bool)>>>,
    /// The bindings `decide_by_service` was asked about.
    asked: Arc<std::sync::Mutex<Vec<RoleBinding>>>,
}

impl BindingSource for Memory {
    type Principal = Caller;

    fn kind<'p>(&self, principal: &'p Caller) -> Option<&'p str> {
        Some(principal.kind)
    }

    async fn bindings(
        &self,
        principal: &Caller,
        root: Root<'_>,
        fresh: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        if self.fail_bindings {
            return Err(Unavailable::new("the bindings table is unreachable"));
        }
        if let Root::Instance { id, .. } = root {
            self.reads.lock().unwrap().push((id.to_owned(), fresh));
        }
        Ok(principal.bindings.clone())
    }
}

impl ServiceGrants for Memory {
    async fn decide_by_service(
        &self,
        _: &Caller,
        grant: ByService<'_>,
    ) -> Result<bool, Unavailable> {
        self.asked.lock().unwrap().push(grant.binding.clone());
        if self.fail_by_service {
            return Err(Unavailable::new("the graph table is unreachable"));
        }
        let Some(graph) = grant.binding.identifiers.get(1) else {
            return Ok(false);
        };
        let history = grant
            .target
            .identifier("historyId")
            .and_then(|t| t.as_one());
        Ok(self.relate && history == Some(format!("history-of-{graph}").as_str()))
    }

    async fn visible_by_service(
        &self,
        _: &Caller,
        grant: ByService<'_>,
    ) -> Result<Visibility, Unavailable> {
        self.asked.lock().unwrap().push(grant.binding.clone());
        if self.fail_by_service {
            return Err(Unavailable::new("the graph table is unreachable"));
        }
        // A graph binding reaches its graph's history; an `allGraphs` one,
        // every graph's, so every history of a live graph: here, all.
        Ok(match grant.binding.identifiers.get(1) {
            _ if !self.relate => Visibility::Only(BTreeSet::new()),
            Some(graph) => Visibility::Only(BTreeSet::from([format!("history-of-{graph}")])),
            None => Visibility::All,
        })
    }
}
