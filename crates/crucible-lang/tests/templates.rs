//! Role templates (decision 47a): what every operation of a role carries.

use crucible_lang::{Compiled, Package, SourceFile, compile};
use crucible_smithy::ir::{Service, Shape};

const BASE: &str = r#"
service Shop {
    version "1"
    protocol json
    resources [Order]
}

string OrderId { length 1..32 }
string PageToken { length 1..100 }
integer PageSize { range 1..100 }
string ClientToken { length 1..64 }
string Note { length 1..100 }

resource Order {
    id orderId: OrderId
    immutable property note: Note
}

template lists {
    paginated(inputToken: nextToken, outputToken: nextToken, pageSize: maxResults)
    input {
        @query
        nextToken?: PageToken
        @query
        maxResults?: PageSize
    }
    output {
        nextToken?: PageToken
    }
}

template creates {
    input {
        @token
        clientToken?: ClientToken
    }
}

structure OrderData for Order { $orderId $note }
list OrderList: OrderData { length ..100 }

operation CreateOrder creates Order {
    http POST "/orders"
    input { $note }
    output { order: OrderData }
}

readonly operation ListOrders lists Order {
    http GET "/orders"
    paginated(items: orders)
    output { orders: OrderList }
}
"#;

fn compiled(extra: &str) -> Compiled {
    compile(
        &Package::new(
            "example.shop",
            vec![SourceFile {
                path: "shop.cru".to_owned(),
                text: format!("{BASE}\n{extra}"),
            }],
        ),
        None,
    )
}

fn service(extra: &str) -> Service {
    let compiled = compiled(extra);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    compiled.service.expect("service")
}

fn errors(extra: &str) -> Vec<String> {
    compiled(extra)
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.is_error())
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

fn members(service: &Service, shape: &str) -> Vec<String> {
    match service.shapes.iter().find(|(id, _)| id.name() == shape) {
        Some((_, Shape::Structure(s))) => s.members.iter().map(|m| m.name.clone()).collect(),
        _ => panic!("no structure {shape}"),
    }
}

#[test]
fn an_operation_gets_the_members_of_its_roles_template_after_its_own() {
    let service = service("");
    assert_eq!(
        members(&service, "CreateOrderInput"),
        ["note", "clientToken"]
    );
    assert_eq!(
        members(&service, "ListOrdersInput"),
        ["nextToken", "maxResults"]
    );
    assert_eq!(
        members(&service, "ListOrdersOutput"),
        ["orders", "nextToken"]
    );
    // The creating operation had no `output` for `creates` to add to: its
    // own is untouched, and `ListOrders` had no input at all.
    assert_eq!(members(&service, "CreateOrderOutput"), ["order"]);
}

#[test]
fn the_template_supplies_the_token_and_the_pagination_clause() {
    let service = service("");
    let create = service
        .operations
        .iter()
        .find(|o| o.name() == "CreateOrder")
        .unwrap();
    assert_eq!(create.idempotency_token.as_deref(), Some("clientToken"));
    let list = service
        .operations
        .iter()
        .find(|o| o.name() == "ListOrders")
        .unwrap();
    let paged = list.paginated.as_ref().expect("paginated");
    assert_eq!(
        (
            paged.input_token.as_str(),
            paged.output_token.as_str(),
            paged.items.as_deref(),
            paged.page_size.as_deref()
        ),
        ("nextToken", "nextToken", Some("orders"), Some("maxResults"))
    );
}

#[test]
fn restating_a_template_member_is_an_error() {
    let found = errors(
        "operation CreateAgain creates Order {\n    http POST \"/again\"\n    input {\n        $note\n        clientToken?: ClientToken\n    }\n}",
    );
    assert!(
        found
            .iter()
            .any(|m| m.contains("`clientToken` comes from `template creates`")),
        "{found:#?}"
    );
}

