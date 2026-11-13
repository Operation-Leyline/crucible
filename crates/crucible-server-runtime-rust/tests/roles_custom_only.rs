//! A service whose roles are all custom (`roles [] custom`), against the
//! catalogue generated from `tests/models/roles-custom-only` (the
//! checked-in golden, compiled here): the authorizer resolves custom roles
//! through `CustomRoles` though the model declares no predefined role.
#![cfg(feature = "roles")]

use std::sync::Arc;

use crucible_server_runtime_rust::permissions::{Check, ResourceRef, Verdict};
use crucible_server_runtime_rust::roles::{
    BindingSource, CustomRoles, PermissionSet, RoleBinding, Root, Unavailable,
};

include!("../../crucible-codegen-rust/tests/golden/roles_custom_only.rs");

use desk::permissions::{NotePermission, NoteRef, Permission, WorkspacePermission, WorkspaceRef};
use desk::roles::{CATALOGUE, Role, Scope};

#[test]
fn the_catalogue_has_scopes_and_no_predefined_role() {
    assert!(CATALOGUE.roles.is_empty());
    assert!(CATALOGUE.custom);
    assert_eq!(Scope::ALL, [Scope::Workspace]);
    assert_eq!(Role::parse("editor"), Role::Custom("editor".into()));
    assert_eq!(Role::parse("editor").as_str(), "editor");
}

#[tokio::test]
async fn a_custom_role_resolves_through_the_hook() {
    let authorizer = desk::roles::authorizer(Source);
    let caller: Arc<[RoleBinding]> = vec![
        RoleBinding::new("editor", "workspace", ["w-1"]),
        RoleBinding::new("unknown", "workspace", ["w-2"]),
    ]
    .into();
    let verdicts = authorizer
        .decide_for(
            &caller,
            &[
                check(NotePermission::Write, NoteRef::new("w-1", "n-1")),
                check(NotePermission::Write, NoteRef::new("w-2", "n-1")),
                // Defined into the role, but `@notGrantable`.
                check(WorkspacePermission::Admin, WorkspaceRef::new("w-1")),
                check(NotePermission::Read, NoteRef::new("w-3", "n-1")),
            ],
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        verdicts,
        [
            Verdict::Allowed,
            Verdict::Hidden,
            Verdict::Denied,
            Verdict::Hidden
        ]
    );
}

fn check(permission: impl Into<Permission>, resource: impl Into<ResourceRef>) -> Check {
    Check::handler(permission.into(), resource.into())
}

struct Source;

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
        _: Root<'_>,
        role: &str,
    ) -> Result<Option<PermissionSet>, Unavailable> {
        Ok((role == "editor").then(|| {
            CATALOGUE.permission_set([
                NotePermission::Read.info(),
                NotePermission::Write.info(),
                WorkspacePermission::Admin.info(),
            ])
        }))
    }
}
