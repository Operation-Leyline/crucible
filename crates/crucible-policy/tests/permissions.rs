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

fn value(model: &mut Value, symbol: &str, value: &str) {
    graph_members(model)[symbol]["traits"]["smithy.api#enumValue"] = json!(value);
}

#[test]
fn swapping_two_values_breaks_stored_grants() {
    // `READ` now names "graph:write" and `WRITE` "graph:read": every
    // stored read grant would satisfy a write check.
    let mut new = model();
    value(&mut new, "READ", "graph:write");
    value(&mut new, "WRITE", "graph:read");
    let found = findings(&new);
    expect(
        &found,
        "compatibility.permissions",
        "Graph$READ",
        "`READ` changed its value from \"graph:read\" to \"graph:write\"",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.permissions",
        "Graph$WRITE",
        "`WRITE` changed its value from \"graph:write\" to \"graph:read\"",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.permissions",
        "Graph$READ",
        "grants stored under \"graph:read\" satisfied `READ` and now satisfy `WRITE`",
        Severity::Error,
    );
    // A Smithy requirement names the value, so `GetGraph` still checks
    // "graph:read": no authorization change, only the broken grants.
    assert!(
        !found
            .iter()
            .any(|f| f.rule == "compatibility.authorization"),
        "{found:#?}"
    );
}

#[test]
fn a_new_value_with_the_old_one_kept_as_its_alias_is_additive() {
    // `AUDIT` now names "graph:inspect", and a deprecated alias of `AUDIT`
    // keeps "graph:audit": every stored grant still satisfies `AUDIT`.
    let mut new = model();
    value(&mut new, "AUDIT", "graph:inspect");
    graph_members(&mut new).insert(
        "OLD_AUDIT".into(),
        json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "graph:audit", "crucible.auth#aliasOf": "AUDIT", "crucible.auth#permissionOnly": {}, "smithy.api#deprecated": {}}}),
    );
    let found = findings(&new);
    assert!(
        !found
            .iter()
            .any(|f| f.rule == "compatibility.permissions"
                || f.rule == "compatibility.authorization"),
        "{found:#?}"
    );
    // The old value kept by an alias of another permission is still a
    // reassignment.
    let traits = &mut graph_members(&mut new)["OLD_AUDIT"]["traits"];
    traits["crucible.auth#aliasOf"] = json!("READ");
    traits
        .as_object_mut()
        .unwrap()
        .remove("crucible.auth#permissionOnly");
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$AUDIT",
        "`AUDIT` changed its value from \"graph:audit\" to \"graph:inspect\"",
        Severity::Error,
    );
}

#[test]
fn reassigning_a_value_to_another_permission_breaks_stored_grants() {
    // `AUDIT` is gone and its value is now an alias of `READ`.
    let mut new = model();
    let members = graph_members(&mut new);
    members.remove("AUDIT");
    members.insert(
        "OLD_AUDIT".into(),
        json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "graph:audit", "crucible.auth#aliasOf": "READ", "smithy.api#deprecated": {}}}),
    );
    let found = findings(&new);
    expect(
        &found,
        "compatibility.permissions",
        "Graph$AUDIT",
        "grants stored under \"graph:audit\" satisfied `AUDIT` and now satisfy `READ`, another released permission",
        Severity::Error,
    );
    // An alias that now satisfies another released permission.
    let mut new = model();
    graph_members(&mut new)["VIEW"]["traits"]["crucible.auth#aliasOf"] = json!("WRITE");
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$VIEW",
        "grants stored under \"graph:view\" satisfied `READ` and now satisfy `WRITE`",
        Severity::Error,
    );
}

