//! Roles and scopes in `.cru` (docs/roles.md §4 and §6): the syntax, its
//! lowering to the IR, the client projection and the compile errors,
//! against Leyline's catalogue and one package per construct and refusal
//! under `tests/models/roles-*`.

use std::path::{Path, PathBuf};

use crucible_ir::ShapeId;
use crucible_ir::ir::{GrantedSet, Role, ScopeTarget, Service};
use crucible_ir::{Audience, project, snapshot};
use crucible_lang::ast::{Decl, GrantList, RoleEntryKind, ScopeTarget as AstScopeTarget};
use crucible_lang::{Compiled, Severity, compile, load_package, load_service, parse};

fn models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models")
}

fn compiled_with(package: &str, edit: impl Fn(&str) -> String) -> Compiled {
    let mut package = load_package(&models().join(package)).unwrap();
    for file in &mut package.files {
        file.text = edit(&file.text.replace("\r\n", "\n"));
    }
    compile(&package, None)
}

fn compiled(package: &str) -> Compiled {
    compiled_with(package, str::to_owned)
}

/// The package's service; it compiles with no diagnostic at all.
fn service(package: &str) -> Service {
    let compiled = compiled(package);
    assert!(
        compiled.diagnostics.is_empty(),
        "{package}:\n{}",
        compiled.render()
    );
    compiled.service.expect("a service")
}

fn leyline() -> Service {
    service("roles-leyline")
}

/// Every diagnostic's message, with its severity.
fn diagnostics(compiled: &Compiled) -> Vec<(Severity, String)> {
    compiled
        .diagnostics
        .iter()
        .map(|d| (d.diagnostic.severity, d.diagnostic.message.clone()))
        .collect()
}

/// The package's errors; it must have no warning beyond `warnings`.
fn errors(package: &str) -> Vec<String> {
    errors_of(&compiled(package))
}

fn errors_of(compiled: &Compiled) -> Vec<String> {
    diagnostics(compiled)
        .into_iter()
        .filter(|(severity, _)| *severity == Severity::Error)
        .map(|(_, message)| message)
        .collect()
}

/// Asserts each message is among `found`, and that nothing else is.
fn assert_exactly(found: &[String], expected: &[&str]) {
    for message in expected {
        assert!(
            found.iter().any(|f| f == message),
            "missing:\n  {message}\nfound:\n  {}",
            found.join("\n  ")
        );
    }
    assert_eq!(
        found.len(),
        expected.len(),
        "found:\n  {}",
        found.join("\n  ")
    );
}

fn id(namespace: &str, name: &str) -> ShapeId {
    ShapeId::new(namespace, name)
}

fn role<'s>(service: &'s Service, symbol: &str) -> &'s Role {
    service
        .role(symbol)
        .unwrap_or_else(|| panic!("no role {symbol}"))
}

/// A role's effective grants as `Resource.permission`.
fn effective(role: &Role) -> Vec<String> {
    role.effective
        .iter()
        .map(|g| format!("{}.{}", g.resource.name(), g.permission))
        .collect()
}

// ---- syntax ------------------------------------------------------------------

#[test]
fn scopes_roles_and_their_entries_parse() {
    let source = r#"
service S {
    roles [a, b] custom direct
}
/// Every graph.
internal scope allGraphs = "allGraphs" on Graph(*)
scope graph = "graph" on Graph
scope everything = "everything" on service
@deprecated
role a = "a" {
    at [graph, allGraphs]; includes [b]
    grants Graph [read, write]
    /// While the graph is live.
    grants History [* except [restore]] by service
    holders [USER]
    assignedBy [owner]
}
"#;
    let parsed = parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let items = &parsed.file.items;
    let Decl::Service(service) = &items[0].decl else {
        panic!("a service")
    };
    let flags: Vec<&str> = service.clauses[0]
        .flags
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(flags, ["custom", "direct"]);
    let Decl::Scope(all) = &items[1].decl else {
        panic!("a scope")
    };
    assert!(items[1].is_internal());
    assert_eq!(all.value.value, "allGraphs");
    assert!(
        matches!(&all.target, AstScopeTarget::Resource { name, every: true } if name.name == "Graph")
    );
    let Decl::Scope(everything) = &items[3].decl else {
        panic!("a scope")
    };
    assert!(matches!(everything.target, AstScopeTarget::Service(_)));
    let Decl::Role(a) = &items[4].decl else {
        panic!("a role")
    };
    assert_eq!(items[4].annotations[0].name.name, "deprecated");
    let kinds: Vec<&str> = a.entries.iter().map(|e| e.keyword.name.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "at",
            "includes",
            "grants",
            "grants",
            "holders",
            "assignedBy"
        ]
    );
    let RoleEntryKind::Grants(by_service) = &a.entries[3].kind else {
        panic!("a grant")
    };
    assert!(by_service.by_service.is_some());
    assert_eq!(
        by_service.doc.as_ref().map(|d| d.text.as_str()),
        Some("While the graph is live.")
    );
    assert!(
        matches!(&by_service.permissions, GrantList::All { except, .. } if except[0].name == "restore")
    );
}

