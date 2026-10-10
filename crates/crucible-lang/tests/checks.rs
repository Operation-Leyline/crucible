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
    // Warnings do not withhold the service; errors do.
    assert_eq!(
        compiled.service.is_none(),
        compiled.has_errors(),
        "a compile has a service exactly when it has no errors"
    );
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
    // A binding outside an input, output or error would be silently ignored.
    let m = messages("structure Note { @query text: string }");
    assert_mentions(&m, "`@query` binds a member of an operation input");
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

/// Compiles `extra` beside the base model and returns (errors, warnings).
fn split(extra: &str) -> (Vec<String>, Vec<String>) {
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
    let (errors, warnings): (Vec<_>, Vec<_>) = compiled
        .diagnostics
        .into_iter()
        .partition(|d| d.diagnostic.is_error());
    let text = |items: Vec<crucible_lang::FileDiagnostic>| {
        items.into_iter().map(|d| d.diagnostic.message).collect()
    };
    (text(errors), text(warnings))
}

#[test]
fn bindings_are_checked_where_they_are_written() {
    let m = messages(
        "operation Put on Order {\n    http PUT \"/orders/{orderId}\"\n    input {\n        $orderId\n        @query @header(\"X-A\") a: string\n    }\n    output { @query b?: string }\n}",
    );
    assert_mentions(&m, "`@query` and `@header` cannot both apply");
    assert_mentions(&m, "`@query` binds a member of an operation input");
}

#[test]
fn idempotency_tokens_are_one_optional_string() {
    let m = messages(
        "idempotent operation Make {\n    http POST \"/make\"\n    input {\n        @token a: string\n        @token b?: long\n    }\n}",
    );
    assert_mentions(&m, "the idempotency token `a` is optional");
    assert_mentions(&m, "an idempotency token is a string");
    assert_mentions(&m, "an input has at most one");
}

#[test]
fn names_maps_and_enums_are_checked() {
    assert_mentions(
        &messages("structure Note { bad-name: string }"),
        "cannot contain `-`",
    );
    assert_mentions(
        &messages("map Counts: long -> long"),
        "a map key is a string",
    );
    let m = messages("enum E { A\n A }\nenum F: integer { X = 1\n Y = 1 }\nenum G { sensitive Z }");
    assert_mentions(&m, "enum value `A` is declared twice");
    assert_mentions(&m, "enum value 1 appears twice");
    assert_mentions(&m, "only `internal` does");
}

#[test]
fn defaults_fit_their_type() {
    assert_mentions(
        &messages("structure Note { count: long = \"many\" }"),
        "the default",
    );
    assert_mentions(
        &messages("union U { a: string }\nstructure Note { u: U = { a: \"x\" } }"),
        "a union has no default",
    );
    assert_eq!(
        messages("enum Size { SMALL = \"s\" }\nstructure Note { size: Size = SMALL }"),
        Vec::<String>::new()
    );
}

#[test]
fn constraints_apply_once_to_what_they_can_constrain() {
    assert_mentions(
        &messages("long Count { length 1..2 }"),
        "`length` constrains",
    );
    assert_mentions(
        &messages("string Name { length 1..2 length 1..3 }"),
        "given twice",
    );
    assert_mentions(
        &messages("string Name { pattern \"(\" }"),
        "this pattern cannot be checked",
    );
    assert_mentions(
        &messages("string Name { length 1..10 }\nstructure Note { name: Name { length ..20 } }"),
        "loosens its type's",
    );
    assert_eq!(
        messages("string Name { length 1..10 }\nstructure Note { name: Name { length 2..5 } }"),
        Vec::<String>::new()
    );
}

#[test]
fn metadata_declarations_are_checked_once() {
    let m = messages(
        "metadata deprecated on member\nmetadata k on widget conflicts nothing retention forever\n",
    );
    assert_mentions(&m, "is a built-in annotation");
    assert_mentions(&m, "`widget` is not a metadata target");
    assert_mentions(&m, "`nothing` is not a metadata key");
    assert_eq!(
        m.iter().filter(|x| x.contains("retention")).count(),
        1,
        "{m:?}"
    );
    let m = messages(
        "metadata level on member: integer\nstructure Note { @level(\"high\") a: string }",
    );
    assert_mentions(&m, "the value of `@level`");
    let m = messages(
        "metadata secret on member conflicts sensitive\nstructure Note { @secret sensitive a: string }",
    );
    assert_mentions(&m, "`@secret` conflicts with `sensitive`");
}

#[test]
fn roles_are_checked() {
    let m = messages(
        "operation Peek reeds Order {\n    http GET \"/x/{orderId}\"\n    input { $orderId }\n}",
    );
    assert_mentions(&m, "`reeds` is not a role");
    let m = messages("operation Get reads Order {\n    http GET \"/orders\"\n}");
    assert_mentions(&m, "binds every identifier: add `$orderId`");
    let m = messages(
        "operation Make creates Order {\n    http POST \"/orders/{orderId}\"\n    input { $orderId }\n}",
    );
    assert_mentions(&m, "an operation that chooses the identifier `puts`");
    let m = messages(
        "readonly operation Drop deletes Order {\n    http DELETE \"/orders/{orderId}\"\n    input { $orderId }\n}",
    );
    assert_mentions(&m, "`deletes` implies `idempotent`");
    let m = messages(
        "operation A reads Order {\n    http GET \"/a/{orderId}\"\n    input { $orderId }\n}\noperation B reads Order {\n    http GET \"/b/{orderId}\"\n    input { $orderId }\n}",
    );
    assert_mentions(&m, "already has a `reads` operation, `A`");
    let m = messages("operation L lists Order collection {\n    http GET \"/orders\"\n}");
    assert_mentions(&m, "`collection` follows `on` only");
}

