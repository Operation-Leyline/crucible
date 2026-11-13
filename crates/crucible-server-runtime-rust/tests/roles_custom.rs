//! Custom roles and a service-wide scope (docs/roles.md §4.7), against the
//! catalogue generated from `tests/models/roles-service-scope` (the
//! checked-in golden, compiled here).
#![cfg(feature = "roles")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crucible_server_runtime_rust::permissions::{Check, ResourceRef, Verdict};
use crucible_server_runtime_rust::roles::{
    BindingSource, CustomRoles, PermissionSet, RoleBinding, Root, Unavailable,
};

include!("../../crucible-codegen-rust/tests/golden/roles_service_scope.rs");

use notes::permissions::{NotePermission, NoteRef, Permission, WorkspacePermission, WorkspaceRef};
use notes::roles::{CATALOGUE, PredefinedRole, Role, Scope};

#[test]
fn the_role_type_is_open_and_the_predefined_one_closed() {
    assert_eq!(
        Role::parse("viewer"),
        Role::Predefined(PredefinedRole::Viewer)
    );
    assert_eq!(Role::parse("auditor"), Role::Custom("auditor".into()));
    assert_eq!(Role::parse("auditor").as_str(), "auditor");
    assert_eq!(
        Role::from(PredefinedRole::Admin).predefined(),
        Some(PredefinedRole::Admin)
    );
    assert_eq!(PredefinedRole::parse("auditor"), None);
    assert!(CATALOGUE.custom);
    // A custom role may hold what is neither internal nor `@notGrantable`.
    let grantable: Vec<&str> = CATALOGUE
        .permissions_in(&CATALOGUE.grantable)
        .map(|p| p.value)
        .collect();
    assert!(grantable.contains(&"workspace:delegate"));
    assert!(!grantable.contains(&"workspace:admin"));
}