#[test]
fn a_misspelt_role_entry_is_one_error_and_parsing_goes_on() {
    let parsed =
        parse("role a = \"a\" {\n    at [x]\n    grant X [read]\n}\nscope x = \"x\" on service\n");
    let messages: Vec<&str> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages.contains(
            &"expected `at`, `includes`, `grants`, `holders` or `assignedBy`, found `grant`"
        ),
        "{messages:?}"
    );
    assert!(matches!(
        parsed.file.items.last().unwrap().decl,
        Decl::Scope(_)
    ));
}

#[test]
fn assigned_by_holders_is_refused_as_not_supported_yet() {
    let parsed = parse("role a = \"a\" {\n    at [x]\n    assignedBy holders\n}\n");
    let messages: Vec<&str> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        [
            "`assignedBy holders` (an assigner may grant only what it holds) is not supported yet; list the roles that may assign it: `assignedBy [role, ..]`"
        ]
    );
}

// ---- Leyline (docs/roles.md §8) ------------------------------------------------

#[test]
fn leyline_compiles_with_no_diagnostic() {
    leyline();
}

#[test]
fn the_catalogue_is_in_the_services_order() {
    let service = leyline();
    let roles: Vec<&str> = service.roles.iter().map(|r| r.symbol.as_str()).collect();
    assert_eq!(
        roles,
        [
            "owner",
            "accessAdmin",
            "graphGroupAdmin",
            "usageViewer",
            "auditViewer",
            "auditAdmin",
            "backupViewer",
            "backupAdmin",
            "graphCreator",
            "graphReader",
            "graphWriter",
            "graphAdmin",
            "historyReader",
            "historyAdmin"
        ]
    );
    let scopes: Vec<(&str, &str, ScopeTarget)> = service
        .role_scopes
        .iter()
        .map(|s| (s.symbol.as_str(), s.value.as_str(), s.on.clone()))
        .collect();
    let ns = |name: &str| id("example.leyline", name);
    assert_eq!(
        scopes,
        [
            (
                "organization",
                "organization",
                ScopeTarget::Instance(ns("Organization"))
            ),
            ("allGraphs", "allGraphs", ScopeTarget::Every(ns("Graph"))),
            ("graph", "graph", ScopeTarget::Instance(ns("Graph"))),
            (
                "allHistories",
                "allHistories",
                ScopeTarget::Every(ns("GraphHistory"))
            ),
            (
                "history",
                "history",
                ScopeTarget::Instance(ns("GraphHistory"))
            ),
        ]
    );
    assert_eq!(
        service.role_scopes[1].docs.as_deref(),
        Some("Every graph in the organization, including graphs created later.")
    );
    let settings = &service.role_settings;
    assert_eq!(settings.principals, Some(ns("PrincipalType")));
    assert_eq!(settings.role_types, Some(ns("Role")));
    assert_eq!(settings.scope_types, Some(ns("RoleScopeType")));
    assert!(!settings.custom && !settings.direct);
    // The enums are part of the IR, for generators to read.
    assert!(service.shape(&ns("PrincipalType")).is_some());
}

#[test]
fn a_role_keeps_what_it_declares() {
    let service = leyline();
    let owner = role(&service, "owner");
    assert_eq!(owner.value, "owner");
    assert_eq!(owner.scopes, ["organization"]);
    assert_eq!(owner.holders, ["USER"]);
    assert_eq!(owner.assigned_by, ["owner"]);
    assert_eq!(
        owner.docs.as_deref(),
        Some("Everything in the organization, and every history in it.")
    );
    assert_eq!(
        owner.grants[0].permissions,
        GrantedSet::All { except: Vec::new() }
    );
    assert_eq!(
        owner.grants[1].permissions,
        GrantedSet::Named(vec!["read".to_owned(), "restore".to_owned()])
    );
    let writer = role(&service, "graphWriter");
    assert_eq!(writer.includes, ["graphReader"]);
    assert!(writer.holders.is_empty() && writer.assigned_by.is_empty());
    let admin = role(&service, "graphAdmin");
    let by_service = &admin.grants[1];
    assert!(by_service.by_service);
    assert_eq!(
        by_service.docs.as_deref(),
        Some("The history of a graph this binding covers, while the graph is live.")
    );
}

