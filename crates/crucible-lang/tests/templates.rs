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