#[tokio::test]
async fn a_service_scope_covers_everything() {
    let authorizer = notes::roles::authorizer(Source::default());
    let everywhere = holding([RoleBinding::new(
        "admin",
        "everywhere",
        Vec::<String>::new(),
    )]);
    let verdicts = authorizer
        .decide_for(
            &everywhere,
            &[
                check(WorkspacePermission::Admin, WorkspaceRef::new("w-1")),
                check(NotePermission::Share, NoteRef::new("w-2", "n-1")),
                check(WorkspacePermission::Read, WorkspaceRef::all()),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(verdicts, [Verdict::Allowed; 3]);
    // `editor` binds only at a workspace.
    assert!(!PredefinedRole::Editor.valid_for(Scope::Everywhere, "anyone"));
}

#[tokio::test]
async fn a_custom_role_grants_what_the_service_defines_that_is_grantable() {
    let source = Source::default();
    let asked = source.asked.clone();
    let authorizer = notes::roles::authorizer(source);
    let caller = holding([
        RoleBinding::new("noteSharer", "workspace", ["w-1"]),
        RoleBinding::new("retired", "workspace", ["w-1"]),
    ]);
    let checks = [
        check(NotePermission::Share, NoteRef::new("w-1", "n-1")),
        check(NotePermission::Share, NoteRef::new("w-2", "n-1")),
        // Defined into the role, but `@notGrantable`: not honoured.
        check(WorkspacePermission::Admin, WorkspaceRef::new("w-1")),
        check(NotePermission::Read, NoteRef::new("w-1", "n-1")),
    ];
    let verdicts = authorizer
        .decide_for(&caller, &checks, false)
        .await
        .unwrap();
    assert_eq!(
        verdicts,
        [
            Verdict::Allowed,
            Verdict::Hidden,
            Verdict::Denied,
            Verdict::Denied
        ]
    );
    // Asked once per custom role per batch, though two roots were read.
    assert_eq!(asked.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn an_unavailable_custom_role_never_allows() {
    let authorizer = notes::roles::authorizer(Source {
        fail: true,
        ..Source::default()
    });
    let caller = holding([
        RoleBinding::new("viewer", "workspace", ["w-1"]),
        RoleBinding::new("noteSharer", "workspace", ["w-1"]),
    ]);
    assert!(
        authorizer
            .decide_for(
                &caller,
                &[check(NotePermission::Read, NoteRef::new("w-1", "n-1"))],
                false
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn only_well_formed_bindings_under_the_root_resolve_custom_roles() {
    let source = Source::default();
    let asked = source.asked.clone();
    let authorizer = notes::roles::authorizer(source);
    // The source returns more than the root holds: another workspace's
    // binding, one at a scope the model lacks, and one with the wrong count
    // of identifiers, each naming a custom role whose lookup fails. None is
    // looked up, so none fails the batch.
    let caller = holding([
        RoleBinding::new("noteSharer", "workspace", ["w-1"]),
        RoleBinding::new("otherTenantRole", "workspace", ["w-2"]),
        RoleBinding::new("otherScopeRole", "folder", ["w-1"]),
        RoleBinding::new("otherShapeRole", "workspace", ["w-1", "n-1"]),
    ]);
    let verdicts = authorizer
        .decide_for(
            &caller,
            &[check(NotePermission::Share, NoteRef::new("w-1", "n-1"))],
            false,
        )
        .await
        .unwrap();
    assert_eq!(verdicts, [Verdict::Allowed]);
    assert_eq!(asked.load(Ordering::Relaxed), 1);
    // Checked in w-2, its binding is looked up, and the failure fails it.
    assert!(
        authorizer
            .decide_for(
                &caller,
                &[check(NotePermission::Share, NoteRef::new("w-2", "n-1"))],
                false,
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_custom_role_is_decided_by_its_own_root_s_definition() {
    let source = Source::default();
    let asked = source.asked.clone();
    let authorizer = notes::roles::authorizer(source);
    // `reviewer` (not a predefined role) in both workspaces, which define
    // it differently.
    let caller = holding([
        RoleBinding::new("reviewer", "workspace", ["w-1"]),
        RoleBinding::new("reviewer", "workspace", ["w-2"]),
    ]);
    let verdicts = authorizer
        .decide_for(
            &caller,
            &[
                check(NotePermission::Share, NoteRef::new("w-1", "n-1")),
                // w-1's stronger `reviewer` must not decide w-2's checks.
                check(NotePermission::Share, NoteRef::new("w-2", "n-1")),
                check(NotePermission::Read, NoteRef::new("w-2", "n-1")),
                check(NotePermission::Read, NoteRef::new("w-1", "n-1")),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        verdicts,
        [
            Verdict::Allowed,
            Verdict::Denied,
            Verdict::Allowed,
            Verdict::Denied
        ]
    );
    // Once per (root, id).
    assert_eq!(asked.load(Ordering::Relaxed), 2);
}

fn check(permission: impl Into<Permission>, resource: impl Into<ResourceRef>) -> Check {
    Check::handler(permission.into(), resource.into())
}

fn holding<const N: usize>(bindings: [RoleBinding; N]) -> Arc<[RoleBinding]> {
    bindings.to_vec().into()
}

#[derive(Default)]
struct Source {
    fail: bool,
    asked: Arc<AtomicUsize>,
}

impl BindingSource for Source {
    type Principal = Arc<[RoleBinding]>;

    async fn bindings(
        &self,
        principal: &Self::Principal,
        _: Root<'_>,
        _: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        Ok(principal.clone())
    }
}

impl CustomRoles for Source {
    async fn custom_role(
        &self,
        _: &Self::Principal,
        root: Root<'_>,
        role: &str,
    ) -> Result<Option<PermissionSet>, Unavailable> {
        // `reviewer` is each workspace's own: w-1's shares notes, w-2's only
        // reads them.
        if role == "reviewer" {
            let Root::Instance { id, .. } = root else {
                return Ok(None);
            };
            self.asked.fetch_add(1, Ordering::Relaxed);
            let permission = match id {
                "w-1" => NotePermission::Share,
                "w-2" => NotePermission::Read,
                _ => return Ok(None),
            };
            return Ok(Some(CATALOGUE.permission_set([permission.info()])));
        }
        // Another tenant's roles live in another shard, unreachable here.
        if self.fail || role.starts_with("other") {
            return Err(Unavailable::new("the custom roles table is unreachable"));
        }
        self.asked.fetch_add(1, Ordering::Relaxed);
        Ok((role == "noteSharer").then(|| {
            CATALOGUE.permission_set([
                NotePermission::Share.info(),
                WorkspacePermission::Admin.info(),
            ])
        }))
    }
}
