//! Older API versions served from the current implementation
//! (docs/api-versions.md): `versioned/` is release v3 of a shop, with v1
//! and v2 frozen under `releases/`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Result, Unversioned, WithContext};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/versioned.rs"));
use shop::versions::{v1, v2};
use shop::{
    DeleteItemInput, DeleteItemOperation, DeleteItemOutput, GetItemInput, GetItemOperation,
    GetItemOutput, ItemGoneException, Kind, Label, ListItemsInput, ListItemsOperation,
    ListItemsOutput, RecordPaymentInput, RecordPaymentOperation, RecordPaymentOutput,
};

struct Shop;

impl GetItemOperation for Shop {
    async fn get_item(&self, input: GetItemInput) -> Result<GetItemOutput> {
        if input.id == "gone" {
            return Err(ItemGoneException {
                message: "sold out".to_owned(),
            }
            .into());
        }
        Ok(GetItemOutput {
            label: Label {
                title: format!("item {}", input.id),
                size: (input.id == "big").then_some(9),
            },
            kind: if input.id == "big" {
                Kind::Huge
            } else {
                Kind::Small
            },
        })
    }
}

impl ListItemsOperation for Shop {
    async fn list_items(&self, input: ListItemsInput) -> Result<ListItemsOutput> {
        // The version the request is served at, and what the
        // implementation sees.
        let version = self.context().api_version().map(|v| v.name());
        Ok(ListItemsOutput {
            names: vec![
                version.unwrap_or("none").to_owned(),
                input.limit.to_string(),
                input.sort,
            ],
            kinds: vec![Kind::Small, Kind::Huge],
        })
    }
}

impl DeleteItemOperation for Shop {
    async fn delete_item(&self, input: DeleteItemInput) -> Result<DeleteItemOutput> {
        // v3's force is a word; older clients sent a boolean.
        Ok(DeleteItemOutput {
            deleted: format!("gone {}", input.force.as_deref().unwrap_or("-")),
        })
    }
}

impl RecordPaymentOperation for Shop {
    async fn record_payment(&self, input: RecordPaymentInput) -> Result<RecordPaymentOutput> {
        // The handler verifies the signature; here, that it arrived.
        assert_eq!(input.signature, "t=1,v1=abc");
        let version = self.context().api_version().map(|v| v.name());
        Ok(RecordPaymentOutput {
            release: version.unwrap_or("none").to_owned(),
        })
    }
}

/// The migration to v2 needs the sort order an older request did not send.
impl v1::Migration for Shop {
    async fn require_list_items_input_sort(&self, request: v1::ListItemsInput) -> Result<String> {
        // Such as the account's saved preference; here, from the request,
        // as v1 sent it (`max`, renamed in the same migration).
        match request.max {
            Some(0) => Err(crucible_server_runtime_rust::Error::internal(
                "no preference",
            )),
            Some(1) => Ok("newest".to_owned()),
            _ => Ok("name".to_owned()),
        }
    }
}

/// The migration to v3 regrouped GetItem's output, retyped `deleted`, and
/// removed CountItems.
impl v2::Migration for Shop {
    fn downgrade_get_item_output(&self, output: GetItemOutput) -> Result<v2::GetItemOutput> {
        Ok(v2::GetItemOutput {
            title: output.label.title,
            size: output.label.size,
            // The hook owns the whole output, the new kind included.
            kind: match output.kind {
                Kind::Small => v2::Kind::Small,
                Kind::Large | Kind::Huge => v2::Kind::Large,
            },
        })
    }

    fn upgrade_delete_item_input_force(&self, force: bool) -> Result<String> {
        Ok(if force { "yes" } else { "no" }.to_owned())
    }

    fn downgrade_delete_item_output_deleted(&self, deleted: String) -> Result<bool> {
        Ok(deleted == "gone yes")
    }

    async fn count_items(&self, input: v2::CountItemsInput) -> Result<v2::CountItemsOutput> {
        // A removed operation's handler answers with its own release's errors.
        if input.shelf.as_deref() == Some("closed") {
            return Err(v2::CountUnavailableException {
                message: "closed".to_owned(),
            }
            .into());
        }
        Ok(v2::CountItemsOutput { count: 3 })
    }
}

