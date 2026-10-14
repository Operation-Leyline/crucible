//! Permission compatibility (docs/permissions.md §5): what breaks stored
//! grants or SDK source, and what is an authorization change.

use crucible_policy::{Config, Finding, Severity, diff};
use crucible_smithy::Model;
use crucible_smithy::ir::Service;
use serde_json::{Value, json};

fn model() -> Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/permissions.json"
    ))
    .unwrap()
}

fn lower(model: &Value) -> Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn findings(new: &Value) -> Vec<Finding> {
    diff(&lower(&model()), &lower(new), &Config::default()).findings
}

fn graph_members(model: &mut Value) -> &mut serde_json::Map<String, Value> {
    model["shapes"]["example.leyline#GraphPermission"]["members"]
        .as_object_mut()
        .unwrap()
}

#[track_caller]
fn expect(findings: &[Finding], rule: &str, shape: &str, fragment: &str, severity: Severity) {
    assert!(
        findings.iter().any(|f| f.rule == rule
            && f.shape == format!("example.leyline#{shape}")
            && f.message.contains(fragment)
            && f.severity == severity),
        "no {rule} on {shape} with {fragment:?}: {findings:#?}"
    );
}

#[test]
fn an_unchanged_model_has_no_permission_findings() {
    let found = findings(&model());
    assert!(
        !found
            .iter()
            .any(|f| f.rule.contains("permissions") || f.rule.contains("authorization")),
        "{found:#?}"
    );
}

#[test]
fn removing_a_permission_or_changing_its_value_breaks_stored_grants() {
    let mut new = model();
    graph_members(&mut new).remove("AUDIT");
    let found = findings(&new);
    expect(
        &found,
        "compatibility.permissions",
        "Graph$AUDIT",
        "the permission \"graph:audit\" (`AUDIT`) was removed",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.sdk",
        "Graph$AUDIT",
        "the permission constant `AUDIT` of `Graph` was removed",
        Severity::Error,
    );

    let mut new = model();
    graph_members(&mut new)["AUDIT"]["traits"]["smithy.api#enumValue"] = json!("graph:inspect");
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$AUDIT",
        "`AUDIT` changed its value from \"graph:audit\" to \"graph:inspect\"",
        Severity::Error,
    );
}

#[test]
fn renaming_through_an_alias_is_additive() {
    let mut new = model();
    let members = graph_members(&mut new);
    members.insert(
        "INSPECT".into(),
        json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "graph:inspect", "crucible.auth#permissionOnly": {}}}),
    );
    members["AUDIT"]["traits"]["crucible.auth#aliasOf"] = json!("INSPECT");
    members["AUDIT"]["traits"]["smithy.api#deprecated"] = json!({});
    let found = findings(&new);
    assert!(
        !found
            .iter()
            .any(|f| f.rule == "compatibility.permissions" || f.shape.contains("Graph$")),
        "{found:#?}"
    );
}

#[test]
fn removing_an_alias_breaks_stored_grants() {
    let mut new = model();
    graph_members(&mut new).remove("VIEW");
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$VIEW",
        "the alias \"graph:view\" was removed",
        Severity::Error,
    );
}

#[test]
fn a_changed_authorization_name_breaks_stored_grants() {
    let mut new = model();
    new["shapes"]["example.leyline#Organization"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("crucible.auth#authorizationName");
    new["shapes"]["example.leyline#ResourceKind"]["members"]["ORG"]["traits"]["smithy.api#enumValue"] =
        json!("organization");
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Organization",
        "the authorization name changed from \"org\" to \"organization\"; every stored grant and audit record names \"org\": keep it with `@authorizationName(\"org\")`",
        Severity::Error,
    );
}

#[test]
fn moving_a_permission_to_internal_breaks_sdk_source() {
    let mut new = model();
    graph_members(&mut new)["AUDIT"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("crucible.auth#permissionOnly");
    graph_members(&mut new)["AUDIT"]["traits"]["smithy.api#internal"] = json!({});
    let found = findings(&new);
    expect(
        &found,
        "compatibility.sdk",
        "Graph$AUDIT",
        "`AUDIT` became internal",
        Severity::Error,
    );
    assert!(
        !found.iter().any(|f| f.rule == "compatibility.permissions"),
        "{found:#?}"
    );
}

#[test]
fn adding_a_requirement_is_an_authorization_change_reported_as_a_warning() {
    let mut new = model();
    let restore = new["shapes"]["example.leyline#RestoreBackup"]["traits"]
        .as_object_mut()
        .unwrap();
    restore.remove("crucible.auth#requiresPending");
    restore.insert(
        "crucible.auth#requires".into(),
        json!([{"permission": "history:read", "resource": "example.leyline#History"}]),
    );
    new["shapes"]["example.leyline#GetGraph"]["traits"]["crucible.auth#requires"]
        .as_array_mut()
        .unwrap()
        .push(json!({"permission": "graph:write", "resource": "example.leyline#Graph", "bind": {"graphId": "$graph"}}));
    let found = findings(&new);
    expect(
        &found,
        "compatibility.authorization",
        "RestoreBackup",
        "an authorization change: `RestoreBackup` now requires \"history:read\" on `History` (it was `requires pending`)",
        Severity::Warning,
    );
    expect(
        &found,
        "compatibility.authorization",
        "GetGraph",
        "`GetGraph` now requires \"graph:write\" on `Graph`;",
        Severity::Warning,
    );
    // Reported, not refused.
    assert!(
        !found
            .iter()
            .any(|f| f.rule == "compatibility.authorization" && f.severity == Severity::Error)
    );
}

#[test]
fn the_new_rules_can_be_excepted() {
    let config = Config::parse(
        "schema_version = 1\n[[exceptions]]\nrule = 'compatibility.permissions'\nshape = 'example.leyline#Graph$VIEW'\nreason = 'no grant stores it'\n",
    )
    .unwrap();
    let mut new = model();
    graph_members(&mut new).remove("VIEW");
    let report = diff(&lower(&model()), &lower(&new), &config);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "compatibility.permissions" && f.exception.is_some())
    );
    assert!(Config::parse("schema_version = 1\n[[exceptions]]\nrule = 'compatibility.authorization'\nshape = 'example.leyline#GetGraph'\nreason = 'closing a hole'\n").is_ok());
}