#[test]
fn effective_grants_expand_inclusion_and_star_and_keep_the_service_decided_apart() {
    let service = leyline();
    // Inclusion, transitively: graphAdmin ⊃ graphWriter ⊃ graphReader, in
    // Graph's declaration order.
    assert_eq!(
        effective(role(&service, "graphAdmin")),
        ["Graph.read", "Graph.write", "Graph.manage", "Graph.create"]
    );
    assert_eq!(
        effective(role(&service, "graphWriter")),
        ["Graph.read", "Graph.write"]
    );
    // `*` is every Organization permission; then GraphHistory's.
    let owner = effective(role(&service, "owner"));
    assert_eq!(owner.len(), 18);
    assert_eq!(owner[0], "GraphHistory.read");
    assert_eq!(owner[2], "Organization.organizationRead");
    assert_eq!(owner[17], "Organization.backupManage");
    assert_eq!(
        effective(role(&service, "auditAdmin")),
        [
            "Organization.organizationRead",
            "Organization.auditRead",
            "Organization.auditManage"
        ]
    );
    // The `by service` grant is the service's to decide, with its doc.
    let admin = role(&service, "graphAdmin");
    assert_eq!(admin.by_service.len(), 1);
    assert_eq!(admin.by_service[0].resource.name(), "GraphHistory");
    assert_eq!(admin.by_service[0].permission, "read");
    assert_eq!(
        admin.by_service[0].docs.as_deref(),
        Some("The history of a graph this binding covers, while the graph is live.")
    );
    assert!(role(&service, "historyReader").by_service.is_empty());
}

#[test]
fn coverage_answers_leylines_rules() {
    use crucible_ir::roles::{Target, covers};
    let service = leyline();
    let on = |scope: &str| service.scope(scope).unwrap().on.clone();
    let ns = |name: &str| id("example.leyline", name);
    let graph = ns("Graph");
    let history = ns("GraphHistory");
    // `graph:create` is required on Graph(*): organization and allGraphs
    // cover it, one graph does not.
    assert!(covers(&service, &on("organization"), Target::Every(&graph)));
    assert!(covers(&service, &on("allGraphs"), Target::Every(&graph)));
    assert!(!covers(&service, &on("graph"), Target::Every(&graph)));
    // The owner reaches every history; a graph binding reaches none.
    assert!(covers(
        &service,
        &on("organization"),
        Target::Instance(&history)
    ));
    assert!(!covers(&service, &on("graph"), Target::Instance(&history)));
    assert!(!covers(
        &service,
        &on("allGraphs"),
        Target::Instance(&history)
    ));
}

#[test]
fn clients_see_public_roles_without_their_internal_grants() {
    let full = leyline();
    let client = project(&full, Audience::External).unwrap();
    // Every role and scope is public; every permission is internal.
    assert_eq!(client.roles.len(), 14);
    assert_eq!(client.role_scopes.len(), 5);
    assert!(client.permissions.is_empty());
    for role in &client.roles {
        assert!(role.grants.is_empty(), "{}", role.symbol);
        assert!(role.effective.is_empty(), "{}", role.symbol);
        assert!(role.by_service.is_empty(), "{}", role.symbol);
    }
    let owner = role(&client, "owner");
    assert_eq!(owner.scopes, ["organization"]);
    assert_eq!(owner.holders, ["USER"]);
    assert_eq!(owner.assigned_by, ["owner"]);
    assert_eq!(role(&client, "graphWriter").includes, ["graphReader"]);
    assert!(client.role_settings.role_types.is_some());
    // The server keeps everything.
    let server = project(&full, Audience::Internal).unwrap();
    assert_eq!(role(&server, "graphAdmin").effective.len(), 4);
}