#[test]
fn a_new_primary_takes_over_one_released_permission_only() {
    let mut new = model();
    let members = graph_members(&mut new);
    members.insert(
        "INSPECT".into(),
        json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "graph:inspect", "crucible.auth#permissionOnly": {}}}),
    );
    // A renamed alias symbol is the same takeover.
    let audit = members.remove("AUDIT").unwrap();
    members.insert("OLD_AUDIT".into(), audit);
    members["OLD_AUDIT"]["traits"]["crucible.auth#aliasOf"] = json!("INSPECT");
    members["OLD_AUDIT"]["traits"]["smithy.api#deprecated"] = json!({});
    let found = findings(&new);
    assert!(
        !found.iter().any(|f| f.rule == "compatibility.permissions"),
        "{found:#?}"
    );
    // Folding a second released permission into it merges the two.
    let export = json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "graph:export", "crucible.auth#permissionOnly": {}}});
    let mut old = model();
    graph_members(&mut old).insert("EXPORT".into(), export.clone());
    let mut merged = new.clone();
    let members = graph_members(&mut merged);
    members.insert("EXPORT".into(), export);
    members["EXPORT"]["traits"]["crucible.auth#aliasOf"] = json!("INSPECT");
    members["EXPORT"]["traits"]["smithy.api#deprecated"] = json!({});
    expect(
        &diff(&lower(&old), &lower(&merged), &Config::default()).findings,
        "compatibility.permissions",
        "Graph$INSPECT",
        "the new permission `INSPECT` takes over the values of `AUDIT`, `EXPORT`",
        Severity::Error,
    );
}

/// The fixture as released before the service declared any permission:
/// no `crucible.auth` permission trait and no vocabulary enum.
fn without_permissions() -> Value {
    let mut old = model();
    let shapes = old["shapes"].as_object_mut().unwrap();
    shapes.retain(|_, shape| {
        let traits = &shape["traits"];
        traits.get("crucible.auth#permissionsOf").is_none()
            && traits.get("crucible.auth#permissionTypes").is_none()
    });
    for shape in shapes.values_mut() {
        if let Some(traits) = shape.get_mut("traits").and_then(Value::as_object_mut) {
            traits.retain(|name, _| {
                !matches!(
                    name.as_str(),
                    "crucible.auth#requires"
                        | "crucible.auth#requiresNone"
                        | "crucible.auth#requiresPending"
                        | "crucible.auth#authorizationName"
                )
            });
        }
    }
    old
}

#[test]
fn declaring_requirements_on_a_released_operation_is_not_a_new_error() {
    // Pending, then a requirement: NotFoundException comes with it.
    let mut new = model();
    let restore = new["shapes"]["example.leyline#RestoreBackup"]["traits"]
        .as_object_mut()
        .unwrap();
    restore.remove("crucible.auth#requiresPending");
    restore.insert(
        "crucible.auth#requires".into(),
        json!([{"permission": "history:read", "resource": "example.leyline#History"}]),
    );
    let found = findings(&new);
    expect(
        &found,
        "compatibility.authorization",
        "RestoreBackup",
        "(it was `requires pending`)",
        Severity::Warning,
    );
    let on_restore: Vec<&Finding> = found
        .iter()
        .filter(|f| f.shape == "example.leyline#RestoreBackup")
        .collect();
    assert_eq!(on_restore.len(), 1, "{on_restore:#?}");
    // A release from before any permission: every operation that now has
    // a requirement gains the framework NotFound, and none of that is a
    // new error or a changed SDK signature.
    let report = diff(
        &lower(&without_permissions()),
        &lower(&model()),
        &Config::default(),
    );
    let broken: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|f| f.rule == "compatibility.error" || f.rule == "compatibility.sdk")
        .collect();
    assert!(broken.is_empty(), "{broken:#?}");
    expect(
        &report.findings,
        "compatibility.authorization",
        "GetGraph",
        "`GetGraph` now requires \"graph:read\" on `Graph` (it declared none)",
        Severity::Warning,
    );
    // A NotFound the model adds itself is still a new error.
    let mut new = model();
    new["shapes"]["example.leyline#GetUser"]["errors"] =
        json!([{"target": "crucible.errors#NotFoundException"}]);
    let found = findings(&new);
    assert!(
        found
            .iter()
            .any(|f| f.rule == "compatibility.error" && f.shape == "example.leyline#GetUser"),
        "{found:#?}"
    );
}
