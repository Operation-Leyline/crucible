//! Role compatibility (docs/roles.md §6) over `.cru` models: what orphans
//! stored bindings, what refuses holders what they had, and what widens a
//! role. The rules are `crucible-policy`'s, applied to the IR; the guard
//! tests at the end declare them in a migration.

use std::path::{Path, PathBuf};

use crucible_ir::ir::Service;
use crucible_lang::{Guard, Package, SourceFile, compile, guard, load_package};
use crucible_policy::{Config, Finding, Severity, diff};

const BASE: &str = r#"
service Atlas {
    version "v1"
    protocol json
    auth bearer
    notFound Missing
    resources [Organization]
    principals PrincipalType
    scopes [organization, allGraphs, graph]
    roles [owner, graphReader, graphWriter, graphAdmin]
}

error Missing {
    status 404
    message: string
}

enum PrincipalType {
    USER = "user"
    API_KEY = "apiKey"
}

string OrganizationId { pattern "^org_[a-z0-9]+$" }
string GraphId { pattern "^g_[a-z0-9]+$" }
string HistoryId { pattern "^h_[a-z0-9]+$" }

resource Organization {
    id organizationId: OrganizationId
    permission read = "organization:read"
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
    permission delete = "graph:delete"
}

readonly operation GetGraph reads Graph {
    http GET "/orgs/{organizationId}/graphs/{graphId}"
    requires read
    input {
        $organizationId
        $graphId
    }
}

operation UpdateGraph updates Graph {
    http PATCH "/orgs/{organizationId}/graphs/{graphId}"
    requires write
    input {
        $organizationId
        $graphId
        name?: string
    }
}

idempotent operation DeleteGraph deletes Graph {
    http DELETE "/orgs/{organizationId}/graphs/{graphId}"
    requires delete
    input {
        $organizationId
        $graphId
    }
}

resource History in Graph {
    id historyId: HistoryId
    permission read = "history:read"
}

readonly operation GetHistory reads History {
    http GET "/orgs/{organizationId}/graphs/{graphId}/histories/{historyId}"
    requires read
    input {
        $organizationId
        $graphId
        $historyId
    }
}

/// The organization itself.
scope organization = "organization" on Organization
/// Every graph in the organization.
scope allGraphs = "allGraphs" on Graph(*)
/// One graph.
scope graph = "graph" on Graph

/// Everything in the organization.
role owner = "owner" {
    at [organization]
    holders [USER]
    grants Organization [*]
    grants Graph [*]
    grants History [*]
}

/// Read graph data.
role graphReader = "graphReader" { at [allGraphs, graph]; grants Graph [read] }

/// Read and write graph data.
role graphWriter = "graphWriter" { at [allGraphs, graph]; includes [graphReader]; grants Graph [write] }

/// Read, write and delete graphs.
role graphAdmin = "graphAdmin" {
    at [allGraphs, graph]
    includes [graphWriter]
    grants Graph [delete]
    /// The graph's histories, while the graph is live.
    grants History [read] by service
}
"#;

/// The rules this file is about.
const RULES: &[&str] = &[
    "compatibility.role",
    "compatibility.scope",
    "compatibility.role-widened",
];

fn lower(text: &str) -> Service {
    let mut package = Package::new(
        "example.atlas",
        vec![SourceFile {
            path: "atlas.cru".into(),
            text: text.to_owned(),
        }],
    );
    package.imports = vec!["crucible.errors".to_owned()];
    let compiled = compile(&package, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
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

/// `model` with `from` replaced by `to`, which must occur.
fn edit(model: &str, from: &str, to: &str) -> String {
    assert!(model.contains(from), "no {from:?} in the model");
    model.replacen(from, to, 1)
}

/// The findings at `shape` under `rule`, as `severity: message`.
fn at(findings: &[Finding], rule: &str, shape: &str) -> Vec<String> {
    findings
        .iter()
        .filter(|f| f.rule == rule && f.shape == format!("example.atlas#{shape}"))
        .map(|f| {
            let severity = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Off => "off",
            };
            format!("{severity}: {}", f.message)
        })
        .collect()
}

#[track_caller]
fn none_of(findings: &[Finding], rules: &[&str]) {
    let found: Vec<&Finding> = findings
        .iter()
        .filter(|f| rules.contains(&f.rule.as_str()))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// The role findings, and the SDK findings at a role or scope.
fn role_findings(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|f| {
            RULES.contains(&f.rule.as_str())
                || f.rule == "compatibility.sdk"
                    && [
                        "#owner",
                        "#graphReader",
                        "#graphWriter",
                        "#graphAdmin",
                        "#organization",
                        "#allGraphs",
                        "#graph",
                    ]
                    .iter()
                    .any(|site| f.shape.ends_with(site))
        })
        .collect()
}

#[test]
fn an_unchanged_model_has_no_role_findings() {
    let found = findings(BASE);
    assert!(role_findings(&found).is_empty(), "{found:#?}");
}

#[test]
fn removing_a_role_orphans_its_bindings() {
    let new = edit(BASE, ", graphAdmin]", "]");
    let start = new.find("/// Read, write and delete graphs.").unwrap();
    let found = findings(&new[..start]);
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: the role \"graphAdmin\" (`graphAdmin`) was removed; bindings stored under it no longer match: keep it, and mark it `@deprecated`"
        ]
    );
    assert_eq!(
        at(&found, "compatibility.sdk", "graphAdmin"),
        ["error: the role constant `graphAdmin` was removed from client SDKs"]
    );
}

#[test]
fn changing_a_roles_value_orphans_its_bindings() {
    let found = findings(&edit(
        BASE,
        "role graphReader = \"graphReader\"",
        "role graphReader = \"reader\"",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "error: `graphReader` changed its value from \"graphReader\" to \"reader\"; bindings stored under the old value no longer match it: keep the value"
        ]
    );
    // The constant keeps its name and sends another value.
    assert_eq!(
        at(&found, "compatibility.sdk", "graphReader"),
        [
            "error: the role constant `graphReader` now sends \"reader\", where it sent \"graphReader\": a client that still names it asks for another role"
        ]
    );
}

#[test]
fn swapping_two_roles_with_their_bodies_changes_what_client_constants_send() {
    // Every value keeps its role, so stored bindings mean what they did;
    // a client's `graphReader` now asks for what was `graphWriter`.
    let swapped = edit(
        BASE,
        "role graphReader = \"graphReader\" { at [allGraphs, graph]; grants Graph [read] }",
        "role graphWriter = \"graphReader\" { at [allGraphs, graph]; grants Graph [read] }",
    );
    let swapped = edit(
        &swapped,
        "role graphWriter = \"graphWriter\" { at [allGraphs, graph]; includes [graphReader]; grants Graph [write] }",
        "role graphReader = \"graphWriter\" { at [allGraphs, graph]; includes [graphWriter]; grants Graph [write] }",
    );
    let swapped = edit(
        &swapped,
        "    includes [graphWriter]\n",
        "    includes [graphReader]\n",
    );
    let found = findings(&swapped);
    none_of(&found, RULES);
    assert_eq!(
        at(&found, "compatibility.sdk", "graphReader"),
        [
            "error: the role constant `graphReader` now sends \"graphWriter\", where it sent \"graphReader\": a client that still names it asks for another role"
        ]
    );
    assert_eq!(
        at(&found, "compatibility.sdk", "graphWriter"),
        [
            "error: the role constant `graphWriter` now sends \"graphReader\", where it sent \"graphWriter\": a client that still names it asks for another role"
        ]
    );
}