#[test]
fn restating_a_pagination_key_is_an_error() {
    let found = errors(
        "readonly operation ListAgain lists Order {\n    http GET \"/again\"\n    paginated(items: orders, pageSize: maxResults)\n    output { orders: OrderList }\n}",
    );
    assert!(found.iter().any(|m| m.contains("pageSize")), "{found:#?}");
}

#[test]
fn a_template_is_for_a_role_and_declared_once_and_carries_only_what_every_operation_shares() {
    let found = errors("template sideways { input { a?: Note } }");
    assert!(
        found.iter().any(|m| m.contains("`sideways` is not a role")),
        "{found:#?}"
    );
    let found = errors("template creates { input { other?: Note } }");
    assert!(
        found
            .iter()
            .any(|m| m.contains("already a `template creates`")),
        "{found:#?}"
    );
    let found = errors("template reads { http GET \"/x\" }");
    assert!(
        found.iter().any(|m| m.contains("a template carries")),
        "{found:#?}"
    );
}

#[test]
fn a_problem_in_a_template_member_is_reported_in_the_template() {
    let found = errors(
        "template reads { input { gone?: Missing } }
readonly operation GetOrder reads Order {
    http GET \"/orders/{orderId}\"
    input { $orderId }
    output { order: OrderData }
}",
    );
    assert!(
        found.iter().any(|m| m.contains("unknown type `Missing`")),
        "{found:#?}"
    );
}

#[test]
fn template_members_are_flagged_for_rules() {
    let compiled = compiled(
        "rule NoTemplateTokens strict { every input member where template must optional }\nrule Mine { every input member where not template must required }",
    );
    // `note` is the operation's own and required; `clientToken` and the
    // paging members are the templates' and optional.
    assert!(!compiled.has_errors(), "{}", compiled.render());
}

#[test]
fn a_template_has_one_body_per_side() {
    let found = errors("template reads { input { a?: Note } input { b?: Note } }");
    assert!(
        found.iter().any(|m| m.contains("one input and one output")),
        "{found:#?}"
    );
}

#[test]
fn a_problem_in_a_template_member_is_reported_once_for_all_its_operations() {
    let found = errors(
        "template deletes { input { gone?: Missing } }\noperation DeleteA deletes Order {\n    http DELETE \"/a/{orderId}\"\n    input { $orderId }\n}",
    );
    let mine: Vec<_> = found
        .iter()
        .filter(|m| m.contains("unknown type `Missing`"))
        .collect();
    assert_eq!(mine.len(), 1, "{found:#?}");
}

fn messages_with_offsets(source: &str) -> Vec<(String, u32)> {
    compiled(source)
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.is_error())
        .map(|d| (d.diagnostic.message.clone(), d.diagnostic.span.start))
        .collect()
}

#[test]
fn a_template_supplying_an_identifier_satisfies_the_role() {
    let source = "template reads { input { $orderId } }
readonly operation GetOrder reads Order {
    http GET \"/orders/{orderId}\"
    output { order: OrderData }
}";
    let found = errors(source);
    assert!(found.is_empty(), "{found:#?}");
    // Without the template the same operation is missing the identifier.
    let found = errors(&source.replace(
        "template reads { input { $orderId } }
",
        "",
    ));
    assert!(found.iter().any(|m| m.contains("`$orderId`")), "{found:#?}");
}

#[test]
fn a_template_binding_a_property_counts_for_the_lifecycle() {
    let widget = "resource Widget {
    id widgetId: OrderId
    immutable property color: Note
}
";
    let operation = "operation PutWidget puts Widget {
    http PUT \"/widgets/{widgetId}\"
    input { $widgetId }
}";
    let unset = "nothing could ever set it";
    let without = errors(&format!("{widget}{operation}"));
    assert!(without.iter().any(|m| m.contains(unset)), "{without:#?}");
    let with = errors(&format!(
        "template puts {{ input {{ $color }} }}
{widget}{operation}"
    ));
    assert!(!with.iter().any(|m| m.contains(unset)), "{with:#?}");
}