fn service(unversioned: Unversioned) -> Crucible {
    Crucible::builder(shop::operations(Arc::new(Shop)))
        .defaults()
        .api_version(unversioned)
        .build()
        .unwrap()
}

async fn call(
    service: &Crucible,
    method: &str,
    uri: &str,
    version: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(version) = version {
        request = request.header("crucible-version", version);
    }
    let response = service.handle(request.body(Body::empty()).unwrap()).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, headers, body)
}

#[tokio::test]
async fn each_version_reads_and_answers_in_its_own_shapes() {
    let service = service(Unversioned::Oldest);

    // v1 knows `name`, and its old route.
    let (status, headers, body) = call(&service, "GET", "/items/a", Some("v1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "name": "item a", "kind": "SMALL" }));
    assert_eq!(headers["crucible-version"], "v1");
    assert_eq!(headers["sunset"], "Fri, 01 Jan 2027 00:00:00 GMT");
    assert_eq!(headers["vary"], "crucible-version");
    // RFC 9745's `Deprecation` is a date the model does not record.
    assert!(headers.get("deprecation").is_none());

    // v2 knows `title` and `size` side by side (the reshape hook), at the
    // old route, and no HUGE.
    let (status, headers, body) = call(&service, "GET", "/items/big", Some("v2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "title": "item big", "size": 9, "kind": "LARGE" })
    );
    assert!(headers.get("sunset").is_none());

    // v1 crosses the hook, then the rename.
    let (_, _, body) = call(&service, "GET", "/items/big", Some("v1")).await;
    assert_eq!(
        body,
        json!({ "name": "item big", "size": 9, "kind": "LARGE" })
    );

    // v3 is the model as it is: the new route, the label, HUGE.
    let (status, _, body) = call(&service, "GET", "/things/big", Some("v3")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "label": { "title": "item big", "size": 9 }, "kind": "HUGE" })
    );
    // At v3 `/items/{id}` is DeleteItem's alone.
    let (status, _, _) = call(&service, "GET", "/items/big", Some("v3")).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    // v2 has no `/things`: answered as any unknown route is.
    let (_, _, unknown) = call(&service, "GET", "/nowhere", Some("v3")).await;
    let (_, _, body) = call(&service, "GET", "/things/big", Some("v2")).await;
    assert_eq!(body["reason"], "unknownOperation", "{body}");
    assert_eq!(body["reason"], unknown["reason"]);
}

#[tokio::test]
async fn an_older_request_gets_the_values_it_could_not_send() {
    let service = service(Unversioned::Oldest);
    // v2 sends a sort order but no page size: the default fills it.
    let (status, _, body) = call(&service, "GET", "/items?sort=price", Some("v2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["names"], json!(["v2", "20", "price"]));
    // v1 may send neither: the async hook supplies the sort order.
    // v1 may send neither, and calls the page size `max`: the rename and
    // the async hook run in the same migration.
    let (_, _, body) = call(&service, "GET", "/items?max=5", Some("v1")).await;
    assert_eq!(body["names"], json!(["v1", "5", "name"]));
    let (_, _, body) = call(&service, "GET", "/items?max=1", Some("v1")).await;
    assert_eq!(body["names"], json!(["v1", "1", "newest"]));
    // A hook that fails fails the request.
    let (status, _, _) = call(&service, "GET", "/items?max=0", Some("v1")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    // What a v1 request did send is kept.
    let (_, _, body) = call(&service, "GET", "/items?sort=size", Some("v1")).await;
    assert_eq!(body["names"], json!(["v1", "20", "size"]));
    // At v3 both are required.
    let (status, _, _) = call(&service, "GET", "/items?sort=name", Some("v3")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, _, body) = call(&service, "GET", "/items?limit=5&sort=name", Some("v3")).await;
    assert_eq!(body["names"], json!(["v3", "5", "name"]));
}

#[tokio::test]
async fn a_new_enum_value_reaches_older_clients_as_one_they_know() {
    let service = service(Unversioned::Oldest);
    for (version, kinds) in [
        ("v1", json!(["SMALL", "LARGE"])),
        ("v2", json!(["SMALL", "LARGE"])),
        ("v3", json!(["SMALL", "HUGE"])),
    ] {
        let (_, _, body) = call(&service, "GET", "/items?limit=5&sort=name", Some(version)).await;
        assert_eq!(body["kinds"], kinds, "{version}: {body}");
    }
}

#[tokio::test]
async fn a_new_error_reaches_older_clients_as_one_they_know() {
    let service = service(Unversioned::Oldest);
    let (status, _, body) = call(&service, "GET", "/things/gone", Some("v3")).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    for version in ["v1", "v2"] {
        let (status, _, body) = call(&service, "GET", "/items/gone", Some(version)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(
            body,
            json!({ "__type": "ItemNotFoundException", "message": "sold out" })
        );
    }
}

#[tokio::test]
async fn a_retyped_member_is_converted_by_its_hook() {
    let service = service(Unversioned::Oldest);
    // Upgraded on the way in (bool to word), downgraded on the way out.
    for version in ["v1", "v2"] {
        let (status, _, body) =
            call(&service, "DELETE", "/items/a?force=true", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, json!({ "deleted": true }));
        let (_, _, body) = call(&service, "DELETE", "/items/a?force=false", Some(version)).await;
        assert_eq!(body, json!({ "deleted": false }));
    }
    let (status, _, body) = call(&service, "DELETE", "/items/a?force=yes", Some("v3")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "deleted": "gone yes" }));
}

#[tokio::test]
async fn a_removed_operation_keeps_serving_older_versions() {
    let service = service(Unversioned::Oldest);
    for version in ["v1", "v2"] {
        let (status, _, body) = call(&service, "GET", "/count", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, json!({ "count": 3 }));
    }
    for version in ["v1", "v2"] {
        let (status, _, body) = call(&service, "GET", "/count?shelf=closed", Some(version)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(
            body,
            json!({ "__type": "CountUnavailableException", "message": "closed" })
        );
    }
    let (_, _, body) = call(&service, "GET", "/count", Some("v3")).await;
    assert_eq!(body["reason"], "unknownOperation", "{body}");
}

/// An operation-aware extension prepares a plan per operation, older
/// versions' included: authentication on a service that removed an
/// operation still serves it to the clients that call it.
#[tokio::test]
async fn a_removed_operation_passes_operation_aware_extensions() {
    let service = Crucible::builder(shop::operations(Arc::new(Shop)))
        .defaults()
        .api_version(Unversioned::Oldest)
        .operation_middleware(crucible_auth::AuthenticatorChain::new(Vec::new()))
        .build()
        .unwrap();
    for version in ["v1", "v2"] {
        let (status, _, body) = call(&service, "GET", "/count", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{version}: {body}");
        assert_eq!(body, json!({ "count": 3 }));
    }
    let (status, _, body) = call(&service, "GET", "/items?limit=2&sort=name", Some("v3")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// An extension scoped by a member a later release renamed runs for the
/// older version only; it still prepares a plan for that version's
/// requests, though the current version is outside its scope.
#[tokio::test]
async fn an_extension_scoped_to_an_older_description_prepares_for_it() {
    use crucible_server_runtime_rust::OperationMiddlewareExt;

    let service = Crucible::builder(shop::operations(Arc::new(Shop)))
        .defaults()
        .api_version(Unversioned::Oldest)
        .operation_middleware(
            crucible_auth::AuthenticatorChain::new(Vec::new())
                // v1's GetItem answers `name`, which v2 renamed `title`.
                .when(|op| op.output.iter().any(|member| member.name == "name")),
        )
        .build()
        .unwrap();
    for (version, uri) in [("v1", "/items/a"), ("v3", "/things/a")] {
        let (status, _, body) = call(&service, "GET", uri, Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{version}: {body}");
    }
}

#[tokio::test]
async fn a_request_without_a_version_is_settled_by_the_builder() {
    // By default: the oldest supported version.
    let (_, headers, body) = call(&service(Unversioned::Oldest), "GET", "/items/a", None).await;
    assert_eq!(headers["crucible-version"], "v1");
    assert_eq!(body["name"], "item a");

    // Required: refused, naming the versions served.
    let (status, _, body) = call(&service(Unversioned::Required), "GET", "/items/a", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("v1, v2, v3"), "{body}");

    // A resolver: such as the version an account first used.
    let resolved = service(Unversioned::Resolve(Arc::new(|_| Some("v2".to_owned()))));
    let (_, headers, _) = call(&resolved, "GET", "/items/a", None).await;
    assert_eq!(headers["crucible-version"], "v2");

    // An unknown version is refused, whatever the builder says, naming the
    // versions served but not the client's text, which could be any length.
    let unknown = "v9-".repeat(1000);
    let (status, _, body) = call(
        &service(Unversioned::Oldest),
        "GET",
        "/items/a",
        Some(&unknown),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("v1, v2, v3"), "{body}");
    assert!(!body.to_string().contains("v9"), "{body}");
}

#[tokio::test]
async fn a_direct_invocation_is_served_at_the_current_version() {
    // Its caller holds the current model; the default would be the oldest.
    let answer = service(Unversioned::Required)
        .invoke("GetItem", json!({ "id": "big" }))
        .await;
    assert_eq!(
        answer,
        json!({ "output": { "label": { "title": "item big", "size": 9 }, "kind": "HUGE" } })
    );
}

/// A sender-signed operation (decision 81) is called by a third party that
/// names no release: it is served at the current release, even where a
/// client's request without one is refused or served at the oldest.
#[tokio::test]
async fn a_sender_signed_request_is_served_at_the_current_release() {
    for unversioned in [Unversioned::Oldest, Unversioned::Required] {
        let response = service(unversioned)
            .handle(
                Request::post("/payments")
                    .header("payment-signature", "t=1,v1=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["crucible-version"], "v3");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "release": "v3" })
        );
    }
    // A client's request without a release is still the builder's to settle.
    let (status, _, _) = call(&service(Unversioned::Required), "GET", "/items/a", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A release it names is honoured: v1 has no such operation.
    let (status, _, _) = call(&service(Unversioned::Oldest), "POST", "/payments", Some("v1")).await;
    assert_ne!(status, StatusCode::OK);
}

#[test]
fn a_split_builder_names_the_hooks_it_is_missing() {
    let shop = Arc::new(Shop);
    let Err(missing) = shop::builder()
        .get_item(shop.clone())
        .list_items(shop.clone())
        .delete_item(shop.clone())
        .record_payment(shop.clone())
        .v1_migration(shop)
        .build()
    else {
        panic!("v2's hooks are missing");
    };
    let missing = missing.to_string();
    for expected in [
        "versions::v2::Migration::count_items",
        "versions::v2::Migration::downgrade_get_item_output",
        "versions::v2::Migration::downgrade_delete_item_output_deleted",
    ] {
        assert!(missing.contains(expected), "{missing}");
    }
}

#[tokio::test]
async fn an_mcp_tool_call_is_served_at_the_current_version() {
    // Tools are described from the current model, whatever a request names.
    let service = Crucible::builder(shop::operations(Arc::new(Shop)))
        .defaults()
        .mcp(crucible_server_runtime_rust::mcp::McpConfig::new())
        .api_version(Unversioned::Required)
        .build()
        .unwrap();
    let message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "GetItem", "arguments": { "id": "big" } },
    });
    let response = service
        .handle(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("crucible-version", "v1")
                .body(Body::from(message.to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["crucible-version"], "v3");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body["result"]["structuredContent"],
        json!({ "label": { "title": "item big", "size": 9 }, "kind": "HUGE" }),
        "{body}"
    );
}

// Every error in this model ends in `Exception`, which the generated error
// enums repeat.
#[allow(clippy::enum_variant_names)]
mod shop_client {
    include!(concat!(env!("OUT_DIR"), "/versioned_client.rs"));
}

#[tokio::test]
async fn a_generated_client_names_its_release() {
    // The server serves a request that names no release at the oldest one, so
    // a client that did not send its own would read release 1's shapes.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service(Unversioned::Oldest),
        listener,
    ));
    let client = shop_client::ShopClient::new(crucible_client_runtime_rust::Config::new(endpoint));
    let input =
        serde_json::from_value::<shop_client::GetItemInput>(json!({ "id": "big" })).unwrap();
    let output = client.get_item(input).await.unwrap();
    server.abort();
    assert_eq!(
        serde_json::to_value(&output).unwrap(),
        json!({ "label": { "title": "item big", "size": 9 }, "kind": "HUGE" })
    );
}

#[test]
fn versions_order_as_released() {
    use shop::versions::{CURRENT, V1, V2, V3};
    assert!(V1 < V2 && V2 < V3);
    assert_eq!(CURRENT, V3);
}
