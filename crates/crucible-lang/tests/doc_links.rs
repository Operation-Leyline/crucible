//! `[Name]` doc links (decision 80): what they resolve to, what the
//! compiler refuses, and what an external projection leaves of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crucible_ir::{Audience, ShapeId, project};
use crucible_lang::load_service;

fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.links", name)
}

#[test]
fn links_resolve_to_operations_types_members_values_resources_and_the_service() {
    let service = load_service(&model("doc-links")).unwrap();
    let expected: BTreeMap<String, ShapeId> = [
        ("Choice", id("Choice")),
        ("Choice.label", id("Choice").with_member("label")),
        ("Choice.widget", id("Choice").with_member("widget")),
        ("Code", id("Code")),
        ("Code.ONE", id("Code").with_member("ONE")),
        ("Label", id("Label")),
        (
            "WidgetError.message",
            id("WidgetError").with_member("message"),
        ),
        ("Widgets", id("Widgets")),
        ("WidgetsByName", id("WidgetsByName")),
        ("Color", id("Color")),
        ("Color.BLUE", id("Color").with_member("BLUE")),
        ("Color.RED", id("Color").with_member("RED")),
        ("CreateWidget", id("CreateWidget")),
        // An operation's member is its input's, else its output's.
        (
            "CreateWidget.name",
            id("CreateWidgetInput").with_member("name"),
        ),
        ("Gadget", id("Gadget")),
        ("GetWidget", id("GetWidget")),
        (
            "GetWidget.widget",
            id("GetWidgetOutput").with_member("widget"),
        ),
        (
            "GetWidget.widgetId",
            id("GetWidgetInput").with_member("widgetId"),
        ),
        ("Links", id("Links")),
        ("Reveal", id("Reveal")),
        ("Secret", id("Secret")),
        ("Secret.value", id("Secret").with_member("value")),
        ("Widget", id("Widget")),
        ("Widget.name", id("Widget").with_member("name")),
        ("Widget.object", id("Widget").with_member("object")),
        ("WidgetError", id("WidgetError")),
    ]
    .into_iter()
    .map(|(label, id)| (label.to_owned(), id))
    .collect();
    // `[String]` is a prelude type: accepted, with nothing to link to.
    assert_eq!(service.doc_links, expected);
    // An operation's member keeps the operation it names, for a renderer
    // to route exactly where operations share a structure.
    let owners: Vec<(&str, &str)> = service
        .doc_link_operations
        .iter()
        .map(|(label, op)| (label.as_str(), op.name()))
        .collect();
    assert_eq!(
        owners,
        [
            ("CreateWidget.name", "CreateWidget"),
            ("GetWidget.widget", "GetWidget"),
            ("GetWidget.widgetId", "GetWidget"),
        ]
    );
    assert_eq!(
        service
            .doc_link_operation("GetWidget.widgetId")
            .map(|op| op.id.name()),
        Some("GetWidget")
    );
    // The documentation stays as written.
    assert!(
        service
            .docs
            .as_deref()
            .unwrap()
            .contains("[the read call][GetWidget]")
    );
    assert!(service.doc_link("Widget.name").is_some());
    assert!(service.doc_link("String").is_none());
}

#[test]
fn an_external_projection_drops_links_to_internal_api() {
    let service = load_service(&model("doc-links")).unwrap();
    let external = project(&service, Audience::External).unwrap();
    for label in ["Reveal", "Secret", "Secret.value"] {
        assert!(!external.doc_links.contains_key(label), "{label}");
        assert!(external.doc_link(label).is_none(), "{label}");
    }
    assert!(external.doc_links.contains_key("GetWidget"));
    let internal = project(&service, Audience::Internal).unwrap();
    assert!(internal.doc_link("Secret").is_some());
}

#[test]
fn a_link_that_names_nothing_is_an_error_at_its_label() {
    let refused = load_service(&model("doc-links-unresolved")).unwrap_err();
    for (position, message) in [
        (
            "unresolved.cru:1:10:",
            "the doc link `[Nowhere]` names nothing in this package or its imports",
        ),
        (
            "unresolved.cru:8:19:",
            "the doc link `[Thing.missing]` names nothing: `Thing` has no member `missing`",
        ),
        (
            "unresolved.cru:8:36:",
            "the doc link `[Kind.GREEN]` names nothing: `Kind` has no value `GREEN`",
        ),
        (
            "unresolved.cru:8:53:",
            "the doc link `[String.length]` names nothing: `String` is a prelude type, with no members",
        ),
        (
            "unresolved.cru:19:25:",
            "the doc link `[GetThing.nope]` names nothing: neither `GetThing`'s input nor its output has a member `nope`",
        ),
        (
            "unresolved.cru:19:48:",
            "the doc link `[Label.x]` names nothing: `Label` has no members to link; an operation, structure, union, error or enum does",
        ),
    ] {
        assert!(
            refused
                .lines()
                .any(|line| line.contains(position) && line.contains(message)),
            "{position} {message}\n{refused}"
        );
    }
}

fn messages(source: &str) -> Vec<String> {
    let package = crucible_lang::Package::new(
        "example.nowhere",
        vec![crucible_lang::SourceFile {
            path: "nowhere.cru".to_owned(),
            text: source.to_owned(),
        }],
    );
    crucible_lang::compile(&package, None)
        .diagnostics
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

#[test]
fn a_package_without_one_service_still_checks_its_links() {
    for services in [
        "",
        "service A { version \"1\" protocol json }\nservice B { version \"1\" protocol json }\n",
    ] {
        let found = messages(&format!(
            "{services}/// See [Nowhere] and [Thing.missing].\nstructure Thing {{ name: string }}\n"
        ));
        assert!(
            found
                .iter()
                .any(|m| m.contains("`[Nowhere]` names nothing")),
            "{found:#?}"
        );
        assert!(
            found
                .iter()
                .any(|m| m.contains("`Thing` has no member `missing`")),
            "{found:#?}"
        );
    }
}

#[test]
fn a_rule_can_be_named_but_has_no_members() {
    let found = messages(
        "service A {\n    version \"1\"\n    protocol json\n}\n\n/// Every operation says what it does.\nrule Explained advice {\n    every operation\n    must documented\n}\n\n/// See [Explained], not [Explained.x].\nstructure Thing { name: string }\n",
    );
    assert!(
        !found.iter().any(|m| m.contains("`[Explained]`")),
        "{found:#?}"
    );
    assert!(
        found
            .iter()
            .any(|m| m
                .contains("`[Explained.x]` names nothing: `Explained` is a rule, with no members")),
        "{found:#?}"
    );
}

#[test]
fn a_dangling_link_says_how_to_write_brackets_that_are_not_one() {
    let found = messages("/// See [Nowhere].\nstructure Thing { name: string }\n");
    assert!(
        found
            .iter()
            .any(|m| m.contains(r"write `\[Nowhere]` or a code span if it is not a link")),
        "{found:#?}"
    );
}
