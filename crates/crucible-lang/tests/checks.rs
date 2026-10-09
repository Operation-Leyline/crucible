//! What the compiler refuses, and how it says so.

use crucible_lang::{Package, SourceFile, compile};

const SERVICE: &str = r#"
service Shop {
    version "1"
    protocol json
    auth bearer
    operations [Ping]
    resources [Order]
    errors [Oops]
}

error Oops {
    status 400
    message: string
}

readonly operation Ping {
    http GET "/ping"
}

resource Order {
    id orderId: string
    property note: string
}
"#;

fn messages(extra: &str) -> Vec<String> {
    let package = Package {
        namespace: "example.shop".to_owned(),
        files: vec![SourceFile {
            path: "shop.cru".to_owned(),
            text: format!("{SERVICE}\n{extra}"),
        }],
        naming: Default::default(),
        imports: Vec::new(),
    };
    let compiled = compile(&package, None);
    if compiled.diagnostics.is_empty() {
        assert!(compiled.service.is_some(), "a clean compile has a service");
    } else {
        assert!(
            compiled.service.is_none(),
            "a failed compile has no service"
        );
    }
    compiled
        .diagnostics
        .into_iter()
        .map(|d| d.diagnostic.message)
        .collect()
}

fn assert_mentions(messages: &[String], needle: &str) {
    assert!(
        messages.iter().any(|m| m.contains(needle)),
        "no diagnostic mentions {needle:?}: {messages:#?}"
    );
}

#[test]
fn the_base_model_compiles() {
    assert_eq!(messages(""), Vec::<String>::new());
}

#[test]
fn unknown_types_are_named() {
    let m = messages("structure Note { text: Txt }");
    assert_mentions(&m, "unknown type `Txt`");
}

#[test]
fn a_uri_label_needs_a_matching_input_member() {
    let m = messages(
        "operation GetOrder reads Order {\n    http GET \"/orders/{orderId}\"\n    input {}\n}",
    );
    assert_mentions(&m, "URI label {orderId} matches no input member");
}

#[test]
fn label_members_are_required() {
    let m = messages(
        "operation GetOrder reads Order {\n    http GET \"/orders/{orderId}\"\n    input { $orderId? }\n}",
    );
    assert_mentions(&m, "must be required");
}

#[test]
fn get_requests_have_no_body() {
    let m = messages(
        "operation GetOrder reads Order {\n    http GET \"/orders/{orderId}\"\n    input { $orderId\n note: string }\n}",
    );
    assert_mentions(&m, "a GET request has no body: bind `note`");
}

#[test]
fn http_method_semantics_are_checked() {
    let m = messages(
        "operation DropOrder on Order {\n    http DELETE \"/orders/{orderId}\"\n    input { $orderId }\n}",
    );
    assert_mentions(&m, "a DELETE operation must be `idempotent`");
}

#[test]
fn elision_needs_a_resource_and_a_known_member() {
    let m = messages("structure Note { $note }");
    assert_mentions(&m, "there is no resource here");
    let m = messages("structure View for Order { $missing }");
    assert_mentions(
        &m,
        "resource `Order` has no identifier or property `missing`",
    );
}

#[test]
fn elided_members_take_the_resource_type() {
    assert_eq!(
        messages("structure View for Order { $orderId as id\n $note? }"),
        Vec::<String>::new()
    );
}

#[test]
fn annotations_must_be_declared_and_placed_correctly() {
    let m = messages("structure Note { @tier text: string }");
    assert_mentions(&m, "unknown annotation `@tier`");
    let m = messages(
        "metadata tier on operation: string\nstructure Note { @tier(\"x\") text: string }",
    );
    assert_mentions(&m, "`@tier` applies to operation, not to a member");
    let m = messages("structure Note { @query text: string }");
    assert!(
        m.is_empty(),
        "@query on a structure member is a binding only inputs use: {m:?}"
    );
}

#[test]
fn enum_metadata_values_are_checked() {
    let m = messages(
        "metadata class on operation: enum { read write }\n@class(delete)\nreadonly operation Peek {\n    http GET \"/peek\"\n}",
    );
    assert_mentions(&m, "`@class` is one of read, write");
}

#[test]
fn optional_and_default_together_is_refused() {
    let m = messages("structure Note { count?: long = 0 }");
    assert_mentions(&m, "not both");
}

#[test]
fn duplicate_declarations_are_refused() {
    let m = messages("structure Oops { a: string }");
    assert_mentions(&m, "`Oops` is already declared as an error");
}

#[test]
fn declarations_the_compiler_cannot_lower_yet_are_not_skipped() {
    let m = messages("rule R {\n    every operation must documented\n}");
    assert_mentions(
        &m,
        "`rule` declarations are not supported by the compiler yet",
    );
}

#[test]
fn recursion_is_reported_rather_than_mislowered() {
    let m = messages("structure Node { next?: Node }");
    assert_mentions(&m, "recursive shapes are not supported");
}

#[test]
fn includes_copy_members() {
    assert_eq!(
        messages("structure Base { id: string }\nstructure Full { include Base\n name: string }"),
        Vec::<String>::new()
    );
    let m = messages("structure Loop { include Loop }");
    assert_mentions(&m, "`Loop` includes itself");
}
