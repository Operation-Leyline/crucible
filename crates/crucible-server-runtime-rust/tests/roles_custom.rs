//! Custom roles and a service-wide scope (docs/roles.md §4.7), against the
//! catalogue generated from `tests/models/roles-service-scope` (the
//! checked-in golden, compiled here).
#![cfg(feature = "roles")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crucible_server_runtime_rust::permissions::{Check, ResourceRef, Verdict};
use crucible_server_runtime_rust::roles::{
    BindingSource, PermissionSet, RoleAuthorizer, RoleBinding, Root, Unavailable,
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
    let authorizer = RoleAuthorizer::new(&CATALOGUE, Source::default());
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
    let authorizer = RoleAuthorizer::new(&CATALOGUE, source);
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
    let authorizer = RoleAuthorizer::new(
        &CATALOGUE,
        Source {
            fail: true,
            ..Source::default()
        },
    );
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

    async fn custom_role(&self, role: &str) -> Result<Option<PermissionSet>, Unavailable> {
        if self.fail {
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