#[test]
fn restating_a_member_a_template_includes_is_reported_at_the_operation() {
    let source = "structure Trace { note?: Note }
template deletes { input { include Trace } }
operation DeleteA deletes Order {
    http DELETE \"/a/{orderId}\"
    input { $orderId note?: Note }
}";
    let text = format!(
        "{BASE}
{source}"
    );
    let at = text.find("DeleteA").unwrap() as u32;
    let found = messages_with_offsets(source);
    let twice: Vec<_> = found
        .iter()
        .filter(|(m, _)| m.contains("`note` is declared twice"))
        .collect();
    assert_eq!(twice.len(), 1, "{found:#?}");
    assert_eq!(twice[0].1, at, "{found:#?}");
}

#[test]
fn an_identifier_a_template_include_supplies_satisfies_the_role() {
    let source = "structure OrderKey for Order { $orderId }
template reads { input { include OrderKey } }
readonly operation GetOrder reads Order {
    http GET \"/orders/{orderId}\"
    output { order: OrderData }
}";
    let found = errors(source);
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_clash_between_two_template_includes_stays_at_the_template() {
    let source = "structure TraceA { trace?: Note }
structure TraceB { trace?: Note }
template deletes { input { include TraceA include TraceB } }
operation DeleteA deletes Order {
    http DELETE \"/a/{orderId}\"
    input { $orderId }
}";
    let text = format!(
        "{BASE}
{source}"
    );
    let operation = text.find("DeleteA").unwrap() as u32;
    let found = messages_with_offsets(source);
    let twice: Vec<_> = found
        .iter()
        .filter(|(m, _)| m.contains("`trace` is declared twice"))
        .collect();
    assert_eq!(twice.len(), 1, "{found:#?}");
    assert_ne!(twice[0].1, operation, "{found:#?}");
}

fn twice_at(source: &str, name: &str) -> (Vec<u32>, u32) {
    let text = format!(
        "{BASE}
{source}"
    );
    let operation = text.find("DeleteA").unwrap() as u32;
    let wanted = format!("`{name}` is declared twice");
    let at = messages_with_offsets(source)
        .into_iter()
        .filter(|(m, _)| m.contains(&wanted))
        .map(|(_, at)| at)
        .collect();
    (at, operation)
}

const TRACES: &str = "structure TraceA { trace?: Note }
structure TraceB { trace?: Note }
";

fn delete_a(template_input: &str, own: &str) -> String {
    format!(
        "{TRACES}structure Holder {{ held?: Note }}
template deletes {{ input {{ {template_input} }} }}
operation DeleteA deletes Order {{
    http DELETE \"/a/{{orderId}}\"
    input {{ $orderId {own} }}
}}"
    )
}

#[test]
fn a_typed_template_field_does_not_hide_the_operation_from_a_later_include() {
    let (at, operation) = twice_at(
        &delete_a("held?: Holder include TraceA", "trace?: Note"),
        "trace",
    );
    assert_eq!(at, [operation]);
}

#[test]
fn a_structure_a_template_field_reaches_keeps_its_own_errors() {
    let source = format!(
        "{TRACES}structure Dup {{ include TraceA include TraceB }}
template deletes {{ input {{ dup?: Dup }} }}
operation DeleteA deletes Order {{
    http DELETE \"/a/{{orderId}}\"
    input {{ $orderId trace?: Note }}
}}"
    );
    let (at, operation) = twice_at(&source, "trace");
    assert_eq!(at.len(), 1, "{at:?}");
    assert_ne!(at[0], operation);
}

#[test]
fn the_operation_and_the_template_are_each_blamed_for_their_own_repeat() {
    let (mut at, operation) = twice_at(
        &delete_a("include TraceA include TraceB", "trace?: Note"),
        "trace",
    );
    at.sort();
    assert_eq!(at.len(), 2, "{at:?}");
    assert!(at.contains(&operation), "{at:?}");
}