/// An operation's granted-by, one line per clause: its clause (or `-` for
/// one the view removed), then `role@scope|scope` for each role, `by
/// service` roles marked `~`, and `+custom`.
fn granted(service: &Service, operation: &str) -> Vec<String> {
    let op = service
        .operations
        .iter()
        .find(|op| op.id.name() == operation)
        .unwrap_or_else(|| panic!("{operation}"));
    op.granted_by
        .iter()
        .map(|g| {
            let mut parts = vec![g.clause.map_or("-".to_owned(), |c| c.to_string())];
            for (mark, list) in [("", &g.roles), ("~", &g.by_service)] {
                for r in list {
                    parts.push(format!("{mark}{}@{}", r.role, r.scopes.join("|")));
                }
            }
            if g.custom {
                parts.push("+custom".to_owned());
            }
            parts.join(" ")
        })
        .collect()
}

#[test]
fn granted_by_answers_leylines_three_examples() {
    let service = leyline();
    // docs/roles.md §5, roles and scopes in catalogue order.
    assert_eq!(
        granted(&service, "GetGraph"),
        ["0 graphReader@allGraphs|graph graphWriter@allGraphs|graph graphAdmin@allGraphs|graph"]
    );
    // Only `allGraphs` and `organization` cover `Graph(*)`.
    assert_eq!(
        granted(&service, "CreateGraph"),
        ["0 graphCreator@organization graphAdmin@allGraphs"]
    );
    assert_eq!(
        granted(&service, "GetGraphHistory"),
        [
            "0 owner@organization historyReader@allHistories|history historyAdmin@allHistories|history ~graphAdmin@allGraphs|graph"
        ]
    );
    let history = service
        .operations
        .iter()
        .find(|op| op.id.name() == "GetGraphHistory")
        .unwrap();
    assert_eq!(
        history.granted_by[0].by_service[0].docs.as_deref(),
        Some("The history of a graph this binding covers, while the graph is live.")
    );
    // `requires none` has no clause to grant.
    assert!(granted(&service, "ListRoles").is_empty());
}

#[test]
fn clients_keep_granted_by_for_requirements_they_do_not_see() {
    // Leyline's permissions are all internal: clients see no clause, and
    // every role that grants one.
    let client = project(&leyline(), Audience::External).unwrap();
    assert_eq!(
        granted(&client, "CreateGraph"),
        ["- graphCreator@organization graphAdmin@allGraphs"]
    );
    let op = client
        .operations
        .iter()
        .find(|op| op.id.name() == "GetGraph")
        .unwrap();
    assert_eq!(op.requirements, crucible_ir::ir::Requirements::Internal);
}

#[test]
fn granted_by_covers_every_clause_kind_and_projects_by_tier() {
    let server = service("roles-granted-by");
    assert_eq!(
        granted(&server, "GetNote"),
        [
            "0 owner@workspace viewer@workspace|allNotes|note editor@allNotes|note archivist@allNotes +custom"
        ]
    );
    // A filter lists each note the caller may see: a binding on one note
    // counts, unlike `requires create on Note(*)` below, which needs them all.
    assert_eq!(
        granted(&server, "ListNotes"),
        [
            "0 owner@workspace viewer@workspace|allNotes|note editor@allNotes|note archivist@allNotes +custom"
        ]
    );
    assert_eq!(
        granted(&server, "CreateNote"),
        ["0 owner@workspace editor@allNotes archivist@allNotes +custom"]
    );
    assert_eq!(
        granted(&server, "GetArchive"),
        ["0 ~archivist@allNotes +custom"]
    );
    // An internal permission is no custom role's to grant.
    assert_eq!(
        granted(&server, "PurgeNote"),
        ["0 owner@workspace auditor@backOffice"]
    );
    let update = server
        .operations
        .iter()
        .find(|op| op.id.name() == "UpdateNote")
        .unwrap();
    assert!(update.granted_by[1].conditional);
    let share = server
        .operations
        .iter()
        .find(|op| op.id.name() == "ShareNote")
        .unwrap();
    assert_eq!(share.granted_by[1].mode, crucible_ir::ir::Mode::Handler);

    // Clients: the internal role and scope go, the internal clauses lose
    // their position and keep their public roles.
    let client = project(&server, Audience::External).unwrap();
    assert_eq!(granted(&client, "PurgeNote"), ["- owner@workspace"]);
    assert_eq!(
        granted(&client, "UpdateNote"),
        [
            "0 owner@workspace editor@allNotes|note archivist@allNotes +custom",
            "- owner@workspace"
        ]
    );
    assert_eq!(granted(&client, "GetNote"), granted(&server, "GetNote"));
}

