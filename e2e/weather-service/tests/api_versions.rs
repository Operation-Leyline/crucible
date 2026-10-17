//! Older API versions served from the current implementation
//! (docs/api-versions.md): `versioned/` is release v4 of a shop, with v1,
//! v2 and v3 frozen under `releases/`. Each older release is served as the
//! current model sees it, so what was added since reaches it (decision 51).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Unversioned};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/versioned.rs"));
include!("support/versioned_shop.rs");
use versioned_shop::{Shop, service};

async fn call(
    service: &Crucible,
    method: &str,
    uri: &str,
    version: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    send(service, method, uri, version, None).await
}

async fn send(
    service: &Crucible,
    method: &str,
    uri: &str,
    version: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(version) = version {
        request = request.header("crucible-version", version);
    }
    let body = match body {
        Some(body) => {
            request = request.header("content-type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = service.handle(request.body(body).unwrap()).await;
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

    // v1 crosses the hooks, then the rename.
    let (_, _, body) = call(&service, "GET", "/items/big", Some("v1")).await;
    assert_eq!(
        body,
        json!({ "name": "item big", "size": 9, "kind": "LARGE" })
    );

    // v3 has the new route, the label and HUGE, a size that is a number,
    // and the color v4 added to the label, which reaches it.
    let (status, _, body) = call(&service, "GET", "/things/big", Some("v3")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "label": { "title": "item big", "size": 9, "color": "red" }, "kind": "HUGE" })
    );
    // v4 is the model as it is: the size is a word.
    let (status, _, body) = call(&service, "GET", "/things/big", Some("v4")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "label": { "title": "item big", "size": "9", "color": "red" }, "kind": "HUGE" })
    );
    // Since v3 `/items/{id}` is DeleteItem's alone.
    for version in ["v3", "v4"] {
        let (status, _, _) = call(&service, "GET", "/items/big", Some(version)).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }
    // v2 has no `/things`: answered as any unknown route is.
    let (_, _, unknown) = call(&service, "GET", "/nowhere", Some("v4")).await;
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
    // Since v3 both are required.
    for version in ["v3", "v4"] {
        let (status, _, _) = call(&service, "GET", "/items?sort=name", Some(version)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (_, _, body) = call(&service, "GET", "/items?limit=5&sort=name", Some(version)).await;
        assert_eq!(body["names"], json!([version, "5", "name"]));
    }
}

#[tokio::test]
async fn a_new_enum_value_reaches_older_clients_as_one_they_know() {
    let service = service(Unversioned::Oldest);
    for (version, kinds) in [
        ("v1", json!(["SMALL", "LARGE"])),
        ("v2", json!(["SMALL", "LARGE"])),
        ("v3", json!(["SMALL", "HUGE"])),
        ("v4", json!(["SMALL", "HUGE"])),
    ] {
        let (_, _, body) = call(&service, "GET", "/items?limit=5&sort=name", Some(version)).await;
        assert_eq!(body["kinds"], kinds, "{version}: {body}");
    }
}

