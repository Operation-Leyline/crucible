//! Lambda returns at most 6 MB from a synchronous invocation. A larger answer
//! was rejected by the platform after the service had logged it as served:
//! the caller saw a bare 502 (CloudFront, a Function URL) and nothing said
//! why. The host now answers 500 in the event's shape, with the request id,
//! and logs why.

use std::sync::Arc;

use axum::body::Body;
use axum::response::Response;
use crucible_server_runtime_rust::{Crucible, host, raw_before_fn};
use serde_json::{Value, json};
use weather_service::MemoryCities;

/// The weather service, with `/big?bytes=N` answered with N bytes of JSON
/// text (or of non-UTF-8 bytes, which the event carries base64-encoded,
/// with `&binary`) by a transport middleware, so any size can be asked for.
fn service() -> Crucible {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(raw_before_fn(|job| {
            let uri = job.request().uri.clone();
            if uri.path() == "/big" {
                let query = uri.query().unwrap_or_default();
                let bytes: usize = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("bytes="))
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                let body = if query.contains("binary") {
                    vec![0xff_u8; bytes]
                } else {
                    let mut text = vec![b'x'; bytes];
                    if bytes >= 2 {
                        text[0] = b'"';
                        text[bytes - 1] = b'"';
                    }
                    text
                };
                job.respond(
                    Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                );
            }
            Ok(())
        }))
        .build()
        .unwrap()
}

fn event(query: &str) -> String {
    json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/big",
        "rawQueryString": query,
        "headers": {},
        "requestContext": {
            "http": { "method": "GET", "path": "/big", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1" },
            "requestId": "big-request",
            "stage": "$default"
        },
        "isBase64Encoded": false
    })
    .to_string()
}

async fn answer(service: &Crucible, query: &str) -> (usize, Value) {
    let answer = host::lambda_event_json(service, &event(query))
        .await
        .unwrap();
    (
        answer.get().len(),
        serde_json::from_str(answer.get()).unwrap(),
    )
}

#[tokio::test]
async fn an_http_answer_too_large_for_lambda_is_a_500_with_its_request_id() {
    let service = service();
    // 7 MB of text, and 5 MB of bytes that become ~6.7 MB of base64.
    for query in ["bytes=7340032", "bytes=5242880&binary"] {
        let (size, answer) = answer(&service, query).await;
        assert!(size < 64 * 1024, "{query}: {size} bytes");
        assert_eq!(answer["statusCode"], 500, "{query}: {answer}");
        assert_eq!(answer["headers"]["x-request-id"], "big-request", "{answer}");
        let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
        assert!(
            body["message"].as_str().unwrap().contains("request id"),
            "{body}"
        );
    }
    // Just under the limit is served as it was.
    let (size, answer) = answer(&service, "bytes=6000000").await;
    assert_eq!(answer["statusCode"], 200, "{size}");
    assert!(size > 6_000_000);
}

#[tokio::test]
async fn a_direct_invoke_answer_too_large_for_lambda_is_the_failure_envelope() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(raw_before_fn(|job| {
            // Valid JSON, so only its size can make it fail.
            let mut text = vec![b'x'; 7 * 1024 * 1024];
            text[0] = b'"';
            *text.last_mut().unwrap() = b'"';
            job.respond(Response::new(Body::from(text)));
            Ok(())
        }))
        .build()
        .unwrap();
    let event = json!({ "operation": "Ping", "input": {} }).to_string();
    let answer = host::lambda_event_json(&service, &event).await.unwrap();
    assert!(
        answer.get().len() < 64 * 1024,
        "{} bytes",
        answer.get().len()
    );
    let answer: Value = serde_json::from_str(answer.get()).unwrap();
    assert!(answer.get("error").is_some(), "{answer}");
}