#[test]
fn a_model_without_roles_has_no_granted_by() {
    for package in ["roles-tiers", "roles-leyline"] {
        let service = service(package);
        assert!(
            service
                .operations
                .iter()
                .any(|op| !op.granted_by.is_empty())
        );
    }
    let compiled = compile(
        &load_package(&models().join("../fixtures/permissions")).unwrap(),
        None,
    );
    let service = compiled.service.expect("the permissions fixture compiles");
    assert!(service.operations.iter().all(|op| op.granted_by.is_empty()));
    let text = snapshot::write(&service, "test");
    assert!(!text.contains("granted_by"));
}

/// Leyline with `PrincipalType` named only by `principals` and `holders`:
/// no operation's input or output uses it.
fn principals_only_in_the_catalogue(text: &str) -> String {
    text.replace("    holders: PrincipalTypes\n", "")
        .replace("list PrincipalTypes: PrincipalType\n", "")
}

#[test]
fn clients_keep_holders_whose_enum_only_the_catalogue_names() {
    let compiled = compiled_with("roles-leyline", principals_only_in_the_catalogue);
    assert!(compiled.diagnostics.is_empty(), "{}", compiled.render());
    let full = compiled.service.unwrap();
    let client = project(&full, Audience::External).unwrap();
    let principals = id("example.leyline", "PrincipalType");
    assert_eq!(client.role_settings.principals, Some(principals.clone()));
    assert!(client.shape(&principals).is_some());
    // Not `[]`, which would read as "anyone may hold it".
    assert_eq!(role(&client, "owner").holders, ["USER"]);
    assert_eq!(role(&client, "auditAdmin").holders, ["USER"]);
}

/// An internal principals enum cannot restrict a public role: refused, so
/// the projection never has to drop a holder.
#[test]
fn a_public_role_is_not_held_by_an_internal_principal_enum() {
    let compiled = compiled_with("roles-leyline", |text| {
        principals_only_in_the_catalogue(text)
            .replace("enum PrincipalType {", "internal enum PrincipalType {")
    });
    let refused: Vec<String> = errors_of(&compiled)
        .into_iter()
        .filter(|e| e.starts_with("roles.internal"))
        .collect();
    assert_exactly(
        &refused,
        &[
            "roles.internal: `owner` is held by `USER`, which is internal, and is part of the external contract: a client would read the role as held by every kind; make the role `internal`, or the value public",
            "roles.internal: `accessAdmin` is held by `USER`, which is internal, and is part of the external contract: a client would read the role as held by every kind; make the role `internal`, or the value public",
            "roles.internal: `auditAdmin` is held by `USER`, which is internal, and is part of the external contract: a client would read the role as held by every kind; make the role `internal`, or the value public",
        ],
    );
}

#[test]
fn a_catalogue_round_trips_through_a_release_snapshot() {
    let service = leyline();
    let text = snapshot::write(&service, "test");
    let read = snapshot::read(&text).unwrap();
    assert_eq!(read.roles, service.roles);
    assert_eq!(read.role_scopes, service.role_scopes);
    assert_eq!(read.role_settings, service.role_settings);
    for (read, op) in read.operations.iter().zip(&service.operations) {
        assert_eq!(read.granted_by, op.granted_by);
    }
    assert_eq!(snapshot::write(&read, "test"), text);
    // A snapshot frozen before roles existed has none of the fields, and
    // still reads.
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let fields = value["service"].as_object_mut().unwrap();
    for field in ["roles", "role_scopes", "role_settings"] {
        assert!(fields.remove(field).is_some(), "{field} is written");
    }
    let older = snapshot::read(&value.to_string()).unwrap();
    assert!(older.roles.is_empty() && older.role_scopes.is_empty());
    assert!(older.role_settings.is_default());
}

// ---- the other constructs -----------------------------------------------------

#[test]
fn a_service_scope_star_except_and_custom() {
    let service = service("roles-service-scope");
    assert!(service.role_settings.custom);
    assert_eq!(
        service.scope("everywhere").unwrap().on,
        ScopeTarget::Service
    );
    // `*` keeps `@notGrantable` and leaves out `@permissionOnly`.
    assert_eq!(
        effective(role(&service, "admin")),
        [
            "Note.read",
            "Note.write",
            "Note.share",
            "Workspace.read",
            "Workspace.write",
            "Workspace.admin"
        ]
    );
    assert_eq!(
        effective(role(&service, "editor")),
        ["Note.read", "Note.write", "Workspace.read"]
    );
}

