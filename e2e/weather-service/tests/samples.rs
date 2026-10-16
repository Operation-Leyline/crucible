//! The API reference's generated samples, sent to the service: each
//! operation's sample request, full and minimal, passes the server's own
//! validation (docs/docs.md, "Generated samples").

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::Request;
use base64::Engine as _;
use crucible_server_runtime_rust::Crucible;
use crucible_server_runtime_rust::explorer::Explorer;
use serde_json::Value;
use weather_service::{MemoryCities, weather};

fn service() -> Crucible {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .explorer(Explorer::new(weather::DESCRIPTION))
        .build()
        .unwrap()
}

/// The description the Explorer serves, which carries the samples.
async fn description(service: &Crucible) -> Value {
    let mut request = Request::get("/explorer/describe.json")
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert("127.0.0.1:1".parse::<SocketAddr>().unwrap());
    let response = service.handle(request).await;
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

/// A sample's input message as an HTTP request.
fn request(message: &Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(message["method"].as_str().unwrap())
        .uri(message["uri"].as_str().unwrap());
    for (name, value) in message["headers"].as_object().unwrap() {
        builder = builder.header(name, value.as_str().unwrap());
    }
    let payload = &message["payload"];
    let body = if !payload.is_null() {
        let value = &payload["value"];
        if payload["encoding"] == "base64" {
            base64::engine::general_purpose::STANDARD
                .decode(value.as_str().unwrap())
                .unwrap()
        } else if let Some(text) = value.as_str() {
            text.as_bytes().to_vec()
        } else {
            serde_json::to_vec(value).unwrap()
        }
    } else if message["body"].is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&message["body"]).unwrap()
    };
    builder.body(Body::from(body)).unwrap()
}

#[tokio::test]
async fn every_sample_request_passes_the_servers_validation() {
    let service = service();
    let description = description(&service).await;
    let operations = description["operations"].as_object().unwrap();
    assert!(!operations.is_empty());
    let mut sent = 0;
    for (id, operation) in operations {
        let sample = &operation["sample"];
        assert_eq!(sample["fallbacks"], Value::Array(Vec::new()), "{id}");
        for key in ["input", "minimalInput"] {
            let response = service.handle(request(&sample[key])).await;
            let status = response.status();
            let error = response
                .headers()
                .get("x-crucible-error")
                .map(|v| v.to_str().unwrap().to_owned());
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            // The service may refuse a sample for its own reasons (no such
            // city, no credentials); never because it breaks the model.
            assert!(
                status != 400 && error.as_deref() != Some("ValidationError"),
                "{id} {key}: {status} {error:?} {}\n{}",
                String::from_utf8_lossy(&body),
                sample[key]
            );
            sent += 1;
        }
    }
    assert_eq!(sent, operations.len() * 2);
}