#[test]
fn resources_are_checked() {
    let m = messages("resource Line in Oops {\n    id lineId: string\n}");
    assert_mentions(&m, "`Oops` is not a resource");
    let m =
        messages("resource A in B {\n    id a: string\n}\nresource B in A {\n    id b: string\n}");
    assert_mentions(&m, "is its own ancestor");
    let m = messages("resource Line in Order {\n    id orderId: string\n    id n: long\n}");
    assert_mentions(&m, "already declared by an ancestor");
    assert_mentions(&m, "a resource identifier is a string");
}

#[test]
fn duplicates_are_refused() {
    let m = messages(
        "error Bad {\n    status 400\n    status 401\n}\nreadonly operation Ex {\n    http GET \"/ex\"\n    errors [Oops, Oops]\n    paginated(inputToken: a, inputToken: b, size: c)\n    example \"x\" {\n        input { a: 1, a: 2 }\n        input {}\n    }\n}",
    );
    assert_mentions(&m, "an error has one `status`");
    assert_mentions(&m, "`Oops` is listed twice");
    assert_mentions(&m, "`inputToken` is given twice");
    assert_mentions(&m, "`size` is not a pagination key");
    assert_mentions(&m, "`a` is given twice");
    assert_mentions(&m, "an example has one `input`");
}

#[test]
fn uri_labels_are_one_identifier_per_segment() {
    let m = messages("readonly operation Ex {\n    http GET \"/x/{a}{b}/{}\"\n}");
    assert_mentions(&m, "holds more than one label");
    assert_mentions(&m, "is an empty URI label");
}

#[test]
fn the_shared_ir_rules_apply_to_cru() {
    let m = messages(
        "readonly operation Head {\n    http HEAD \"/head\"\n    output { body: string }\n}",
    );
    assert_mentions(&m, "a HEAD response carries no body");
    let m = messages(
        "idempotent operation Put {\n    http PUT \"/put\"\n    input {\n        @header(\"Host\") host: string\n        @header(\"Authorization\") token: string\n    }\n}",
    );
    assert_mentions(&m, "the HTTP layer owns the Host header");
    assert_mentions(&m, "bearer token in the Authorization header");
    let m = messages("readonly operation Ping2 {\n    http GET \"/ping\"\n}");
    // Ping2 is not in the service, so the route does not clash with Ping.
    assert_eq!(m, Vec::<String>::new());
}

#[test]
fn warnings_do_not_withhold_the_service() {
    let (errors, warnings) = split(
        "resource Loose {\n    id looseId: string\n}\noperation GetLoose reads Loose {\n    http GET \"/loose/{looseId}\"\n    input { $looseId }\n}\noperation Note on Order {\n    http POST \"/orders/{orderId}/note\"\n    input {\n        $orderId\n        note: string\n    }\n}\n/// See [Nowhere].\nstructure Doc { a: string }",
    );
    assert_eq!(errors, Vec::<String>::new());
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("does not reach `Loose`")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("did you mean `$note`")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`[Nowhere]` names nothing")),
        "{warnings:?}"
    );
}

#[test]
fn service_clauses_are_checked() {
    let package = Package::new(
        "example.shop",
        vec![SourceFile {
            path: "shop.cru".to_owned(),
            text: "service S {\n    protocol json\n    protocol json\n    operations [P, P]\n}\nreadonly operation P {\n    http GET \"/p\"\n}\n".to_owned(),
        }],
    );
    let rendered = compile(&package, None).render();
    assert!(
        rendered.contains("a service has one `protocol` clause"),
        "{rendered}"
    );
    assert!(rendered.contains("`P` is listed twice"), "{rendered}");
    assert!(
        rendered.contains("a service declares its version"),
        "{rendered}"
    );
}

#[test]
fn a_problem_is_reported_once_where_it_is_written() {
    let m = messages("structure Note { a: string\n a: long }\nstructure Uses { note: Note }");
    assert_eq!(
        m.iter().filter(|x| x.contains("declared twice")).count(),
        1,
        "{m:?}"
    );
    // An operation without `http` still has its input checked.
    let m = messages("readonly operation NoHttp {\n    input { a: Missing }\n}");
    assert_mentions(&m, "needs an `http METHOD");
    assert_mentions(&m, "unknown type `Missing`");
}

#[test]
fn included_views_elide_against_their_own_resource() {
    assert_eq!(
        messages("structure View for Order { $note? }\nstructure Wrapper { include View }"),
        Vec::<String>::new()
    );
}

#[test]
fn an_error_payload_takes_the_whole_body() {
    let m = messages(
        "error Raw {\n    status 400\n    @payload data: blob\n    note: string\n}\nreadonly operation Get {\n    http GET \"/raw\"\n    errors [Raw]\n}",
    );
    assert_mentions(
        &m,
        "the body is the payload `data`; bind `note` to a header",
    );
}

#[test]
fn a_manifest_may_hold_policy_but_not_unknown_keys() {
    let dir = std::env::temp_dir();
    let policy =
        "[model]\nnamespace = \"example.shop\"\n\n[rules.naming]\noperations = \"PascalCase\"\n";
    assert_eq!(crucible_lang::check_manifest(&dir, policy), Vec::new());
    let typo =
        "[model]\nnamespace = \"example.shop\"\n\n[rule.naming]\noperations = \"PascalCase\"\n";
    let problems = crucible_lang::check_manifest(&dir, typo);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].message.contains("unknown key `rule`"));
    assert_eq!(problems[0].span.text(typo), "rule");
}
