//! Where a request's id and deadline come from: the service's own source,
//! then the host (a Lambda invocation, or an HTTP event's request context),
//! then a fresh id.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderValue, Request};
use crucible::handlers::before_fn;
use crucible::{Crucible, RequestMeta};
use serde_json::json;
use weather_service::MemoryCities;

type Seen = Arc<Mutex<Vec<(String, Option<Instant>)>>>;

/// The weather service with a handler recording each request's id and
/// deadline as the context holds them.
fn recording(source: Option<crucible::service::RequestIdSource>) -> (Crucible, Seen) {
    let seen: Seen = Arc::default();
    let record = seen.clone();
    let mut builder = weather_service::builder(Arc::new(MemoryCities::default()));
    if let Some(source) = source {
        builder = builder.request_id(source);
    }
    let service = builder
        .handler(before_fn(move |job| {
            let context = job.context();
            record
                .lock()
                .unwrap()
                .push((context.request_id().to_owned(), context.deadline()));
            Ok(())
        }))
        .build()
        .unwrap();
    (service, seen)
}

fn http_event(request_id: &str) -> String {
    json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/ping",
        "rawQueryString": "",
        "headers": {},
        "requestContext": {
            "accountId": "123456789012",
            "apiId": "api",
            "domainName": "example.test",
            "domainPrefix": "example",
            "http": { "method": "GET", "path": "/ping", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1", "userAgent": "test" },
            "requestId": request_id,
            "routeKey": "$default",
            "stage": "$default",
            "time": "04/Oct/2026:12:00:00 +0000",
            "timeEpoch": 1790000000000_i64
        },
        "isBase64Encoded": false
    })
    .to_string()
}

fn meta(id: &str, deadline: Instant) -> RequestMeta {
    RequestMeta {
        request_id: Some(HeaderValue::from_str(id).unwrap()),
        deadline: Some(deadline),
    }
}

#[tokio::test]
async fn an_http_event_keeps_its_own_request_id_and_takes_the_invocation_deadline() {
    let (service, seen) = recording(None);
    let deadline = Instant::now() + Duration::from_secs(5);
    let answer = crucible::host::lambda_event_json_with(
        &service,
        &http_event("event-id-1"),
        meta("invocation-1", deadline),
    )
    .await
    .unwrap();
    let answer: serde_json::Value = serde_json::from_str(answer.get()).unwrap();
    assert_eq!(answer["headers"]["x-request-id"], "event-id-1", "{answer}");
    assert_eq!(
        *seen.lock().unwrap(),
        [("event-id-1".to_owned(), Some(deadline))]
    );
}

#[tokio::test]
async fn a_direct_invoke_takes_the_invocation_id_and_deadline() {
    let (service, seen) = recording(None);
    let deadline = Instant::now() + Duration::from_secs(5);
    let event = json!({ "operation": "Ping", "input": {} }).to_string();
    crucible::host::lambda_event_json_with(&service, &event, meta("invocation-2", deadline))
        .await
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [("invocation-2".to_owned(), Some(deadline))]
    );
}

fn edge_id(parts: &axum::http::request::Parts) -> Option<HeaderValue> {
    parts.headers.get("x-edge-id").cloned()
}

#[tokio::test]
async fn the_services_own_source_comes_first_and_bad_ids_are_replaced() {
    let (service, seen) = recording(Some(edge_id));
    let request = |id: &str| {
        Request::get("/ping")
            .header("x-edge-id", id)
            .body(Body::empty())
            .unwrap()
    };
    let response = service.handle(request("edge-1")).await;
    assert_eq!(response.headers()["x-request-id"], "edge-1");

    let response = service.handle(request(&"x".repeat(129))).await;
    let replaced = response.headers()["x-request-id"].to_str().unwrap();
    assert_eq!(replaced.len(), 36, "a fresh UUID instead of an overlong id");

    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0],
        ("edge-1".to_owned(), None),
        "no deadline over plain HTTP"
    );
}
