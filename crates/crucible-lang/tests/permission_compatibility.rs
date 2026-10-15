//! Permission compatibility (docs/permissions.md §5) over `.cru` models:
//! what breaks stored grants or SDK source, and what is an authorization
//! change. The rules are `crucible-policy`'s, applied to the IR.

use crucible_ir::ir::Service;
use crucible_lang::{Package, SourceFile, compile};
use crucible_policy::{Config, Finding, Severity, diff};

const BASE: &str = r#"
service Leyline {
    version "2026-10-01"
    protocol json
    auth bearer
    operations [GetUser]
    resources [Organization]
}

readonly operation GetUser {
    http GET "/user"
    requires none
    output { subject: string }
}

string OrganizationId { pattern "^org_[a-z0-9]+$" }
string GraphId { pattern "^g_[a-z0-9]+$" }
string HistoryId { pattern "^h_[a-z0-9]+$" }

@authorizationName("org")
resource Organization {
    id organizationId: OrganizationId
    internal permission read = "organization:read"
    @notGrantable
    permission admin = "organization:admin"
}

readonly operation GetOrganization reads Organization {
    http GET "/orgs/{organizationId}"
    requires read
    input { $organizationId }
}

resource Graph in Organization {
    id graphId: GraphId
    permission read = "graph:read"
    permission write = "graph:write"
    @permissionOnly
    permission audit = "graph:audit"
    @deprecated @aliasOf(read)
    permission view = "graph:view"
}

readonly operation GetGraph reads Graph {
    http GET "/orgs/{organizationId}/graphs/{graphId}"
    requires read
    input {
        $organizationId
        $graphId
    }
}

operation CreateGraph creates Graph {
    http POST "/orgs/{organizationId}/graphs" -> 201
    requires write on Graph(*)
    input {
        $organizationId
        name: string
    }
}

resource History in Graph {
    id historyId: HistoryId
    permission read = "history:read"
    @permissionOnly
    permission restore = "history:restore"
}

operation RestoreBackup on History {
    http POST "/orgs/{organizationId}/graphs/{graphId}/histories/{historyId}/restore"
    requires pending because "guard::allow in the handler until the history module moves."
    input {
        $organizationId
        $graphId
        $historyId
    }
}
"#;

fn lower(text: &str) -> Service {
    let mut package = Package::new(
        "example.leyline",
        vec![SourceFile {
            path: "leyline.cru".into(),
            text: text.to_owned(),
        }],
    );
    package.imports = vec!["crucible.errors".to_owned()];
    let compiled = compile(&package, None);
    compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()))
}

fn findings_from(old: &str, new: &str) -> Vec<Finding> {
    diff(&lower(old), &lower(new), &Config::default()).findings
}

fn findings(new: &str) -> Vec<Finding> {
    findings_from(BASE, new)
}

/// `BASE` with `from` replaced by `to`, which must occur.
fn edit(model: &str, from: &str, to: &str) -> String {
    assert!(model.contains(from), "no {from:?} in the model");
    model.replacen(from, to, 1)
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

#[track_caller]
fn none_of(findings: &[Finding], rules: &[&str]) {
    let found: Vec<&Finding> = findings
        .iter()
        .filter(|f| rules.contains(&f.rule.as_str()))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

const AUDIT: &str = "    @permissionOnly\n    permission audit = \"graph:audit\"\n";

#[test]
fn an_unchanged_model_has_no_permission_findings() {
    let found = findings(BASE);
    assert!(
        !found
            .iter()
            .any(|f| f.rule.contains("permissions") || f.rule.contains("authorization")),
        "{found:#?}"
    );
}

#[test]
fn removing_a_permission_or_changing_its_value_breaks_stored_grants() {
    let found = findings(&edit(BASE, AUDIT, ""));
    expect(
        &found,
        "compatibility.permissions",
        "Graph$audit",
        "the permission \"graph:audit\" (`audit`) was removed",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.sdk",
        "Graph$audit",
        "the permission constant `audit` of `Graph` was removed",
        Severity::Error,
    );
    expect(
        &findings(&edit(BASE, "\"graph:audit\"", "\"graph:inspect\"")),
        "compatibility.permissions",
        "Graph$audit",
        "`audit` changed its value from \"graph:audit\" to \"graph:inspect\"",
        Severity::Error,
    );
}

#[test]
fn renaming_through_an_alias_is_additive() {
    let new = edit(
        BASE,
        AUDIT,
        "    @permissionOnly\n    permission inspect = \"graph:inspect\"\n    @deprecated @aliasOf(inspect) @permissionOnly\n    permission audit = \"graph:audit\"\n",
    );
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
    let new = edit(
        BASE,
        "    @deprecated @aliasOf(read)\n    permission view = \"graph:view\"\n",
        "",
    );
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$view",
        "the alias \"graph:view\" was removed",
        Severity::Error,
    );
}

#[test]
fn a_changed_authorization_name_breaks_stored_grants() {
    expect(
        &findings(&edit(BASE, "@authorizationName(\"org\")\n", "")),
        "compatibility.permissions",
        "Organization",
        "the authorization name changed from \"org\" to \"organization\"; every stored grant and audit record names \"org\": keep it with `@authorizationName(\"org\")`",
        Severity::Error,
    );
}

#[test]
fn moving_a_permission_to_internal_breaks_sdk_source() {
    let found = findings(&edit(
        BASE,
        AUDIT,
        "    internal permission audit = \"graph:audit\"\n",
    ));
    expect(
        &found,
        "compatibility.sdk",
        "Graph$audit",
        "`audit` became internal",
        Severity::Error,
    );
    none_of(&found, &["compatibility.permissions"]);
}

#[test]
fn adding_a_requirement_is_an_authorization_change_reported_as_a_warning() {
    let new = edit(
        BASE,
        "    requires pending because \"guard::allow in the handler until the history module moves.\"\n",
        "    requires read\n",
    );
    let new = edit(
        &new,
        "    requires read\n    input {\n        $organizationId\n        $graphId\n    }",
        "    requires read\n    requires write\n    input {\n        $organizationId\n        $graphId\n    }",
    );
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
        "schema_version = 1\n[[exceptions]]\nrule = 'compatibility.permissions'\nshape = 'example.leyline#Graph$view'\nreason = 'no grant stores it'\n",
    )
    .unwrap();
    let new = edit(
        BASE,
        "    @deprecated @aliasOf(read)\n    permission view = \"graph:view\"\n",
        "",
    );
    let report = diff(&lower(BASE), &lower(&new), &config);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "compatibility.permissions" && f.exception.is_some())
    );
    assert!(Config::parse("schema_version = 1\n[[exceptions]]\nrule = 'compatibility.authorization'\nshape = 'example.leyline#GetGraph'\nreason = 'closing a hole'\n").is_ok());
}

