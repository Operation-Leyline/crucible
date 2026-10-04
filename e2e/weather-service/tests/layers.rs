//! Layers wrap the whole service on every host: the same compression and the
//! same outer observer over HTTP, Lambda HTTP events and direct invokes.

use std::io::Read;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, Response};
use base64::Engine;
use crucible::Crucible;
use serde_json::{Value, json};
use tower::util::MapResponseLayer;
use tower_http::compression::CompressionLayer;
use weather_service::MemoryCities;

/// The weather service with an observer outside compression, recording the
/// `content-encoding` of every response as it leaves.
fn layered() -> (Crucible, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .layer(MapResponseLayer::new(move |response: Response<Body>| {
            let encoding = response
                .headers()
                .get("content-encoding")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("identity");
            record.lock().unwrap().push(encoding.to_owned());
            response
        }))
        .layer(CompressionLayer::new())
        .build()
        .unwrap();
    (service, seen)
}

fn gunzip(bytes: &[u8]) -> Value {
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn http_responses_are_compressed_errors_included() {
    let (service, seen) = layered();
    let request = Request::get("/cities/c-99999999")
        .header("accept-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.headers()["content-encoding"], "gzip");
    assert!(response.headers().contains_key("x-request-id"));
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = gunzip(&bytes);
    assert_eq!(body["__type"], "NotFoundError", "{body}");
    assert_eq!(body["resourceId"], "c-99999999");
    assert_eq!(
        *seen.lock().unwrap(),
        ["gzip"],
        "the outer layer sees the response as sent"
    );
}

#[tokio::test]
async fn lambda_http_events_run_the_same_layers() {
    let (service, seen) = layered();
    let event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/cities/c-99999999",
        "rawQueryString": "",
        "headers": { "accept-encoding": "gzip" },
        "requestContext": {
            "accountId": "123456789012",
            "apiId": "api",
            "domainName": "example.test",
            "domainPrefix": "example",
            "http": { "method": "GET", "path": "/cities/c-99999999", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1", "userAgent": "test" },
            "requestId": "id",
            "routeKey": "$default",
            "stage": "$default",
            "time": "04/Oct/2026:12:00:00 +0000",
            "timeEpoch": 1790000000000_i64
        },
        "isBase64Encoded": false
    });
    let answered = crucible::host::lambda_event(&service, event).await.unwrap();
    assert_eq!(answered["statusCode"], 404, "{answered}");
    assert_eq!(
        answered["headers"]["content-encoding"], "gzip",
        "{answered}"
    );
    assert_eq!(
        answered["isBase64Encoded"], true,
        "a compressed body is binary"
    );
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(answered["body"].as_str().unwrap())
        .unwrap();
    assert_eq!(gunzip(&bytes)["__type"], "NotFoundError");
    assert_eq!(*seen.lock().unwrap(), ["gzip"]);
}

#[tokio::test]
async fn direct_invokes_pass_through_the_layers_as_json() {
    let (service, seen) = layered();
    let answer = service
        .invoke("CreateCity", json!({ "name": "Quito" }))
        .await;
    assert_eq!(answer["output"]["city"]["name"], "Quito", "{answer}");
    assert_eq!(
        *seen.lock().unwrap(),
        ["identity"],
        "no accept-encoding, so nothing to compress"
    );
}

#[tokio::test]
async fn a_service_without_layers_answers_as_before() {
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    let request = Request::get("/ping")
        .header("accept-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key("content-encoding"));
}