#[test]
fn tiers_deprecation_and_direct() {
    let full = service("roles-tiers");
    assert!(full.role_settings.direct);
    let reader = role(&full, "reader");
    assert_eq!(effective(reader), ["Shelf.read", "Shelf.audit"]);
    assert!(role(&full, "auditor").internal);
    assert!(full.scope("backOffice").unwrap().internal);
    let clerk = role(&full, "clerk");
    let deprecated = clerk.notes.deprecated.as_ref().expect("deprecated");
    assert_eq!(deprecated.message.as_deref(), Some("Use `librarian`."));
    assert_eq!(role(&full, "librarian").notes.since.as_deref(), Some("2"));
    assert_eq!(
        full.scope("aisle").unwrap().notes.since.as_deref(),
        Some("2")
    );

    let client = project(&full, Audience::External).unwrap();
    let roles: Vec<&str> = client.roles.iter().map(|r| r.symbol.as_str()).collect();
    assert_eq!(roles, ["reader", "librarian", "clerk"]);
    let scopes: Vec<&str> = client
        .role_scopes
        .iter()
        .map(|s| s.symbol.as_str())
        .collect();
    assert_eq!(scopes, ["shelf", "aisle"]);
    // The role stays; its grant of the internal `audit` leaves.
    let reader = role(&client, "reader");
    assert_eq!(effective(reader), ["Shelf.read"]);
    assert_eq!(
        reader.grants[0].permissions,
        GrantedSet::Named(vec!["read".to_owned()])
    );
    assert_eq!(
        effective(role(&client, "librarian")),
        ["Shelf.read", "Shelf.stock"]
    );
    let text = serde_json::to_string(&client).unwrap();
    assert!(!text.contains("auditor") && !text.contains("backOffice"));
}

// ---- refusals (docs/roles.md §6) ------------------------------------------------

#[test]
fn a_grant_no_scope_reaches_is_refused_and_the_help_names_by_service() {
    let compiled = compiled("roles-unreachable-grant");
    assert_exactly(
        &errors_of(&compiled),
        &[
            "roles.unreachable-grant: `shelfClerk` grants permissions on `Ledger`, but none of its scopes (shelf) covers a `Ledger`: bind it at a scope on `Ledger` or above it, or, if the service decides the grant through a relationship the resources do not have, write `grants Ledger [..] by service`",
            "roles.unreachable-grant: `shelfAuditor` includes `ledgerReader`, which grants permissions on `Ledger`, but none of `shelfAuditor`'s scopes (shelf) covers a `Ledger`: bind it at a scope on `Ledger` or above it, or include a role that does not grant them (a grant the service decides through a relationship is written `grants Ledger [..] by service`)",
        ],
    );
    // Reported at the grant, in its file.
    let diagnostic = &compiled.diagnostics[0];
    let file = &compiled.files[diagnostic.file.expect("a file")];
    let span = diagnostic.diagnostic.span;
    assert_eq!(
        &file.text[span.start as usize..span.end as usize],
        "grants Ledger [read]"
    );
}

#[test]
fn an_inclusion_cycle_is_refused_on_every_role_on_it() {
    assert_exactly(
        &errors("roles-include-cycle"),
        &[
            "roles.include-cycle: `a` includes itself: `a` includes `b` includes `a`; inclusion is acyclic",
            "roles.include-cycle: `b` includes itself: `b` includes `a` includes `b`; inclusion is acyclic",
            "roles.include-cycle: `c` includes itself: `c` includes `c`; inclusion is acyclic",
        ],
    );
}

#[test]
fn a_scope_the_service_does_not_list_is_unknown() {
    let compiled = compiled("roles-unknown-scope");
    assert_exactly(
        &errors_of(&compiled),
        &[
            "roles.unknown-scope: `shelf` is not a scope of the service; its scopes are `store`",
            "roles.unknown-scope: `nowhere` is not a scope of the service; its scopes are `store`",
        ],
    );
    assert!(diagnostics(&compiled).contains(&(
        Severity::Warning,
        "`shelf` is not part of the service: list it in the service's `scopes [..]`".to_owned()
    )));
}

#[test]
fn a_holder_names_a_value_of_the_principals_enum() {
    assert_exactly(
        &errors("roles-unknown-holder"),
        &[
            "roles.unknown-holder: `ROBOT` is not a value of `PrincipalKind`; its values are `USER`, `API_KEY`",
        ],
    );
    // Without `principals`, no holder is known.
    let compiled = compiled_with("roles-unknown-holder", |text| {
        text.replace("    principals PrincipalKind\n", "")
    });
    assert_exactly(
        &errors_of(&compiled),
        &[
            "roles.unknown-holder: `robotKeeper` names holders, but the service names no principal kinds: add `principals <Enum>` to the service",
        ],
    );
}

