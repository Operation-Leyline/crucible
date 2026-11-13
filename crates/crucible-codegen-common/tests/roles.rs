//! The one rule every output states roles by (docs/roles.md §5 and §7
//! "Clients"): the client registry every language generates from, and the
//! granted-by sentences, read from the IR's granted-by analysis.

use crucible_codegen_common::roles::{
    client_roles, client_scopes, granted_by_sentences, roles_registry,
};
use crucible_ir::ir::Service;
use crucible_ir::{Audience, project};

fn load(name: &str) -> Service {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir).unwrap()
}

fn sentences(service: &Service, name: &str, internal: bool) -> Vec<String> {
    let op = service
        .operations
        .iter()
        .find(|op| op.id.name() == name)
        .unwrap_or_else(|| panic!("{name}"));
    granted_by_sentences(service, op, internal)
}

/// Every language's registry: the same roles, in the same order, with the
/// same fields, whatever audience the service was projected for.
#[test]
fn the_client_registry_is_one_rule_for_every_audience() {
    let server = load("roles-granted-by");
    let client = project(&server, Audience::External).unwrap();
    let rows = |service: &Service| -> Vec<String> {
        client_roles(service)
            .iter()
            .map(|r| {
                format!(
                    "{} at {:?} held by {:?} assigned by {:?} grants {:?}",
                    r.value,
                    r.scopes,
                    r.holders,
                    r.assigned_by,
                    r.permissions
                        .iter()
                        .map(|p| format!("{}/{}", p.resource_type, p.value))
                        .collect::<Vec<_>>()
                )
            })
            .collect()
    };
    assert_eq!(rows(&server), rows(&client));
    assert_eq!(
        rows(&client),
        [
            r#"owner at ["workspace"] held by ["user"] assigned by ["owner"] grants ["note/note:read", "note/note:write", "note/note:create", "workspace/workspace:read", "workspace/workspace:admin"]"#,
            r#"viewer at ["workspace", "allNotes", "note"] held by [] assigned by [] grants ["note/note:read"]"#,
            r#"editor at ["allNotes", "note"] held by [] assigned by [] grants ["note/note:read", "note/note:write", "note/note:create"]"#,
            r#"archivist at ["allNotes"] held by [] assigned by [] grants ["note/note:read", "note/note:write", "note/note:create"]"#,
        ]
    );
    let scopes: Vec<&str> = client_scopes(&server).iter().map(|s| s.value).collect();
    assert_eq!(scopes, ["workspace", "allNotes", "note"]);
    // The server-audience registry (descriptions, OpenAPI) keeps the
    // internal role and its internal grants.
    let registry = roles_registry(&server, true);
    assert_eq!(registry.as_array().unwrap().len(), 5);
    assert_eq!(registry[4]["value"], "auditor");
    assert_eq!(registry[4]["internal"], true);
    assert_eq!(roles_registry(&server, false).as_array().unwrap().len(), 4);
}

#[test]
fn granted_by_names_roles_and_never_an_internal_permission() {
    let server = load("roles-granted-by");
    let client = project(&server, Audience::External).unwrap();
    // A client SDK generated from either view says the same.
    for name in [
        "GetNote",
        "UpdateNote",
        "PurgeNote",
        "ShareNote",
        "GetArchive",
    ] {
        assert_eq!(
            sentences(&server, name, false),
            sentences(&client, name, false),
            "{name}"
        );
    }
    assert_eq!(
        sentences(&client, "PurgeNote", false),
        ["Granted by `owner` at `workspace`."]
    );
    assert_eq!(
        sentences(&client, "UpdateNote", false),
        [
            "`note:write` on the note: granted by `owner` at `workspace`; `editor` at `allNotes` or `note`; `archivist` at `allNotes`; or a custom role that grants `note:write`.",
            "A permission internal to the service, when a condition on the input holds: granted by `owner` at `workspace`."
        ]
    );
    // The server audience names the internal permission and role.
    assert_eq!(
        sentences(&server, "PurgeNote", true),
        ["Granted by `owner` at `workspace`; `auditor` at `backOffice`."]
    );
    assert_eq!(
        sentences(&server, "UpdateNote", true)[1],
        "`workspace:audit` on the workspace, when `has audited`: granted by `owner` at `workspace`; `auditor` at `backOffice`."
    );
    assert_eq!(
        sentences(&client, "GetArchive", false),
        [
            "Granted by `archivist` at `allNotes`, as the service decides (the archives of the notes this binding covers, while they are retained); or a custom role that grants `archive:read`."
        ]
    );
    assert_eq!(
        sentences(&client, "ListNotes", false),
        [
            "Returns each note the caller holds `note:read` on through `owner` at `workspace`; `viewer` at `workspace`, `allNotes` or `note`; `editor` at `allNotes` or `note`; `archivist` at `allNotes`; or a custom role that grants `note:read`."
        ]
    );
}