#[test]
fn swapping_two_values_breaks_stored_grants() {
    // `read` now names "graph:write" and `write` "graph:read": every stored
    // read grant would satisfy a write check.
    let new = edit(BASE, "\"graph:read\"", "\"graph:tmp\"");
    let new = edit(&new, "\"graph:write\"", "\"graph:read\"");
    let new = edit(&new, "\"graph:tmp\"", "\"graph:write\"");
    let found = findings(&new);
    expect(
        &found,
        "compatibility.permissions",
        "Graph$read",
        "`read` changed its value from \"graph:read\" to \"graph:write\"",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.permissions",
        "Graph$write",
        "`write` changed its value from \"graph:write\" to \"graph:read\"",
        Severity::Error,
    );
    expect(
        &found,
        "compatibility.permissions",
        "Graph$read",
        "grants stored under \"graph:read\" satisfied `read` and now satisfy `write`",
        Severity::Error,
    );
}

#[test]
fn reassigning_a_value_to_another_permission_breaks_stored_grants() {
    // `audit` is gone and its value is now an alias of `read`.
    let new = edit(
        BASE,
        AUDIT,
        "    @deprecated @aliasOf(read)\n    permission oldAudit = \"graph:audit\"\n",
    );
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$audit",
        "grants stored under \"graph:audit\" satisfied `audit` and now satisfy `read`, another released permission",
        Severity::Error,
    );
    // An alias that now satisfies another released permission.
    let new = edit(
        BASE,
        "@aliasOf(read)\n    permission view",
        "@aliasOf(write)\n    permission view",
    );
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$view",
        "grants stored under \"graph:view\" satisfied `read` and now satisfy `write`",
        Severity::Error,
    );
}

#[test]
fn a_new_value_with_the_old_one_kept_as_its_alias_is_additive() {
    // `audit` now names "graph:inspect", and a deprecated alias of `audit`
    // keeps "graph:audit": every stored grant still satisfies `audit`.
    let new = edit(
        BASE,
        AUDIT,
        "    @permissionOnly\n    permission audit = \"graph:inspect\"\n    @deprecated @aliasOf(audit) @permissionOnly\n    permission oldAudit = \"graph:audit\"\n",
    );
    none_of(
        &findings(&new),
        &["compatibility.permissions", "compatibility.authorization"],
    );
    // The old value kept by an alias of another permission is still a
    // reassignment.
    let new = edit(
        BASE,
        AUDIT,
        "    @permissionOnly\n    permission audit = \"graph:inspect\"\n    @deprecated @aliasOf(read)\n    permission oldAudit = \"graph:audit\"\n",
    );
    expect(
        &findings(&new),
        "compatibility.permissions",
        "Graph$audit",
        "`audit` changed its value from \"graph:audit\" to \"graph:inspect\"",
        Severity::Error,
    );
}