#[test]
fn an_assigner_is_a_role_of_the_service() {
    assert_exactly(
        &errors("roles-unknown-assigner"),
        &[
            "roles.unknown-assigner: `mayor` is not a role of the service, so no one could assign `deputy`; its roles are `keeper`, `deputy`",
        ],
    );
}

#[test]
fn two_roles_or_two_scopes_never_share_a_value() {
    assert_exactly(
        &errors("roles-duplicate-value"),
        &[
            "roles.duplicate-value: the scopes `store` and `outlet` both have the value \"store\"; stored bindings name a scope by its value, so each needs its own",
            "roles.duplicate-value: the roles `keeper` and `manager` both have the value \"keeper\"; stored bindings name a role by its value, so each needs its own",
        ],
    );
}

#[test]
fn linked_wire_enums_are_checked_both_ways() {
    assert_exactly(
        &errors("roles-types"),
        &[
            "roles.types: \"ghost\" is not the value of a role of the service; its values are `\"keeper\"`, `\"shelver\"`",
            "roles.types: the role `shelver` has no value in `RoleName`; add one with the value \"shelver\"",
            "roles.types: the scope `shelf` has no value in `ScopeName`; add one with the value \"shelf\"",
        ],
    );
}

#[test]
fn a_public_value_names_no_internal_role_and_a_public_role_has_a_public_value() {
    let compiled = compiled_with("roles-tiers", |text| {
        format!(
            "{text}\n@roleTypes\nenum RoleName {{\n    READER = \"reader\"\n    LIBRARIAN = \"librarian\"\n    internal CLERK = \"clerk\"\n    AUDITOR = \"auditor\"\n}}\n\nreadonly operation DescribeRole {{\n    http GET \"/role\"\n    requires none\n    output {{\n        role: RoleName\n    }}\n}}\n"
        )
    });
    assert_exactly(
        &errors_of(&compiled),
        &[
            "roles.types: \"auditor\" names the role `auditor`, which is internal, and is part of the external contract: mark the value `internal`",
            "roles.types: the role `clerk` is public, but its value \"clerk\" in `RoleName` is internal, so clients could not name it",
        ],
    );
}

#[test]
fn a_required_permission_no_role_grants_is_refused_unless_custom_or_direct() {
    assert_exactly(
        &errors("roles-ungranted"),
        &[
            "roles.ungranted: no role grants `read` on `Ledger`, which `GetLedger` requires, so no binding could satisfy it: grant it in a role; or say the service grants permissions outside roles, `roles [..] direct`, or lets customers define roles, `roles [..] custom`",
            "roles.ungranted: no role grants `stock` on `Shelf`, which `StockShelf` requires, so no binding could satisfy it: grant it in a role; or say the service grants permissions outside roles, `roles [..] direct`, or lets customers define roles, `roles [..] custom`",
        ],
    );
    for flag in ["custom", "direct"] {
        let compiled = compiled_with("roles-ungranted", |text| {
            text.replace("roles [reader]", &format!("roles [reader] {flag}"))
        });
        assert!(
            compiled.diagnostics.is_empty(),
            "{flag}:\n{}",
            compiled.render()
        );
    }
    // A service without roles grants directly, as before roles existed.
    let compiled = compiled_with("roles-ungranted", |text| {
        text.replace("    roles [reader]\n", "")
            .replace("    scopes [store]\n", "")
    });
    assert!(
        !errors_of(&compiled)
            .iter()
            .any(|e| e.starts_with("roles.ungranted")),
        "{}",
        compiled.render()
    );
}

#[test]
fn a_grant_names_primary_permissions_of_a_resource() {
    assert_exactly(
        &errors("roles-grant"),
        &[
            "roles.grant: `Missing` is not a resource; a role grants a resource's permissions: `grants Resource [permission, ..]`",
            "roles.grant: `Shelf` declares no permission `dust`; its permissions are `read`, `stock`",
            "roles.grant: `read` is named twice",
            "roles.grant: `Store` declares no permission `count`; its permissions are `read`",
            "roles.grant: the grant on `Ledger` names no permission",
            "roles.grant: `unlock` is an alias; name its primary `open`",
            "roles.grant: `*` does not include `audit`, which is `@permissionOnly`, so excepting it changes nothing; remove it",
        ],
    );
}

