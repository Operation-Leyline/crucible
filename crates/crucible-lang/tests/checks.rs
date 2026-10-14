//! What the compiler refuses, and how it says so.

use crucible_lang::{Package, SourceFile, compile};
use crucible_smithy::ir::Shape;
use serde_json::json;

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
    readonly property note: string
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
        baseline: None,
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
    let m = messages("structure Note { @query text: string }\nstructure Uses { note: Note }");
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
fn includes_copy_members() {
    assert_eq!(
        messages("structure Base { id: string }\nstructure Full { include Base\n name: string }"),
        Vec::<String>::new()
    );
    let m = messages("structure Loop { include Loop }");
    assert_mentions(&m, "`Loop` includes itself");
}

/// Compiles `extra` beside the base model.
fn compiled(extra: &str) -> crucible_lang::Compiled {
    compile(
        &Package::new(
            "example.shop",
            vec![SourceFile {
                path: "shop.cru".to_owned(),
                text: format!("{SERVICE}\n{extra}"),
            }],
        ),
        None,
    )
}

/// The operation `name` of a model that compiled without an error.
fn operation(compiled: &crucible_lang::Compiled, name: &str) -> crucible_smithy::ir::Operation {
    let service = compiled.service.as_ref().unwrap_or_else(|| {
        panic!("no service: {}", compiled.render());
    });
    service
        .operations
        .iter()
        .find(|operation| operation.id.name() == name)
        .unwrap_or_else(|| panic!("no operation {name}"))
        .clone()
}

/// Shared HTTP parameters, as a bundle of members for `include`.
const PAGING: &str =
    "structure Paging {\n    @query nextToken?: string\n    @query(\"max\") limit?: integer\n}\n";

/// Shared idempotency token.
const RETRY: &str = "structure Retry { @token clientToken?: string }\n";

#[test]
fn an_included_structure_binds_where_it_is_included() {
    // Included into an input, the bindings apply there, and the structure is
    // not also judged as a nested type nobody uses.
    let model = compiled(&format!(
        "{PAGING}structure OrderView for Order {{ $* }}\nlist Orders: OrderView\noperation ListOrders lists Order {{\n    http GET \"/orders\"\n    input {{ include Paging }}\n    output {{ orders: Orders }}\n}}"
    ));
    assert_eq!(model.render(), "");
    let list = operation(&model, "ListOrders");
    let query: Vec<(&str, &str)> = list
        .request
        .query
        .iter()
        .map(|b| (b.member.as_str(), b.name.as_str()))
        .collect();
    assert_eq!(query, [("nextToken", "nextToken"), ("limit", "max")]);
    // Nothing uses it yet: its bindings are judged where it is first used.
    assert_eq!(messages(PAGING), Vec::<String>::new());
    // A bundle that includes another, itself only included, is still one.
    let m = messages(&format!(
        "{PAGING}structure Listing {{\n    include Paging\n    @query filter?: string\n}}\nreadonly operation ListAll {{\n    http GET \"/all\"\n    input {{ include Listing }}\n}}"
    ));
    assert_eq!(m, Vec::<String>::new());
    // A token travels the same way, and makes the operation idempotent.
    let model = compiled(&format!(
        "{RETRY}operation Touch on Order {{\n    http POST \"/orders/{{orderId}}/touch\"\n    input {{\n        $orderId\n        include Retry\n    }}\n}}"
    ));
    assert_eq!(model.render(), "");
    let touch = operation(&model, "Touch");
    assert!(touch.idempotent);
    assert_eq!(touch.idempotency_token.as_deref(), Some("clientToken"));
}