#[test]
fn swapping_two_scopes_changes_what_client_constants_send() {
    // `graph` now sends the value of the scope on every graph.
    let swapped = edit(
        BASE,
        "scope allGraphs = \"allGraphs\" on Graph(*)",
        "scope graph = \"allGraphs\" on Graph(*)",
    );
    let swapped = edit(
        &swapped,
        "scope graph = \"graph\" on Graph\n",
        "scope allGraphs = \"graph\" on Graph\n",
    );
    let found = findings(&swapped);
    none_of(&found, RULES);
    assert_eq!(
        at(&found, "compatibility.sdk", "graph"),
        [
            "error: the scope constant `graph` now sends \"allGraphs\", where it sent \"graph\": a client that still names it asks for another scope"
        ]
    );
    assert_eq!(at(&found, "compatibility.sdk", "allGraphs").len(), 1);
}

#[test]
fn removing_a_scope_orphans_its_bindings_and_is_reported_once() {
    let new = edit(BASE, ", allGraphs, graph]", ", allGraphs]");
    let new = edit(
        &new,
        "/// One graph.\nscope graph = \"graph\" on Graph\n",
        "",
    );
    let new = new.replace("at [allGraphs, graph]", "at [allGraphs]");
    let found = findings(&new);
    assert_eq!(
        at(&found, "compatibility.scope", "graph"),
        [
            "error: the scope \"graph\" (`graph`) was removed; bindings stored at it no longer match: keep it, and mark it `@deprecated`"
        ]
    );
    assert_eq!(
        at(&found, "compatibility.sdk", "graph"),
        ["error: the scope constant `graph` was removed from client SDKs"]
    );
    // Not again for each role bound at it.
    none_of(&found, &["compatibility.role"]);
}

#[test]
fn changing_a_scopes_value_orphans_its_bindings() {
    let found = findings(&edit(
        BASE,
        "scope allGraphs = \"allGraphs\"",
        "scope allGraphs = \"graphs\"",
    ));
    assert_eq!(
        at(&found, "compatibility.scope", "allGraphs"),
        [
            "error: `allGraphs` changed its value from \"allGraphs\" to \"graphs\"; bindings stored at the old value no longer match it: keep the value"
        ]
    );
}

#[test]
fn moving_a_scope_to_another_resource_orphans_its_bindings() {
    let found = findings(&edit(
        BASE,
        "scope graph = \"graph\" on Graph\n",
        "scope graph = \"graph\" on Organization\n",
    ));
    assert_eq!(
        at(&found, "compatibility.scope", "graph"),
        [
            "error: the scope \"graph\" (`graph`) is now on `Organization`, where it was on `Graph`; bindings stored at it name an instance of what it was on"
        ]
    );
}

#[test]
fn moving_a_scopes_resource_under_another_parent_orphans_its_bindings() {
    // `Workspace` names its instances by an identifier spelled as
    // `Organization`'s, so every route and identifier stays as it was: only
    // what a stored instance's `organizationId` means changes.
    let new = edit(
        BASE,
        "    resources [Organization]\n",
        "    resources [Organization, Workspace]\n",
    );
    let new = edit(
        &new,
        "resource Graph in Organization {",
        "resource Workspace {\n    id organizationId: OrganizationId\n}\n\nresource Graph in Workspace {",
    );
    // The owner's binding at an organization no longer reaches graphs.
    let new = edit(&new, "    grants Graph [*]\n    grants History [*]\n", "");
    let found = findings(&new);
    assert_eq!(
        at(&found, "compatibility.scope", "graph"),
        [
            "error: the scope \"graph\" (`graph`) now names its instance as `Workspace(organizationId)`/`graph(graphId)`, where it named it as `organization(organizationId)`/`graph(graphId)`; bindings stored at it carry the old path"
        ]
    );
    assert_eq!(
        at(&found, "compatibility.scope", "allGraphs"),
        [
            "error: the scope \"allGraphs\" (`allGraphs`) now names its instance as `Workspace(organizationId)`/`graph(*)`, where it named it as `organization(organizationId)`/`graph(*)`; bindings stored at it carry the old path"
        ]
    );
    assert!(at(&found, "compatibility.scope", "organization").is_empty());
    // No route changed, so nothing else would have told.
    none_of(&found, &["compatibility.http"]);
}

#[test]
fn a_resource_moved_out_from_under_a_scope_narrows_the_roles_bound_there() {
    // The owner is bound at an organization and at the service, so its
    // graph grants stay reachable when `Graph` leaves `Organization`.
    let wide = edit(
        BASE,
        "    scopes [organization, allGraphs, graph]\n",
        "    scopes [organization, allGraphs, graph, everything]\n",
    );
    let wide = edit(
        &wide,
        "/// Every graph in the organization.\n",
        "/// The whole service.\nscope everything = \"everything\" on service\n/// Every graph in the organization.\n",
    );
    let wide = edit(
        &wide,
        "    at [organization]\n",
        "    at [organization, everything]\n",
    );
    let moved = edit(
        &wide,
        "    resources [Organization]\n",
        "    resources [Organization, Workspace]\n",
    );
    let moved = edit(
        &moved,
        "resource Graph in Organization {",
        "resource Workspace {\n    id organizationId: OrganizationId\n}\n\nresource Graph in Workspace {",
    );
    let found = findings_from(&wide, &moved);
    // The scopes and grants read the same; what a binding at an
    // organization covers does not.
    assert_eq!(
        at(&found, "compatibility.role", "owner"),
        [
            "error: `owner` bound at \"organization\" no longer covers `Graph`, where it grants \"graph:read\", \"graph:write\", \"graph:delete\"; its holders bound there are refused what they had",
            "error: `owner` bound at \"organization\" no longer covers `History`, where it grants \"history:read\"; its holders bound there are refused what they had",
        ]
    );
    // A migration declares it with `narrow role owner`.
    assert!(crucible_policy::role_narrowed(
        &lower(&wide),
        &lower(&moved),
        "owner"
    ));
    // `graph` and `allGraphs` changed their instance paths: reported at
    // them, not again for the roles bound there.
    assert_eq!(at(&found, "compatibility.scope", "graph").len(), 1);
    assert_eq!(at(&found, "compatibility.scope", "allGraphs").len(), 1);
    for role in ["graphReader", "graphWriter", "graphAdmin"] {
        assert!(at(&found, "compatibility.role", role).is_empty(), "{role}");
    }

    // The reverse move widens it.
    let found = findings_from(&moved, &wide);
    assert_eq!(
        at(&found, "compatibility.role-widened", "owner"),
        [
            "warning: `owner` bound at \"organization\" now covers `Graph`, where it grants \"graph:read\", \"graph:write\", \"graph:delete\"; its holders bound there gain it without a new binding",
            "warning: `owner` bound at \"organization\" now covers `History`, where it grants \"history:read\"; its holders bound there gain it without a new binding",
        ]
    );
    assert!(at(&found, "compatibility.role", "owner").is_empty());
}