#[test]
fn a_public_role_or_scope_names_nothing_internal() {
    assert_exactly(
        &errors("roles-internal"),
        &[
            "roles.internal: `vault` is on `Vault`, which is internal, and is part of the external contract: make the scope `internal`",
            "roles.internal: `teller` binds at `backRoom`, which is internal, and is part of the external contract: make the role `internal`, or the scope public",
            "roles.internal: `teller` is assigned by `boss`, which is internal, and is part of the external contract: a client would read the role as assignable by anyone; make the role `internal`, or `boss` public",
            "roles.internal: `teller` is held by `ROBOT`, which is internal, and is part of the external contract: a client would read the role as held by every kind; make the role `internal`, or the value public",
        ],
    );
    // An internal service (IR another front end built) has no external
    // contract to mislead.
    let mut service = compiled("roles-internal")
        .best_effort
        .expect("what lowering made of it");
    let internal = |service: &Service| {
        crucible_ir::validate::roles(service)
            .iter()
            .filter(|f| f.code == "roles.internal")
            .count()
    };
    assert_eq!(internal(&service), 4);
    service.internal = true;
    assert_eq!(internal(&service), 0);
}

#[test]
fn malformed_declarations_are_refused() {
    let compiled = compiled("roles-declarations");
    assert_exactly(
        &errors_of(&compiled),
        &[
            "`store` is listed twice",
            "roles.principals: `Store` is not an enum; `principals` names the enum of principal kinds whose values `holders` lists",
            "`@permissionTypes` does not apply to a role; a role takes `@deprecated` and `@since`",
            "`twice` has two `at` entries; list every name in one `at [..]`",
            "`USER` is listed twice",
            "roles.scope: `orphanScope` is on `Orphan`, which is not a resource of the service; a scope is on a resource the service reaches, or on `service`",
            "roles.value: \"Bad Value\" is not a role value: write lowercase-led segments of letters and digits, joined by `:`, with single `-` inside a segment, such as \"graphReader\"",
            "roles.at: `nowhere` binds at no scope: say where it is held, `at [scope, ..]`",
            "roles.unknown-role: `phantom` is not a role of the service; its roles are `keeper`, `nowhere`, `ghostly`, `badValue`, `twice`",
            "roles.unknown-holder: `twice` names holders, but the service names no principal kinds: add `principals <Enum>` to the service",
        ],
    );
    let warnings: Vec<String> = diagnostics(&compiled)
        .into_iter()
        .filter(|(s, _)| *s == Severity::Warning)
        .map(|(_, m)| m)
        .collect();
    assert_exactly(
        &warnings,
        &[
            "`forgotten` is not part of the service: list it in the service's `scopes [..]`",
            "`forgottenRole` is not part of the service: list it in the service's `roles [..]`",
        ],
    );
}

#[test]
fn one_run_reports_every_refusal() {
    let codes: std::collections::BTreeSet<String> = errors("roles-refusals")
        .iter()
        .filter_map(|m| m.split_once(':').map(|(code, _)| code.to_owned()))
        .collect();
    for code in [
        "roles.unreachable-grant",
        "roles.include-cycle",
        "roles.unknown-scope",
        "roles.unknown-holder",
        "roles.unknown-assigner",
        "roles.duplicate-value",
        "roles.types",
        "roles.ungranted",
    ] {
        assert!(codes.contains(code), "{code} missing from {codes:?}");
    }
}

// ---- a model without roles ----------------------------------------------------

/// A model without roles lowers to the IR it lowered to before roles
/// existed, byte for byte. The digests are of the release snapshots the
/// compiler wrote for these packages just before roles were added; a
/// later, intended change to the IR of every model updates them.
#[test]
fn a_model_without_roles_lowers_to_byte_identical_ir() {
    use sha2::{Digest, Sha256};
    for (package, before) in [
        (
            "weather",
            "f55c68ebeee61f06108a5a219b8e31524313b691f8ea5a3eea37b1b85ae19a46",
        ),
        (
            "internal-resource",
            "64dfbfe15ee20aa404a6a764979a7473272c12b26da80a9d5c86fc887692acc3",
        ),
    ] {
        let service = load_service(&models().join(package)).unwrap();
        assert!(service.roles.is_empty() && service.role_scopes.is_empty());
        let text = snapshot::write(&service, "test");
        for field in ["\"roles\"", "\"role_scopes\"", "\"role_settings\""] {
            assert!(!text.contains(field), "{package} writes {field}");
        }
        let digest: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(digest, before, "{package}: its IR changed");
    }
}