#[tokio::test]
async fn a_new_error_reaches_older_clients_as_one_they_know() {
    let service = service(Unversioned::Oldest);
    for version in ["v3", "v4"] {
        let (status, _, body) = call(&service, "GET", "/things/gone", Some(version)).await;
        assert_eq!(status, StatusCode::GONE, "{body}");
    }
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
    for version in ["v3", "v4"] {
        let (status, _, body) = call(&service, "DELETE", "/items/a?force=yes", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body, json!({ "deleted": "gone yes" }));
    }
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
    for version in ["v3", "v4"] {
        let (_, _, body) = call(&service, "GET", "/count", Some(version)).await;
        assert_eq!(body["reason"], "unknownOperation", "{body}");
    }
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
    let (status, _, body) = call(&service, "GET", "/items?limit=2&sort=name", Some("v4")).await;
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
    for (version, uri) in [("v1", "/items/a"), ("v4", "/things/a")] {
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
    assert!(body.to_string().contains("v1, v2, v3, v4"), "{body}");

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
    assert!(body.to_string().contains("v1, v2, v3, v4"), "{body}");
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
        json!({ "output": { "label": { "title": "item big", "size": "9", "color": "red" }, "kind": "HUGE" } })
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
        assert_eq!(response.headers()["crucible-version"], "v4");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "release": "v4" })
        );
    }
    // A client's request without a release is still the builder's to settle.
    let (status, _, _) = call(&service(Unversioned::Required), "GET", "/items/a", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A release it names is honoured: added in v3, the operation reaches v1
    // too (decision 51), and is served there.
    let response = service(Unversioned::Oldest)
        .handle(
            Request::post("/payments")
                .header("payment-signature", "t=1,v1=abc")
                .header("crucible-version", "v1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["crucible-version"], "v1");
}

#[test]
fn a_split_builder_names_the_hooks_it_is_missing() {
    let shop = Arc::new(Shop);
    let Err(missing) = shop::builder()
        .get_item(shop.clone())
        .list_items(shop.clone())
        .list_labels(shop.clone())
        .get_label(shop.clone())
        .put_label(shop.clone())
        .delete_item(shop.clone())
        .record_payment(shop.clone())
        .v1_migration(shop)
        .build()
    else {
        panic!("v2's and v3's hooks are missing");
    };
    let missing = missing.to_string();
    for expected in [
        "versions::v2::Migration::count_items",
        "versions::v2::Migration::downgrade_get_item_output",
        "versions::v2::Migration::downgrade_delete_item_output_deleted",
        "versions::v3::Migration::downgrade_label_size",
        "versions::v3::Migration::upgrade_label_size",
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
    assert_eq!(response.headers()["crucible-version"], "v4");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body["result"]["structuredContent"],
        json!({ "label": { "title": "item big", "size": "9", "color": "red" }, "kind": "HUGE" }),
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
        json!({ "label": { "title": "item big", "size": "9", "color": "red" }, "kind": "HUGE" })
    );
}

/// A generated input, from its JSON.
fn input<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

// Release v1's SDK line (decision 76): the current model as v1 sees it.
#[allow(clippy::enum_variant_names)]
mod shop_v1_client {
    include!(concat!(env!("OUT_DIR"), "/versioned_v1_client.rs"));
}

/// A client of release v1, generated with `.api_version("v1")` from the
/// current model (docs/api-versions-clients.md, 5c): v1's routes and
/// shapes, what v1 had that a later release removed, what was added since,
/// and v1 in `Crucible-Version`. The server refuses a request that names
/// no release, so every call below names v1.
#[tokio::test]
async fn a_release_v1_sdk_line_calls_the_server_at_v1() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service(Unversioned::Required),
        listener,
    ));
    let client =
        shop_v1_client::ShopClient::new(crucible_client_runtime_rust::Config::new(endpoint));
    // v1's route and names: the item's `name`, its size a number, no HUGE.
    let item = client
        .get_item(input(json!({ "id": "big" })))
        .await
        .unwrap();
    // v1's page size is `max`, and the hook supplies the sort order it
    // never sent.
    let items = client.list_items(input(json!({ "max": 1 }))).await.unwrap();
    // Removed in v3, still v1's.
    let count = client.count_items(input(json!({}))).await.unwrap();
    // Added in v3 and v4, they reach v1 in its shapes.
    let labels = client.list_labels().await.unwrap();
    let label = client
        .get_label(input(json!({ "id": "big" })))
        .await
        .unwrap();
    let gone = client
        .get_item(input(json!({ "id": "gone" })))
        .await
        .unwrap_err();
    server.abort();
    assert_eq!(
        serde_json::to_value(&item).unwrap(),
        json!({ "name": "item big", "size": 9, "kind": "LARGE" })
    );
    assert_eq!(
        serde_json::to_value(&items).unwrap(),
        json!({ "names": ["v1", "1", "newest"], "kinds": ["SMALL", "LARGE"] })
    );
    assert_eq!(serde_json::to_value(&count).unwrap(), json!({ "count": 3 }));
    assert_eq!(
        serde_json::to_value(&labels).unwrap()["kinds"],
        json!(["LARGE", "SMALL"])
    );
    assert_eq!(
        serde_json::to_value(&label).unwrap(),
        json!({ "label": { "title": "item big", "size": 9, "color": "red" } })
    );
    // v3's ItemGoneException is v1's ItemNotFoundException.
    assert!(format!("{gone:?}").contains("ItemNotFound"), "{gone:?}");
}