/// `model` with `History` moved from beneath `Graph` to beneath
/// `Organization`.
fn history_under_organization(model: &str) -> String {
    let moved = edit(
        model,
        "resource History in Graph {",
        "resource History in Organization {",
    );
    let moved = edit(
        &moved,
        "\"/orgs/{organizationId}/graphs/{graphId}/histories/{historyId}\"",
        "\"/orgs/{organizationId}/histories/{historyId}\"",
    );
    edit(
        &moved,
        "        $graphId\n        $historyId\n",
        "        $historyId\n",
    )
}

#[test]
fn a_grant_the_service_decides_does_not_follow_coverage() {
    // graphAdmin, bound at graphs, grants History's read `by service`: the
    // service decides when, whatever a graph binding covers.
    let found = findings(&history_under_organization(BASE));
    assert!(
        at(&found, "compatibility.role", "graphAdmin").is_empty(),
        "{found:#?}"
    );
    assert!(at(&found, "compatibility.role-widened", "graphAdmin").is_empty());
    assert!(!crucible_policy::role_narrowed(
        &lower(BASE),
        &lower(&history_under_organization(BASE)),
        "graphAdmin"
    ));

    // The same move under a grant the model decides narrows the role at
    // the scopes that covered History through Graph.
    let decided = edit(
        BASE,
        "    at [allGraphs, graph]\n    includes [graphWriter]\n",
        "    at [allGraphs, graph, organization]\n    includes [graphWriter]\n",
    );
    let decided = edit(
        &decided,
        "    grants History [read] by service\n",
        "    grants History [read]\n",
    );
    let found = findings_from(&decided, &history_under_organization(&decided));
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` bound at \"allGraphs\" no longer covers `History`, where it grants \"history:read\"; its holders bound there are refused what they had",
            "error: `graphAdmin` bound at \"graph\" no longer covers `History`, where it grants \"history:read\"; its holders bound there are refused what they had",
        ]
    );
}

/// `BASE` with graphAdmin assigned by `assigned_by` (`""`: no `assignedBy`).
fn assigned(assigned_by: &str) -> String {
    let entry = if assigned_by.is_empty() {
        String::new()
    } else {
        format!("    assignedBy [{assigned_by}]\n")
    };
    edit(
        BASE,
        "    at [allGraphs, graph]\n    includes [graphWriter]\n",
        &format!("    at [allGraphs, graph]\n{entry}    includes [graphWriter]\n"),
    )
}

#[test]
fn more_assigners_widen_a_role_and_fewer_narrow_it() {
    let owner = assigned("owner");
    let both = assigned("owner, graphWriter");
    let anyone = assigned("");
    // Another role may assign it.
    let found = findings_from(&owner, &both);
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphAdmin"),
        [
            "warning: `graphAdmin` may now be assigned by holders of \"graphWriter\"; they can grant or revoke it without a new binding"
        ]
    );
    assert!(at(&found, "compatibility.role", "graphAdmin").is_empty());
    // One may no longer.
    let found = findings_from(&both, &owner);
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` may no longer be assigned by holders of \"graphWriter\"; they can no longer grant or revoke it"
        ]
    );
    assert!(at(&found, "compatibility.role-widened", "graphAdmin").is_empty());
    // A list where there was none: whoever else the binding operation
    // admitted loses it.
    let found = findings_from(&anyone, &owner);
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` may now be assigned only by holders of \"owner\"; others the binding operation admits can no longer grant or revoke it"
        ]
    );
    assert!(crucible_policy::role_narrowed(
        &lower(&anyone),
        &lower(&owner),
        "graphAdmin"
    ));
    // No list where there was one: the binding operation decides.
    let found = findings_from(&owner, &anyone);
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphAdmin"),
        [
            "warning: `graphAdmin` may now be assigned by whoever the binding operation admits, where only holders of \"owner\" could; they can grant or revoke it without a new binding"
        ]
    );
    assert!(at(&found, "compatibility.role", "graphAdmin").is_empty());
    // The same assigners, nothing.
    assert!(role_findings(&findings_from(&both, &both)).is_empty());
}

#[test]
fn renaming_an_identifier_a_scope_instance_carries_orphans_its_bindings() {
    let found = findings(&BASE.replace("graphId", "graphKey"));
    // Reported once, at the scope whose instance carries it; `allGraphs`
    // is named by the organization alone.
    assert_eq!(
        at(&found, "compatibility.scope", "graph"),
        [
            "error: the scope \"graph\" (`graph`) now names its instance as `organization(organizationId)`/`graph(graphKey)`, where it named it as `organization(organizationId)`/`graph(graphId)`; bindings stored at it carry the old path"
        ]
    );
    assert!(at(&found, "compatibility.scope", "allGraphs").is_empty());
    assert!(at(&found, "compatibility.scope", "organization").is_empty());
}

#[test]
fn a_parent_renamed_with_its_authorization_name_kept_keeps_every_scope() {
    let new = BASE.replace("Organization", "Org").replace(
        "resource Org {",
        "@authorizationName(\"organization\")\nresource Org {",
    );
    let found = findings(&new);
    none_of(&found, &["compatibility.scope"]);
}

#[test]
fn a_role_granting_less_refuses_its_holders() {
    let found = findings(&edit(
        BASE,
        "includes [graphReader]; grants Graph [write] }",
        "includes [graphReader] }",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphWriter"),
        [
            "error: `graphWriter` no longer grants \"graph:write\" on `Graph`; its holders are refused what they had"
        ]
    );
    // A role that includes it loses it too.
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` no longer grants \"graph:write\" on `Graph`; its holders are refused what they had"
        ]
    );
    none_of(&found, &["compatibility.role-widened"]);
}