#[test]
fn a_new_primary_takes_over_one_released_permission_only() {
    // A renamed alias symbol is the same takeover.
    let renamed = "    @permissionOnly\n    permission inspect = \"graph:inspect\"\n    @deprecated @aliasOf(inspect) @permissionOnly\n    permission oldAudit = \"graph:audit\"\n";
    none_of(
        &findings(&edit(BASE, AUDIT, renamed)),
        &["compatibility.permissions"],
    );
    // Folding a second released permission into it merges the two.
    let export = "    @permissionOnly\n    permission export = \"graph:export\"\n";
    let old = edit(BASE, AUDIT, &format!("{AUDIT}{export}"));
    let merged = edit(
        BASE,
        AUDIT,
        &format!(
            "{renamed}    @deprecated @aliasOf(inspect) @permissionOnly\n    permission export = \"graph:export\"\n"
        ),
    );
    expect(
        &findings_from(&old, &merged),
        "compatibility.permissions",
        "Graph$inspect",
        "the new permission `inspect` takes over the values of `audit`, `export`",
        Severity::Error,
    );
}

#[test]
fn declaring_requirements_on_a_released_operation_is_not_a_new_error() {
    // Pending, then a requirement: NotFoundException comes with it.
    let new = edit(
        BASE,
        "    requires pending because \"guard::allow in the handler until the history module moves.\"\n",
        "    requires read\n",
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
    // A release from before any permission: every operation that now has a
    // requirement gains the framework NotFound, and none of that is a new
    // error or a changed SDK signature.
    let bare: String = BASE
        .lines()
        .filter(|line| {
            ![
                "permission ",
                "requires ",
                "@permissionOnly",
                "@notGrantable",
                "@deprecated",
                "@authorizationName",
            ]
            .iter()
            .any(|word| line.contains(word))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let found = findings_from(&bare, BASE);
    none_of(&found, &["compatibility.error", "compatibility.sdk"]);
    expect(
        &found,
        "compatibility.authorization",
        "GetGraph",
        "`GetGraph` now requires \"graph:read\" on `Graph` (it declared none)",
        Severity::Warning,
    );
    // A NotFound the model adds itself is still a new error.
    let new = edit(
        BASE,
        "    requires none\n",
        "    requires none\n    errors [NotFoundException]\n",
    );
    assert!(
        findings(&new)
            .iter()
            .any(|f| f.rule == "compatibility.error" && f.shape == "example.leyline#GetUser"),
    );
}

#[test]
fn designating_the_services_own_not_found_error_after_a_release_is_a_new_error() {
    let designated = edit(
        BASE,
        "    auth bearer
",
        "    auth bearer
    notFound Missing
",
    ) + "
error Missing {
    status 404
    message: string
}
";
    // An operation that already hid something now answers another 404.
    let found = findings(&designated);
    assert!(
        found.iter().any(|f| f.rule == "compatibility.error"
            && f.shape == "example.leyline#GetGraph"
            && f.message == "new modelled error \"Missing\" can reach old clients"),
        "{found:#?}"
    );
    // A requirement declared on a pending operation brings the designated
    // 404 with it, as it brings the framework's: an authorization change.
    let pending = edit(
        &designated,
        "    requires read
    input { $organizationId }",
        "    requires pending because \"later\"
    input { $organizationId }",
    );
    let found = findings_from(&pending, &designated);
    expect(
        &found,
        "compatibility.authorization",
        "GetOrganization",
        "(it was `requires pending`)",
        Severity::Warning,
    );
    none_of(&found, &["compatibility.error", "compatibility.sdk"]);
}

/// An SDK release plan compares the models its audience sees: a clause on
/// an internal permission is left out of the client view, but adding one is
/// still an authorization change there.
#[test]
fn an_internal_requirement_added_is_an_authorization_change_in_the_client_view() {
    let new = edit(
        BASE,
        "    requires read\n    input {\n        $organizationId\n        $graphId\n    }",
        "    requires read\n    requires read on Organization\n    input {\n        $organizationId\n        $graphId\n    }",
    );
    let external =
        |model: &str| crucible_ir::project(&lower(model), crucible_ir::Audience::External).unwrap();
    let found = diff(&external(BASE), &external(&new), &Config::default()).findings;
    expect(
        &found,
        "compatibility.authorization",
        "GetGraph",
        "`GetGraph` now requires permissions internal to the service; callers without them are refused",
        Severity::Warning,
    );
    // GetOrganization's requirements were internal before and still are.
    assert!(
        !found.iter().any(
            |f| f.rule == "compatibility.authorization" && f.shape.ends_with("GetOrganization")
        ),
        "{found:#?}"
    );
    // A permission that becomes internal changes what the client view
    // shows, not what a caller must hold: no authorization change there.
    let hidden = edit(
        BASE,
        "    permission write = \"graph:write\"",
        "    internal permission write = \"graph:write\"",
    );
    let found = diff(&external(BASE), &external(&hidden), &Config::default()).findings;
    assert!(
        !found
            .iter()
            .any(|f| f.rule == "compatibility.authorization"),
        "{found:#?}"
    );
}