#[test]
fn versions_order_as_released() {
    use shop::versions::{CURRENT, V1, V2, V3, V4};
    assert!(V1 < V2 && V2 < V3 && V3 < V4);
    assert_eq!(CURRENT, V4);
}

/// ListLabels was added in v3, after v1 and v2 were released; a v1 client
/// calls it all the same (decision 51). Its response crosses every
/// migration down: v4's size word becomes the number older releases have,
/// v3's HUGE the LARGE they know, and the color v4 appended to a label
/// reaches them as it is.
#[tokio::test]
async fn an_operation_added_since_is_served_at_older_releases() {
    let service = service(Unversioned::Oldest);
    let older = json!({
        "labels": [
            { "title": "item big", "size": 9, "color": "red" },
            { "title": "item a" },
        ],
        "kinds": ["LARGE", "SMALL"],
    });
    for version in ["v1", "v2"] {
        let (status, headers, body) = call(&service, "GET", "/labels", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{version}: {body}");
        assert_eq!(headers["crucible-version"], version);
        assert_eq!(body, older, "{version}");
    }
    let (_, _, body) = call(&service, "GET", "/labels", Some("v3")).await;
    assert_eq!(body["kinds"], json!(["HUGE", "SMALL"]));
    assert_eq!(body["labels"][0]["size"], 9);
    let (_, _, body) = call(&service, "GET", "/labels", Some("v4")).await;
    assert_eq!(body["labels"][0]["size"], "9");

    // GetLabel, added in v4, reaches every release too.
    for (version, size) in [("v1", json!(9)), ("v3", json!(9)), ("v4", json!("9"))] {
        let (status, _, body) = call(&service, "GET", "/labels/big", Some(version)).await;
        assert_eq!(status, StatusCode::OK, "{version}: {body}");
        assert_eq!(
            body,
            json!({ "label": { "title": "item big", "size": size, "color": "red" } }),
            "{version}"
        );
    }
}

/// PutLabel, added in v4, takes a label: an older client sends the size as
/// the number its release has, which the migration to v4 converts up, and
/// reads it back as one. Its input reaches `Label` as no operation's did
/// when v3 was released, so the migration from v3 converts a size both ways.
#[tokio::test]
async fn an_operation_added_since_converts_its_input_up() {
    let service = service(Unversioned::Oldest);
    let sent = json!({ "label": { "title": "new", "size": 3 } });
    for version in ["v1", "v2", "v3"] {
        let (status, _, body) = send(
            &service,
            "PUT",
            "/labels/a",
            Some(version),
            Some(sent.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{version}: {body}");
        assert_eq!(
            body,
            json!({ "label": { "title": "new", "size": 3, "color": format!("{version} for a") } })
        );
    }
    let current = json!({ "label": { "title": "new", "size": "3" } });
    let (status, _, body) = send(&service, "PUT", "/labels/a", Some("v4"), Some(current)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "label": { "title": "new", "size": "3", "color": "v4 for a" } })
    );
    // An older release's number is not the current release's word.
    let (status, _, _) = send(&service, "PUT", "/labels/a", Some("v4"), Some(sent)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// `Label` is strict: the current release refuses a member it does not
/// define. Release v3's own outputs reached it, so at v3 it reads what it
/// does not know (the lenient output rule, docs/api-versions.md#limits).
/// v1 never had it: only operations added since reach it there, so v1
/// refuses an unknown member as the current release does.
#[tokio::test]
async fn a_strict_structure_its_releases_outputs_reached_reads_what_it_does_not_know() {
    let service = service(Unversioned::Oldest);
    let label = |size: Value| json!({ "label": { "title": "new", "size": size, "shelf": "top" } });
    for (version, size, status) in [
        ("v4", json!("3"), StatusCode::BAD_REQUEST),
        ("v1", json!(3), StatusCode::BAD_REQUEST),
        ("v3", json!(3), StatusCode::OK),
    ] {
        let (got, _, body) = send(
            &service,
            "PUT",
            "/labels/a",
            Some(version),
            Some(label(size)),
        )
        .await;
        assert_eq!(got, status, "{version}: {body}");
        if status == StatusCode::OK {
            assert_eq!(body["label"]["title"], "new");
            assert!(body["label"].get("shelf").is_none(), "{body}");
        }
    }
}
