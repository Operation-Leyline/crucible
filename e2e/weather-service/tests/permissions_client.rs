//! The permission constants a generated Rust client gets from
//! `permissions/mail.cru` (docs/permissions.md §6.3): open types that keep
//! a value they do not know and write it back unchanged, deprecated aliases
//! that `canonical()` maps to their primaries, and the registry a console's
//! grant picker reads, without the server's internal permissions.
// The generated client trips two lints on every model: variant names that
// share `Exception`, and service docs before the client's own.
#[allow(clippy::enum_variant_names, clippy::empty_line_after_doc_comments)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/permissions_client.rs"));
}

/// The generators' shared permissions fixture (`tests/fixtures/permissions`),
/// for its `CHECKS`.
#[allow(dead_code)]
mod leyline_fixture {
    include!(concat!(env!("OUT_DIR"), "/leyline_permissions.rs"));
}

use client::permissions::{InboxPermission, PERMISSIONS, WorkspacePermission};
use crucible_client_runtime_rust::permissions::Tier;

/// A binding through a union arm applies only when the arm is selected.
#[test]
fn bindings_read_union_arms() {
    use crucible_server_runtime_rust::Operation;
    use leyline_fixture::leyline::{ExecuteQuery, ExecuteQueryInput, HistoryRef, QueryTarget};
    let checks = |target: Option<QueryTarget>| {
        let mut out = Vec::new();
        (<ExecuteQuery as Operation>::CHECKS.unwrap())(
            &ExecuteQueryInput {
                organization_id: "org_a".into(),
                graph_id: "g_b".into(),
                query: "MATCH".into(),
                target,
            },
            &mut out,
        );
        out.iter()
            .map(|check| format!("{} on {}", check.permission.value, check.resource))
            .collect::<Vec<_>>()
    };
    assert_eq!(checks(None), ["graph:read on graph(org_a, g_b)"]);
    assert_eq!(
        checks(Some(QueryTarget::History(HistoryRef {
            history_id: "h_c".into()
        }))),
        [
            "graph:read on graph(org_a, g_b)",
            "history:read on history(org_a, g_b, h_c)"
        ]
    );
    assert_eq!(
        leyline_fixture::leyline::ExecuteQuery::WRITE_ON_GRAPH
            .permission
            .value,
        "graph:write"
    );
}

#[test]
fn unknown_values_round_trip_unchanged() {
    // A grant written by a newer server, or naming a permission this
    // audience never sees.
    let stored = serde_json::json!(["inbox:read", "inbox:archive", "graph:read"]);
    let parsed: Vec<InboxPermission> = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(parsed[0], InboxPermission::Read);
    assert_eq!(
        parsed[1],
        InboxPermission::UnknownValue("inbox:archive".to_owned())
    );
    assert_eq!(serde_json::to_value(&parsed).unwrap(), stored);
    assert_eq!(InboxPermission::from("mail:send").as_str(), "mail:send");
}

#[test]
#[allow(deprecated)]
fn an_alias_is_kept_as_written_and_canonical_names_its_primary() {
    let alias = InboxPermission::from("email:send");
    assert_eq!(alias, InboxPermission::EmailSend);
    assert_eq!(alias.to_string(), "email:send");
    assert_eq!(alias.canonical(), InboxPermission::MailSend);
    assert_eq!(InboxPermission::Read.canonical(), InboxPermission::Read);
}

#[test]
fn the_registry_has_every_public_permission_and_no_internal_one() {
    let types: Vec<&str> = PERMISSIONS.iter().map(|r| r.resource_type).collect();
    // `graph` declares only an internal permission.
    assert_eq!(types, ["inbox", "template", "workspace"]);
    let workspace = &PERMISSIONS[2];
    let admin = workspace
        .permissions
        .iter()
        .find(|p| p.value == WorkspacePermission::Admin.as_str())
        .unwrap();
    assert_eq!(admin.tier, Tier::NotGrantable);
    assert!(!admin.grantable);
    let inbox = &PERMISSIONS[0];
    let values: Vec<&str> = inbox.permissions.iter().map(|p| p.value).collect();
    assert_eq!(
        values,
        ["email:send", "inbox:manage", "inbox:read", "mail:send"]
    );
    let alias = &inbox.permissions[0];
    assert!(alias.deprecated);
    assert_eq!(alias.alias_of, Some("mail:send"));
    assert_eq!(inbox.permissions[2].description, "The inbox row.");
}