#[test]
fn leylines_reference_answers_its_three_examples() {
    let client = project(&load("roles-leyline"), Audience::External).unwrap();
    assert_eq!(
        sentences(&client, "GetGraph", false),
        ["Granted by `graphReader`, `graphWriter` or `graphAdmin` at `allGraphs` or `graph`."]
    );
    assert_eq!(
        sentences(&client, "CreateGraph", false),
        ["Granted by `graphCreator` at `organization`; `graphAdmin` at `allGraphs`."]
    );
    assert_eq!(
        sentences(&client, "GetGraphHistory", false),
        [
            "Granted by `owner` at `organization`; `historyReader` or `historyAdmin` at `allHistories` or `history`; `graphAdmin` at `allGraphs` or `graph`, as the service decides (the history of a graph this binding covers, while the graph is live)."
        ]
    );
    // Every operation that requires something says who grants it.
    for op in &client.operations {
        assert_eq!(
            granted_by_sentences(&client, op, false).is_empty(),
            op.granted_by.is_empty(),
            "{}",
            op.id
        );
    }
}

#[test]
fn a_model_without_roles_says_nothing() {
    let weather = load("weather");
    assert!(client_roles(&weather).is_empty());
    assert!(
        weather
            .operations
            .iter()
            .all(|op| granted_by_sentences(&weather, op, true).is_empty())
    );
}

/// A `filters` clause lists each instance the caller may see, so a binding
/// on one note grants as much as one on all notes; its sentence says what
/// the caller sees. An internal filter still names its roles.
#[test]
fn a_filter_is_granted_by_any_binding_that_covers_an_instance() {
    let server = load("roles-granted-by");
    let scopes_of = |operation: &str, role: &str| -> Vec<String> {
        server
            .operations
            .iter()
            .find(|op| op.id.name() == operation)
            .unwrap()
            .granted_by[0]
            .roles
            .iter()
            .find(|g| g.role == role)
            .unwrap()
            .scopes
            .clone()
    };
    assert_eq!(
        scopes_of("ListNotes", "viewer"),
        ["workspace", "allNotes", "note"]
    );
    assert_eq!(scopes_of("ListNotes", "editor"), ["allNotes", "note"]);
    // `requires create on Note(*)` needs the whole collection: one note's
    // binding does not grant it.
    assert_eq!(scopes_of("CreateNote", "editor"), ["allNotes"]);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/roles-granted-by");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("filters read on Note", "filters purge on Note");
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let server = compiled.service.unwrap_or_else(|| panic!("{rendered}"));
    let client = project(&server, Audience::External).unwrap();
    assert_eq!(
        sentences(&client, "ListNotes", false),
        ["What the caller sees is granted by `owner` at `workspace`."]
    );
    // An SDK method says it in order: the requirement sentence, the
    // filtered note, then which roles grant it.
    let list = client
        .operations
        .iter()
        .find(|op| op.id.name() == "ListNotes")
        .unwrap();
    assert_eq!(
        crucible_codegen_common::docs::requirement_sentences(&client, list),
        [
            "Requires no permission.",
            "Returns only what the caller may see.",
            "What the caller sees is granted by `owner` at `workspace`."
        ]
    );
    assert_eq!(
        sentences(&server, "ListNotes", true),
        [
            "Returns each note the caller holds `note:purge` on through `owner` at `workspace`; `auditor` at `backOffice`."
        ]
    );
}
