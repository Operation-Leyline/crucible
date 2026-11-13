//! The role registry a generated Rust client publishes (docs/roles.md §7
//! "Clients"), compiled from `crucible-codegen-rust`'s reviewed golden for
//! `tests/models/roles-granted-by`: open enums that keep a value they do
//! not know and write it back unchanged, and `ROLES` in catalogue order
//! with only public roles, scopes and permissions.

include!("../../crucible-codegen-rust/tests/golden/roles_client.rs");

use crucible_client_runtime_rust::OpenEnum;
use crucible_client_runtime_rust::roles::RoleDescription;
use roles::{ROLES, Role, Scope};

#[test]
fn the_registry_lists_public_roles_in_catalogue_order() {
    let values: Vec<&str> = ROLES.iter().map(|r| r.value).collect();
    assert_eq!(values, ["owner", "viewer", "editor", "archivist"]);
    let owner: &RoleDescription = &ROLES[0];
    assert_eq!(owner.scopes, [Scope::Workspace.as_str()]);
    assert_eq!(owner.holders, ["user"]);
    assert_eq!(owner.assigned_by, [Role::Owner.as_str()]);
    let granted: Vec<String> = owner
        .permissions
        .iter()
        .map(|p| format!("{} {}", p.resource_type, p.permission))
        .collect();
    assert_eq!(
        granted,
        [
            "note note:read",
            "note note:write",
            "note note:create",
            "workspace workspace:read",
            "workspace workspace:admin"
        ]
    );
    assert!(ROLES[1].holders.is_empty() && ROLES[1].assigned_by.is_empty());
    assert_eq!(ROLES[1].scopes, ["workspace", "allNotes", "note"]);
}

#[test]
fn unknown_role_and_scope_values_round_trip_unchanged() {
    assert_eq!(Role::from("viewer"), Role::Viewer);
    let custom = Role::from("custom/reviewer");
    assert_eq!(custom, Role::UnknownValue("custom/reviewer".to_owned()));
    assert_eq!(custom.as_str(), "custom/reviewer");
    assert_eq!(
        Scope::from_string("backOffice".to_owned()).to_string(),
        "backOffice"
    );
    assert_eq!(
        Scope::ALL.iter().map(Scope::as_str).collect::<Vec<_>>(),
        ["workspace", "allNotes", "note"]
    );
}