#[test]
fn an_included_binding_is_still_checked_where_it_lands() {
    // Used as a member's type, the structure is nested: reported once per
    // binding, not once per use.
    let m = messages(&format!(
        "{PAGING}structure Page {{ paging: Paging }}\nstructure Other {{ paging?: Paging }}"
    ));
    let nested = "`@query` binds a member of an operation input; here it would be ignored";
    assert_eq!(m.iter().filter(|x| x.contains(nested)).count(), 2, "{m:?}");
    // Included into an output, or into a structure used as a type.
    let m = messages(&format!(
        "{PAGING}readonly operation Peek {{\n    http GET \"/peek\"\n    output {{ include Paging }}\n}}"
    ));
    assert_mentions(&m, "`PeekOutput` includes it, where it would be ignored");
    let m = messages(&format!(
        "{PAGING}structure Wrapper {{ include Paging }}\nstructure Uses {{ wrapper: Wrapper }}"
    ));
    assert_mentions(&m, "`Wrapper` includes it, where it would be ignored");
    // A token outside an input, through an error's include.
    let m = messages(&format!(
        "{RETRY}error Busy {{\n    status 409\n    include Retry\n}}"
    ));
    assert_mentions(
        &m,
        "`@token` marks an operation input member; `Busy` includes it",
    );
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
        baseline: None,
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

/// `SPECIAL`'s wire value spells RED's symbol, so a symbol and a quoted
/// string lower differently.
const COLOR: &str = r#"
enum Color { RED = "r", SPECIAL = "RED" }
list Colors: Color
map Palette: string -> Color
map Shades: Color -> string
"#;

#[test]
fn enum_symbols_in_defaults_lower_to_wire_values_at_any_depth() {
    // Note is an operation's input, so the service keeps it.
    let model = r#"
service Studio {
    version "1"
    protocol json
    operations [Paint]
}

operation Paint {
    http POST "/paint"
    input { note: Note }
}

structure Note {
    colors: Colors = [RED, "RED"]
    palette: Palette = { a: RED, b: "RED" }
    shades: Shades = { RED: "x", SPECIAL: "y" }
    symbol: Color = RED
    wire: Color = "RED"
}
"#;
    let package = Package {
        namespace: "example.shop".to_owned(),
        files: vec![SourceFile {
            path: "shop.cru".to_owned(),
            text: format!("{model}{COLOR}"),
        }],
        naming: Default::default(),
        imports: Vec::new(),
        baseline: None,
    };
    let compiled = compile(&package, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    let Some(Shape::Structure(note)) = service.shape(&"example.shop#Note".parse().unwrap()) else {
        panic!("Note is a structure");
    };
    let default = |name: &str| note.member(name).unwrap().default.clone();
    assert_eq!(default("colors"), Some(json!(["r", "RED"])));
    assert_eq!(default("palette"), Some(json!({"a": "r", "b": "RED"})));
    // An unquoted key of an enum-keyed map is a symbol too.
    assert_eq!(default("shades"), Some(json!({"r": "x", "RED": "y"})));
    assert_eq!(default("symbol"), Some(json!("r")));
    assert_eq!(default("wire"), Some(json!("RED")));
    // A quoted symbol name is no wire value.
    assert_mentions(
        &messages(&format!(
            r#"{COLOR}structure Note {{ colors: Colors = ["SPECIAL"] }}"#
        )),
        "is not a value",
    );
    // A symbol key and a quoted key naming one wire value collide.
    assert_mentions(
        &messages(&format!(
            r#"{COLOR}structure Note {{ shades: Shades = {{ RED: "x", "r": "y" }} }}"#
        )),
        "`r` is given twice",
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
fn updates_implies_no_idempotency() {
    // Smithy's update lifecycle does not require idempotency, and an update
    // that appends a version is not safe to repeat.
    let edit = |modifiers: &str, method: &str| {
        compiled(&format!(
            "{modifiers}operation EditOrder updates Order {{\n    http {method} \"/orders/{{orderId}}\"\n    input {{\n        $orderId\n        comment?: string\n    }}\n}}"
        ))
    };
    let patch = edit("", "PATCH");
    assert_eq!(patch.render(), "");
    let lowered = operation(&patch, "EditOrder");
    assert!(!lowered.idempotent && !lowered.readonly);
    // Saying so makes it idempotent, which PUT requires.
    assert!(operation(&edit("idempotent ", "PATCH"), "EditOrder").idempotent);
    assert!(operation(&edit("idempotent ", "PUT"), "EditOrder").idempotent);
    assert!(
        edit("", "PUT")
            .render()
            .contains("a PUT operation must be `idempotent`")
    );
    // It still changes the resource, so it is never `readonly`.
    assert!(
        edit("readonly ", "PATCH")
            .render()
            .contains("`updates` changes the resource; the operation is not `readonly`")
    );
    // `puts` and `deletes` still imply it.
    let put = compiled(
        "operation PutOrder puts Order {\n    http PUT \"/orders/{orderId}\"\n    input { $orderId }\n}",
    );
    assert!(operation(&put, "PutOrder").idempotent);
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
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("did you mean"))
            .count(),
        1,
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
fn did_you_mean_is_only_for_a_member_elision_could_replace() {
    // A member of another type than the property cannot be `$note`.
    let (errors, warnings) = split(
        "operation Annotate on Order {\n    http POST \"/orders/{orderId}/annotate\"\n    input {\n        $orderId\n        note: long\n    }\n}",
    );
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
    // A collection operation filtering on a child identifier may not bind
    // it, so `$lineId` would be an error, not a fix.
    let (errors, warnings) = split(
        "resource Line in Order {\n    id lineId: string\n    readonly property qty: long\n}\noperation ListLines lists Line {\n    http GET \"/orders/{orderId}/lines\"\n    input {\n        $orderId\n        @query lineId?: string\n        @query qty?: long\n    }\n}",
    );
    assert_eq!(errors, Vec::<String>::new());
    // A property it may bind still gets the hint. (Line's outputs show none
    // of its properties, which decision 34h warns about too.)
    let hints: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("did you mean"))
        .collect();
    assert_eq!(hints.len(), 1, "{warnings:?}");
    assert!(hints[0].contains("did you mean `$qty`"), "{warnings:?}");
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