#[test]
fn a_role_losing_a_grant_the_service_decided_refuses_its_holders() {
    let found = findings(&edit(
        BASE,
        "    /// The graph's histories, while the graph is live.\n    grants History [read] by service\n",
        "",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` no longer grants \"history:read\" on `History` (`by service`); its holders are refused what they had"
        ]
    );
}

#[test]
fn a_role_bound_at_fewer_scopes_refuses_its_holders() {
    let found = findings(&edit(
        BASE,
        "role graphReader = \"graphReader\" { at [allGraphs, graph];",
        "role graphReader = \"graphReader\" { at [graph];",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "error: `graphReader` can no longer be bound at \"allGraphs\"; its holders bound there are refused what they had"
        ]
    );
}

#[test]
fn a_role_bound_at_more_scopes_is_a_widening() {
    // No holder gains anything, but whoever may assign it can now grant it
    // at every graph of an organization.
    let found = findings_from(
        &edit(
            BASE,
            "role graphReader = \"graphReader\" { at [allGraphs, graph];",
            "role graphReader = \"graphReader\" { at [graph];",
        ),
        BASE,
    );
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphReader"),
        [
            "warning: `graphReader` may now be bound at \"allGraphs\"; whoever may assign it can grant it there without a new binding"
        ]
    );
    assert!(at(&found, "compatibility.role", "graphReader").is_empty());
    // With `assignedBy` too: an owner may now grant graph admin at every
    // graph.
    let found = findings_from(
        &assigned("owner").replace(
            "    at [allGraphs, graph]\n    assignedBy",
            "    at [graph]\n    assignedBy",
        ),
        &assigned("owner"),
    );
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphAdmin"),
        [
            "warning: `graphAdmin` may now be bound at \"allGraphs\"; whoever may assign it can grant it there without a new binding"
        ]
    );
}

/// `BASE` with a root `Workspace` that declares no permission (or `with`),
/// and a scope on one workspace and one on every graph of none: just the
/// scope on it.
fn with_workspace(with: &str) -> String {
    let model = edit(
        BASE,
        "    resources [Organization]\n",
        "    resources [Organization, Workspace]\n",
    );
    let model = edit(
        &model,
        "    scopes [organization, allGraphs, graph]\n",
        "    scopes [organization, allGraphs, graph, workspace]\n",
    );
    edit(
        &model,
        "/// The organization itself.\n",
        &format!(
            "string WorkspaceId {{ pattern \"^w_[a-z0-9]+$\" }}\n\nresource Workspace {{\n    id workspaceId: WorkspaceId\n{with}}}\n\n/// One workspace.\nscope workspace = \"workspace\" on Workspace\n/// The organization itself.\n"
        ),
    )
}

/// `BASE` with a root `Document` named by two identifiers, in `order`, and
/// a scope on one document.
fn with_document(order: &str) -> String {
    let model = edit(
        BASE,
        "    resources [Organization]\n",
        "    resources [Organization, Document]\n",
    );
    let model = edit(
        &model,
        "    scopes [organization, allGraphs, graph]\n",
        "    scopes [organization, allGraphs, graph, oneDocument]\n",
    );
    edit(
        &model,
        "/// The organization itself.\n",
        &format!(
            "string AccountId {{ pattern \"^a_[a-z0-9]+$\" }}\nstring DocumentId {{ pattern \"^d_[a-z0-9]+$\" }}\n\nresource Document {{\n{order}    permission read = \"document:read\"\n}}\n\n/// One document.\nscope oneDocument = \"document\" on Document\n/// The organization itself.\n"
        ),
    )
}

#[test]
fn reordering_a_composite_identifier_orphans_bindings_at_its_scopes() {
    // A stored binding's values are compared by position: `[A, B]` stored
    // under the old order names another document under the new one.
    let ordered = with_document("    id accountId: AccountId\n    id documentId: DocumentId\n");
    let swapped = with_document("    id documentId: DocumentId\n    id accountId: AccountId\n");
    let found = findings_from(&ordered, &swapped);
    assert_eq!(
        at(&found, "compatibility.scope", "oneDocument"),
        [
            "error: the scope \"document\" (`oneDocument`) now names its instance as `document(documentId, accountId)`, where it named it as `document(accountId, documentId)`; bindings stored at it carry the old path"
        ]
    );
    let scope_findings: Vec<&Finding> = found
        .iter()
        .filter(|f| f.rule == "compatibility.scope")
        .collect();
    assert_eq!(scope_findings.len(), 1, "{scope_findings:#?}");
    // The same order: nothing.
    none_of(&findings_from(&ordered, &ordered), &["compatibility.scope"]);
}

#[test]
fn a_resource_gaining_its_first_permission_keeps_its_scopes() {
    let bare = with_workspace("");
    let declared = with_workspace("    permission read = \"workspace:read\"\n");
    // Nothing moved: the resource gains an authorization name, not a place.
    let found = findings_from(&bare, &declared);
    none_of(&found, &["compatibility.scope"]);
    // A real move still reports.
    let moved = edit(
        &bare,
        "resource Workspace {",
        "resource Workspace in Organization {",
    );
    let moved = edit(
        &moved,
        "    resources [Organization, Workspace]\n",
        "    resources [Organization]\n",
    );
    let found = findings_from(&bare, &moved);
    assert_eq!(
        at(&found, "compatibility.scope", "workspace"),
        [
            "error: the scope \"workspace\" (`workspace`) now names its instance as `organization(organizationId)`/`Workspace(workspaceId)`, where it named it as `Workspace(workspaceId)`; bindings stored at it carry the old path"
        ]
    );
}

#[test]
fn a_role_admitting_fewer_kinds_of_principal_refuses_its_holders() {
    // Every kind, then one.
    let found = findings(&edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; holders [USER]; grants Graph [read] }",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "error: `graphReader` may now be held only by `USER`; its other holders, those of no known kind among them, are refused what they had"
        ]
    );
    // Every kind, then all of them named: a principal of no known kind
    // (`BindingSource::kind` answering none) is refused now, as the
    // runtime's `valid_for` decides.
    let found = findings(&edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; holders [USER, API_KEY]; grants Graph [read] }",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "error: `graphReader` may now be held only by `API_KEY`, `USER`; its other holders, those of no known kind among them, are refused what they had"
        ]
    );
    // A kind named, then not.
    let both = edit(
        BASE,
        "    holders [USER]\n",
        "    holders [USER, API_KEY]\n",
    );
    let found = findings_from(&both, BASE);
    assert_eq!(
        at(&found, "compatibility.role", "owner"),
        [
            "error: `owner` may no longer be held by `API_KEY`; its holders among them are refused what they had"
        ]
    );
    // Admitting more, or every kind, refuses nobody, but bindings a
    // narrowing made inert (no migration converts them) take effect again.
    let found = findings_from(BASE, &both);
    assert_eq!(
        at(&found, "compatibility.role-widened", "owner"),
        [
            "warning: `owner` may now be held by `API_KEY`; bindings of those kinds, inert until now, take effect"
        ]
    );
    assert!(at(&found, "compatibility.role", "owner").is_empty());
    let every = edit(BASE, "    holders [USER]\n", "");
    let found = findings(&every);
    assert_eq!(
        at(&found, "compatibility.role-widened", "owner"),
        [
            "warning: `owner` may now be held by every kind of principal, where only `USER` could; bindings of the other kinds, inert until now, take effect"
        ]
    );
    assert!(at(&found, "compatibility.role", "owner").is_empty());
    // A role that named none still names none: nothing.
    assert!(role_findings(&findings_from(&every, &every)).is_empty());
}

#[test]
fn a_role_granting_more_is_a_widening_reported_as_a_warning() {
    let found = findings(&edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; grants Graph [read, delete] }",
    ));
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphReader"),
        [
            "warning: `graphReader` now grants \"graph:delete\" on `Graph`; its holders gain it without a new binding"
        ]
    );
    // graphWriter includes it, and graphAdmin had it already.
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphWriter"),
        [
            "warning: `graphWriter` now grants \"graph:delete\" on `Graph`; its holders gain it without a new binding"
        ]
    );
    assert!(at(&found, "compatibility.role-widened", "graphAdmin").is_empty());
    none_of(&found, &["compatibility.role"]);
}

#[test]
fn a_star_covering_a_new_permission_widens_the_role() {
    let found = findings(&edit(
        BASE,
        "    permission delete = \"graph:delete\"\n",
        "    permission delete = \"graph:delete\"\n    permission export = \"graph:export\"\n",
    ));
    assert_eq!(
        at(&found, "compatibility.role-widened", "owner"),
        [
            "warning: `owner` now grants \"graph:export\" on `Graph`, a new permission its `*` grant covers; its holders gain it without a new binding"
        ]
    );
    // A new `@permissionOnly` permission is outside `*`.
    let found = findings(&edit(
        BASE,
        "    permission delete = \"graph:delete\"\n",
        "    permission delete = \"graph:delete\"\n    @permissionOnly\n    permission export = \"graph:export\"\n",
    ));
    assert!(role_findings(&found).is_empty(), "{found:#?}");
}

#[test]
fn a_grant_the_model_takes_from_the_service_narrows_scopes_that_do_not_cover_it() {
    // History hangs off the organization; graphAdmin is bound at graphs
    // and at the organization. The service could allow History's read at
    // a graph binding; the model grants it only at the organization.
    let base = edit(
        &history_under_organization(BASE),
        "    at [allGraphs, graph]\n    includes [graphWriter]\n",
        "    at [allGraphs, graph, organization]\n    includes [graphWriter]\n",
    );
    let decided = edit(
        &base,
        "    grants History [read] by service\n",
        "    grants History [read]\n",
    );
    let found = findings_from(&base, &decided);
    assert_eq!(
        at(&found, "compatibility.role", "graphAdmin"),
        [
            "error: `graphAdmin` bound at \"allGraphs\", \"graph\" no longer grants \"history:read\" on `History`: the model decides it now, where the service did (`by service`), and those scopes do not cover both `History` and its collection; its holders bound there are refused what the service could allow"
        ]
    );
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphAdmin"),
        [
            "warning: `graphAdmin` now grants \"history:read\" on `History` outright, where the service decided (`by service`); its holders gain it without the service's check"
        ]
    );
    // `narrow role graphAdmin` declares it.
    assert!(crucible_policy::role_narrowed(
        &lower(&base),
        &lower(&decided),
        "graphAdmin"
    ));
}

#[test]
fn an_instance_scope_never_covers_the_collection_the_service_could_allow() {
    // graphReader, bound at one graph, may create graphs where the service
    // decides; the model's grant needs `Graph(*)`, which a binding at one
    // graph never covers.
    let base = edit(
        BASE,
        "    permission delete = \"graph:delete\"\n",
        "    permission delete = \"graph:delete\"\n    permission create = \"graph:create\"\n",
    );
    let base = edit(
        &base,
        "role graphReader = \"graphReader\" { at [allGraphs, graph]; grants Graph [read] }",
        "role graphReader = \"graphReader\" { at [graph]; grants Graph [read]; grants Graph [create] by service }",
    );
    let decided = edit(
        &base,
        "grants Graph [read]; grants Graph [create] by service }",
        "grants Graph [read, create] }",
    );
    let found = findings_from(&base, &decided);
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "error: `graphReader` bound at \"graph\" no longer grants \"graph:create\" on `Graph`: the model decides it now, where the service did (`by service`), and those scopes do not cover both `Graph` and its collection; its holders bound there are refused what the service could allow"
        ]
    );
    assert!(crucible_policy::role_narrowed(
        &lower(&base),
        &lower(&decided),
        "graphReader"
    ));
}

#[test]
fn under_custom_a_permission_becoming_grantable_widens_and_the_reverse_narrows() {
    let custom = edit(
        BASE,
        "    roles [owner, graphReader, graphWriter, graphAdmin]\n",
        "    roles [owner, graphReader, graphWriter, graphAdmin] custom\n",
    );
    let kept = edit(
        &custom,
        "    permission delete = \"graph:delete\"\n",
        "    @notGrantable\n    permission delete = \"graph:delete\"\n",
    );
    let found = findings_from(&custom, &kept);
    assert_eq!(
        at(&found, "compatibility.role", "Atlas"),
        [
            "error: `Atlas` lets customers define roles (`custom`), and \"graph:delete\" on `Graph` is no longer grantable: holders of custom roles that include it are refused it"
        ]
    );
    let found = findings_from(&kept, &custom);
    assert_eq!(
        at(&found, "compatibility.role-widened", "Atlas"),
        [
            "warning: `Atlas` lets customers define roles (`custom`), and \"graph:delete\" on `Graph` is now grantable: a custom role may include it"
        ]
    );
    assert!(at(&found, "compatibility.role", "Atlas").is_empty());
    // Without `custom`, no customer role can acquire it: nothing.
    let not_grantable = edit(
        BASE,
        "    permission delete = \"graph:delete\"\n",
        "    @notGrantable\n    permission delete = \"graph:delete\"\n",
    );
    let found = findings_from(&not_grantable, BASE);
    assert!(at(&found, "compatibility.role-widened", "Atlas").is_empty());
}

#[test]
fn a_kind_of_principal_added_widens_every_role_without_holders() {
    let more = edit(
        BASE,
        "    API_KEY = \"apiKey\"\n}",
        "    API_KEY = \"apiKey\"\n    SERVICE = \"service\"\n}",
    );
    let found = findings_from(BASE, &more);
    for role in ["graphReader", "graphWriter", "graphAdmin"] {
        assert_eq!(
            at(&found, "compatibility.role-widened", role),
            [format!(
                "warning: `{role}` names no `holders`, so every kind of principal may hold it, and `principals` now has `SERVICE`: review whether those kinds should"
            )],
            "{role}"
        );
    }
    // `owner` names its holders: a new kind is not one of them.
    assert!(at(&found, "compatibility.role-widened", "owner").is_empty());
    // A kind removed refuses no holder of a role without `holders`, which
    // is valid for any kind (the runtime's `valid_for`): no role finding.
    let found = findings_from(&more, BASE);
    none_of(&found, RULES);
}

/// `model` with `roles [..] custom`.
fn with_custom(model: &str) -> String {
    edit(
        model,
        "    roles [owner, graphReader, graphWriter, graphAdmin]\n",
        "    roles [owner, graphReader, graphWriter, graphAdmin] custom\n",
    )
}

#[test]
fn under_custom_what_a_scope_covers_of_grantable_permissions_is_compared() {
    // A custom binding is valid at any scope: History moved out from under
    // Graph takes it from custom roles bound at a graph, though no
    // predefined role grants it there.
    let custom = with_custom(BASE);
    let moved = history_under_organization(&custom);
    let found = findings_from(&custom, &moved);
    assert_eq!(
        at(&found, "compatibility.role", "Atlas"),
        [
            "error: `Atlas` lets customers define roles (`custom`), and a custom role bound at \"allGraphs\" no longer covers `History`, where it may grant \"history:read\"; holders of custom roles bound there are refused it",
            "error: `Atlas` lets customers define roles (`custom`), and a custom role bound at \"graph\" no longer covers `History`, where it may grant \"history:read\"; holders of custom roles bound there are refused it",
        ]
    );
    // Moved back in beneath them, they gain it.
    let found = findings_from(&moved, &custom);
    assert_eq!(
        at(&found, "compatibility.role-widened", "Atlas"),
        [
            "warning: `Atlas` lets customers define roles (`custom`), and a custom role bound at \"allGraphs\" now covers `History`, where it may grant \"history:read\"; holders of custom roles bound there gain it without a new binding",
            "warning: `Atlas` lets customers define roles (`custom`), and a custom role bound at \"graph\" now covers `History`, where it may grant \"history:read\"; holders of custom roles bound there gain it without a new binding",
        ]
    );
    assert!(at(&found, "compatibility.role", "Atlas").is_empty());
    // A scope added is a place custom roles reach everything beneath.
    let added = edit(
        &custom,
        "    scopes [organization, allGraphs, graph]\n",
        "    scopes [organization, allGraphs, graph, everything]\n",
    );
    let added = edit(
        &added,
        "/// One graph.\n",
        "/// The whole service.\nscope everything = \"everything\" on service\n/// One graph.\n",
    );
    let found = findings_from(&custom, &added);
    let widened = at(&found, "compatibility.role-widened", "Atlas");
    assert!(
        widened.contains(&"warning: `Atlas` lets customers define roles (`custom`), and a custom role bound at \"everything\" now covers `Graph`, where it may grant \"graph:read\", \"graph:write\", \"graph:delete\"; holders of custom roles bound there gain it without a new binding".to_owned()),
        "{widened:#?}"
    );
    assert_eq!(widened.len(), 3, "{widened:#?}");
    // Without `custom`, the same move is the predefined roles' to report.
    let found = findings_from(BASE, &history_under_organization(BASE));
    assert!(at(&found, "compatibility.role", "Atlas").is_empty());
}

#[test]
fn turning_custom_or_direct_on_widens_and_off_narrows() {
    let roles = "    roles [owner, graphReader, graphWriter, graphAdmin]\n";
    for (word, on_message, off_message) in [
        (
            "custom",
            "warning: `Atlas` now lets customers define roles (`custom`): a requirement may be met by what the model does not list, and custom roles stored before take effect",
            "error: `Atlas` no longer lets customers define roles (`custom`): holders of custom roles are refused what they had",
        ),
        (
            "direct",
            "warning: `Atlas` now grants permissions outside roles (`direct`): a requirement may be met by what the model does not list, and permissions granted outside roles stored before take effect",
            "error: `Atlas` no longer grants permissions outside roles (`direct`): holders of permissions granted outside roles are refused what they had",
        ),
    ] {
        let on = edit(
            BASE,
            roles,
            &format!("    roles [owner, graphReader, graphWriter, graphAdmin] {word}\n"),
        );
        let found = findings_from(BASE, &on);
        assert_eq!(
            at(&found, "compatibility.role-widened", "Atlas"),
            [on_message],
            "{word}"
        );
        assert!(at(&found, "compatibility.role", "Atlas").is_empty());
        let found = findings_from(&on, BASE);
        assert_eq!(
            at(&found, "compatibility.role", "Atlas"),
            [off_message],
            "{word}"
        );
        assert!(at(&found, "compatibility.role-widened", "Atlas").is_empty());
    }
    // A release from before roles reports neither.
    let custom = edit(
        BASE,
        roles,
        "    roles [owner, graphReader, graphWriter, graphAdmin] custom\n",
    );
    assert!(role_findings(&findings_from(&without_roles(BASE), &custom)).is_empty());
}

#[test]
fn a_grant_moving_between_the_model_and_the_service_is_reported_for_review() {
    // The service decided, now the model grants it outright: wider.
    let found = findings(&edit(
        BASE,
        "    grants History [read] by service\n",
        "    grants History [read]\n",
    ));
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphAdmin"),
        [
            "warning: `graphAdmin` now grants \"history:read\" on `History` outright, where the service decided (`by service`); its holders gain it without the service's check"
        ]
    );
    none_of(&found, &["compatibility.role"]);
    // The model granted it, now the service decides: holders keep it where
    // the service's hook allows, which only a review of the hook can tell.
    let found = findings(&edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; grants Graph [read] by service }",
    ));
    assert_eq!(
        at(&found, "compatibility.role", "graphReader"),
        [
            "warning: `graphReader` now leaves \"graph:read\" on `Graph` to the service (`by service`); its holders keep it only where the service allows"
        ]
    );
    assert!(
        !found
            .iter()
            .any(|f| RULES.contains(&f.rule.as_str()) && f.severity == Severity::Error),
        "{found:#?}"
    );
    // A new grant the service decides is a widening too.
    let found = findings(&edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; grants Graph [read]; grants History [read] by service }",
    ));
    assert_eq!(
        at(&found, "compatibility.role-widened", "graphReader"),
        [
            "warning: `graphReader` now grants \"history:read\" on `History` where the service decides (`by service`); its holders may gain it without a new binding"
        ]
    );
}

#[test]
fn moving_a_role_or_scope_to_internal_breaks_sdk_source() {
    let found = findings(&edit(
        BASE,
        "role owner = \"owner\"",
        "internal role owner = \"owner\"",
    ));
    assert_eq!(
        at(&found, "compatibility.sdk", "owner"),
        ["error: `owner` became internal, so client SDKs lose its constant"]
    );
    none_of(&found, RULES);
    let both = edit(
        &edit(
            BASE,
            "role owner = \"owner\"",
            "internal role owner = \"owner\"",
        ),
        "scope organization = \"organization\"",
        "internal scope organization = \"organization\"",
    );
    let found = findings(&both);
    assert_eq!(
        at(&found, "compatibility.sdk", "organization"),
        ["error: `organization` became internal, so client SDKs lose its constant"]
    );
    // Making one public again is additive.
    assert!(role_findings(&findings_from(&both, BASE)).is_empty());
}

#[test]
fn a_removed_permission_is_reported_once_not_for_every_role() {
    let new = edit(BASE, "    permission delete = \"graph:delete\"\n", "");
    let new = edit(
        &new,
        "idempotent operation DeleteGraph deletes Graph {\n    http DELETE \"/orgs/{organizationId}/graphs/{graphId}\"\n    requires delete\n",
        "idempotent operation DeleteGraph deletes Graph {\n    http DELETE \"/orgs/{organizationId}/graphs/{graphId}\"\n    requires write\n",
    );
    let new = edit(&new, "    grants Graph [delete]\n", "");
    let found = findings(&new);
    assert!(
        found
            .iter()
            .any(|f| f.rule == "compatibility.permissions"
                && f.shape == "example.atlas#Graph$delete"),
        "{found:#?}"
    );
    none_of(&found, RULES);
}

#[test]
fn a_permission_renamed_through_an_alias_keeps_every_grant() {
    let new = edit(
        BASE,
        "    permission delete = \"graph:delete\"\n",
        "    permission remove = \"graph:remove\"\n    @deprecated @aliasOf(remove)\n    permission delete = \"graph:delete\"\n",
    );
    let new = edit(&new, "    requires delete\n", "    requires remove\n");
    let new = edit(
        &new,
        "    grants Graph [delete]\n",
        "    grants Graph [remove]\n",
    );
    let found = findings(&new);
    assert!(role_findings(&found).is_empty(), "{found:#?}");
}

/// A model with every role, scope and `principals` line taken out: a
/// release from before roles existed.
fn without_roles(model: &str) -> String {
    // The catalogue is the rest of the file.
    let end = model.find("/// The organization itself.").unwrap();
    model[..end]
        .lines()
        .filter(|line| {
            !["principals ", "scopes [", "roles ["]
                .iter()
                .any(|word| line.trim_start().starts_with(word))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_baseline_from_before_roles_produces_no_role_findings() {
    let bare = without_roles(BASE);
    assert!(!bare.contains("role "), "{bare}");
    let found = findings_from(&bare, BASE);
    assert!(role_findings(&found).is_empty(), "{found:#?}");
    // Nor does one whose catalogue the model removed: that is a change to
    // what was released, reported as such.
    assert!(!role_findings(&findings_from(BASE, &bare)).is_empty());
}

#[test]
fn the_role_rules_can_be_excepted() {
    let config = Config::parse(
        "schema_version = 1\n[[exceptions]]\nrule = 'compatibility.role'\nshape = 'example.atlas#graphWriter'\nreason = 'no binding stores it yet'\n",
    )
    .unwrap();
    let new = edit(
        BASE,
        "includes [graphReader]; grants Graph [write] }",
        "includes [graphReader] }",
    );
    let report = diff(&lower(BASE), &lower(&new), &config);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == "compatibility.role"
                && f.shape == "example.atlas#graphWriter"
                && f.exception.is_some())
    );
    // graphAdmin's loss is its own finding.
    assert!(report.has_errors());
    for rule in RULES {
        assert!(
            Config::parse(&format!(
                "schema_version = 1\n[[exceptions]]\nrule = '{rule}'\nshape = 'example.atlas#owner'\nreason = 'reviewed'\n"
            ))
            .is_ok(),
            "{rule}"
        );
    }
}

// The guard: a release frozen as a snapshot, and the migration from it.

const MANIFEST: &str = "[model]\nnamespace = \"example.atlas\"\nimports = [\"crucible.errors\"]\n";

/// A package whose baseline, `releases/v1.json`, is `baseline` frozen as
/// `crucible freeze` freezes it, and whose model is `current` plus
/// `migration`, at version v2 when there is one.
fn package(name: &str, baseline: &str, current: &str, migration: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("role-compatibility-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let staging = dir.with_extension("staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("crucible.toml"), MANIFEST).unwrap();
    std::fs::write(staging.join("atlas.cru"), baseline).unwrap();
    let compiled = compile(&load_package(&staging).unwrap(), None);
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the baseline does not compile"));
    crucible_lang::release::freeze(&dir.join("releases"), &service, "test").unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/v1.json\"\n"),
    )
    .unwrap();
    let current = if migration.is_empty() {
        current.to_owned()
    } else {
        current.replace("version \"v1\"", "version \"v2\"")
    };
    std::fs::write(dir.join("atlas.cru"), format!("{current}\n{migration}")).unwrap();
    dir
}

/// The guard of a package whose `releases`, each a whole source naming its
/// own version, are frozen in order, the last one its baseline, and whose
/// model is `current`.
fn run_releases(name: &str, releases: &[&str], current: &str) -> Guard {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("role-compatibility-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let mut baseline = String::new();
    for (index, source) in releases.iter().enumerate() {
        let staging = dir.with_extension(format!("staging{index}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("crucible.toml"), MANIFEST).unwrap();
        std::fs::write(staging.join("atlas.cru"), source).unwrap();
        let compiled = compile(&load_package(&staging).unwrap(), None);
        assert!(compiled.service.is_some(), "{}", compiled.render());
        let service = compiled.service.unwrap();
        baseline = service.version.clone();
        crucible_lang::release::freeze(&dir.join("releases"), &service, "test").unwrap();
    }
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{MANIFEST}\n[model.baseline]\npath = \"releases/{baseline}.json\"\n"),
    )
    .unwrap();
    std::fs::write(dir.join("atlas.cru"), current).unwrap();
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    guard(&loaded, &compiled, own, &Config::default())
        .expect("a baseline")
        .expect("the baseline loads")
}

#[test]
fn a_value_an_earlier_release_retired_stays_reserved() {
    // v1 has graphAdmin; v2 removes it; v3 brings the value back.
    let v1 = BASE.to_owned();
    let v2 = edit(BASE, ", graphAdmin]", "]");
    let v2 = v2[..v2.find("/// Read, write and delete graphs.").unwrap()]
        .replace("version \"v1\"", "version \"v2\"");
    let v3 =
        BASE.replace("version \"v1\"", "version \"v3\"") + "\nmigration \"v3\" from \"v2\" {}\n";
    let guard = run_releases("reused-role", &[&v1, &v2], &v3);
    let reused: Vec<String> = guard
        .undeclared
        .iter()
        .filter(|f| f.shape == "example.atlas#graphAdmin")
        .map(|f| format!("{} {}", f.rule, f.message))
        .collect();
    assert_eq!(
        reused,
        [
            "compatibility.role the role `graphAdmin` reuses \"graphAdmin\", which release \"v1\" had and a later release retired: bindings stored under it then, inert since, would take effect again with what it grants now; give it a new value"
        ]
    );
    assert!(guard.has_errors());

    // A scope's value, the same.
    let v2 = edit(BASE, ", allGraphs, graph]", ", graph]");
    let v2 = edit(
        &v2,
        "/// Every graph in the organization.\nscope allGraphs = \"allGraphs\" on Graph(*)\n",
        "",
    )
    .replace("at [allGraphs, graph]", "at [graph]")
    .replace("version \"v1\"", "version \"v2\"");
    let guard = run_releases("reused-scope", &[&v1, &v2], &v3);
    assert!(
        undeclared_errors(&guard)
            .contains(&"compatibility.scope example.atlas#allGraphs".to_owned()),
        "{:?}",
        undeclared_errors(&guard)
    );

    // A value no release had is new: nothing.
    let v2 = BASE.replace("version \"v1\"", "version \"v2\"");
    let added = edit(
        &BASE.replace("version \"v1\"", "version \"v3\""),
        ", graphAdmin]",
        ", graphAdmin, auditor]",
    ) + "\nrole auditor = \"auditor\" { at [graph]; grants Graph [read] }\n\nmigration \"v3\" from \"v2\" {}\n";
    let guard = run_releases("new-value", &[&v1, &v2], &added);
    assert_eq!(undeclared_errors(&guard), Vec::<String>::new());
}

fn run(name: &str, baseline: &str, current: &str, migration: &str) -> Guard {
    let dir = package(name, baseline, current, migration);
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    guard(&loaded, &compiled, own, &Config::default())
        .expect("a baseline")
        .expect("the baseline loads")
}

fn undeclared(guard: &Guard) -> Vec<String> {
    guard
        .undeclared
        .iter()
        .map(|finding| format!("{} {}", finding.rule, finding.shape))
        .collect()
}

fn problems(guard: &Guard) -> Vec<String> {
    guard
        .problems
        .iter()
        .map(|item| item.diagnostic.message.clone())
        .collect()
}

#[test]
fn a_frozen_release_from_before_roles_guards_nothing_about_them() {
    let guard = run("before-roles", &without_roles(BASE), BASE, "");
    assert_eq!(undeclared(&guard), Vec::<String>::new());
    assert!(!guard.has_errors());
}

#[test]
fn a_migration_declares_an_intended_narrowing() {
    let narrowed = edit(
        BASE,
        "includes [graphReader]; grants Graph [write] }",
        "includes [graphReader] }",
    );
    let guard = run("narrow-undeclared", BASE, &narrowed, "");
    assert_eq!(
        undeclared(&guard),
        [
            "compatibility.role example.atlas#graphAdmin",
            "compatibility.role example.atlas#graphWriter",
        ]
    );
    assert!(guard.has_errors());

    let guard = run(
        "narrow-declared",
        BASE,
        &narrowed,
        "migration \"v2\" from \"v1\" {\n    /// Writing moves to a role of its own.\n    narrow role graphWriter\n    narrow role graphAdmin\n}\n",
    );
    assert_eq!(undeclared(&guard), Vec::<String>::new());
    assert_eq!(problems(&guard), Vec::<String>::new());
    assert_eq!(guard.declared, 2);

    // A widening is a warning, which needs no step.
    let widened = edit(
        BASE,
        "{ at [allGraphs, graph]; grants Graph [read] }",
        "{ at [allGraphs, graph]; grants Graph [read, delete] }",
    );
    let guard = run("widen", BASE, &widened, "");
    assert!(!guard.has_errors(), "{:?}", undeclared(&guard));
}

/// The undeclared errors, as `rule shape`; warnings left out.
fn undeclared_errors(guard: &Guard) -> Vec<String> {
    guard
        .undeclared
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .map(|finding| format!("{} {}", finding.rule, finding.shape))
        .collect()
}

#[test]
fn removing_a_value_does_not_declare_its_symbol_reused_for_another() {
    // `graph`'s value goes, and the symbol now names a scope on every
    // graph: a client's `graph` constant now asks for all of them.
    let current = edit(
        BASE,
        "scope graph = \"graph\" on Graph\n",
        "scope graph = \"everyGraph\" on Graph(*)\n",
    );
    let guard = run(
        "remove-scope-reused",
        BASE,
        &current,
        "migration \"v2\" from \"v1\" {\n    remove scope graph\n}\n",
    );
    assert_eq!(problems(&guard), Vec::<String>::new());
    assert_eq!(
        undeclared_errors(&guard),
        ["compatibility.sdk example.atlas#graph"]
    );
    assert!(guard.has_errors());

    // A role symbol kept for a new value: the same.
    let current = edit(
        BASE,
        "role graphReader = \"graphReader\"",
        "role graphReader = \"superReader\"",
    );
    let guard = run(
        "remove-role-reused",
        BASE,
        &current,
        "migration \"v2\" from \"v1\" {\n    remove role graphReader\n}\n",
    );
    assert_eq!(problems(&guard), Vec::<String>::new());
    assert_eq!(
        undeclared_errors(&guard),
        ["compatibility.sdk example.atlas#graphReader"]
    );
}

#[test]
fn a_migration_declares_a_removed_role_or_scope() {
    let current = edit(BASE, ", graphAdmin]", "]");
    let current = current[..current.find("/// Read, write and delete graphs.").unwrap()].to_owned();
    let current = edit(&current, ", allGraphs, graph]", ", graph]");
    let current = edit(
        &current,
        "/// Every graph in the organization.\nscope allGraphs = \"allGraphs\" on Graph(*)\n",
        "",
    );
    let current = current.replace("at [allGraphs, graph]", "at [graph]");
    let guard = run("remove-undeclared", BASE, &current, "");
    assert_eq!(
        undeclared(&guard),
        [
            "compatibility.scope example.atlas#allGraphs",
            "compatibility.sdk example.atlas#allGraphs",
            "compatibility.role example.atlas#graphAdmin",
            "compatibility.sdk example.atlas#graphAdmin",
        ]
    );
    let guard = run(
        "remove-declared",
        BASE,
        &current,
        "migration \"v2\" from \"v1\" {\n    remove role graphAdmin\n    remove scope allGraphs\n}\n",
    );
    assert_eq!(undeclared(&guard), Vec::<String>::new());
    assert_eq!(problems(&guard), Vec::<String>::new());
}

#[test]
fn a_declared_role_step_must_match_what_changed() {
    let guard = run(
        "phantom",
        BASE,
        BASE,
        r#"migration "v2" from "v1" {
    remove role graphAdmin
    remove scope graph
    remove role auditor
    narrow role graphReader
}
"#,
    );
    assert_eq!(
        problems(&guard),
        [
            "\"graphAdmin\" still names the role `graphAdmin` in the current model; nothing was removed",
            "\"graph\" still names the scope `graph` in the current model; nothing was removed",
            "The baseline has no role `auditor`",
            "`graphReader` grants what it granted, at the same scopes, to the same holders in the current model; nothing was narrowed",
        ]
    );
    assert!(guard.has_errors());

    // A role whose value is gone is removed, not narrowed.
    let renamed = edit(
        BASE,
        "role graphReader = \"graphReader\"",
        "role graphReader = \"reader\"",
    );
    let guard = run(
        "narrow-removed",
        BASE,
        &renamed,
        "migration \"v2\" from \"v1\" {\n    narrow role graphReader\n}\n",
    );
    assert_eq!(
        problems(&guard),
        ["\"graphReader\" names no role in the current model; declare `remove role graphReader`"]
    );
}

#[test]
fn a_role_step_parses_and_a_misspelled_one_is_refused() {
    let parsed =
        crucible_lang::parse("migration \"v2\" from \"v1\" {\n    remove view graphAdmin\n}\n");
    let messages: Vec<String> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("expected `operation`, `role` or `scope`")),
        "{messages:?}"
    );
}

/// The shared test package `tests/models/<name>`.
fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// The guard, and the chain of releases built, for a shared package.
fn checked_model(name: &str) -> (Guard, Result<(), String>) {
    let loaded = load_package(&model(name)).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let guard = guard(&loaded, &compiled, own, &Config::default())
        .expect("a baseline")
        .expect("the baseline loads");
    let versions = crucible_lang::versions(&loaded, &compiled, own, &Config::default())
        .into_result(&compiled)
        .map(|_| ());
    (guard, versions)
}

#[test]
fn the_role_migration_package_declares_every_break() {
    let (guard, versions) = checked_model("roles-migration");
    assert_eq!(undeclared(&guard), Vec::<String>::new());
    assert_eq!(problems(&guard), Vec::<String>::new());
    // The removed role and scope, each with its SDK constant, and the
    // stocker's narrowed holders.
    assert_eq!(guard.declared, 5);
    // The steps convert nothing, so the releases still build.
    versions.unwrap();
}

#[test]
fn the_role_migration_refusals_package_is_refused_at_each_step() {
    let (guard, _) = checked_model("roles-migration-refusals");
    assert_eq!(
        problems(&guard),
        [
            "\"keeper\" still names the role `keeper` in the current model; nothing was removed",
            "\"shelf\" still names the scope `shelf` in the current model; nothing was removed",
            "`browser` grants what it granted, at the same scopes, to the same holders in the current model; nothing was narrowed",
            "The baseline has no role `cashier`",
        ]
    );
    assert!(guard.has_errors());
}
