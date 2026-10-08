use std::sync::Arc;

use crucible_server_runtime_rust::host;
use serde_json::{Value, json};
use weather_service::MemoryCities;

fn v2_event(stage: &str, path: &str, query: &str) -> Value {
    json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": path,
        "rawQueryString": query,
        "headers": {},
        "requestContext": {
            "http": { "method": "GET", "path": path, "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1" },
            "requestId": "stage-request",
            "stage": stage
        },
        "isBase64Encoded": false
    })
}

async fn assert_routes(event: Value) {
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    let answer = host::lambda_event(&service, event).await.unwrap();
    assert_eq!(answer["statusCode"], 200, "{answer}");
}

#[tokio::test]
async fn lambda_rest_v1_routes_without_prod_stage_prefix() {
    assert_routes(json!({
        "httpMethod": "GET",
        "path": "/ping",
        "headers": {},
        "requestContext": { "stage": "prod", "requestId": "stage-request", "httpMethod": "GET" },
        "isBase64Encoded": false
    }))
    .await;
}

#[tokio::test]
async fn lambda_http_v2_routes_without_named_stage_prefix() {
    assert_routes(v2_event("prod", "/ping", "")).await;
}

#[tokio::test]
async fn lambda_http_v2_default_and_named_stages_preserve_query_bindings() {
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    for stage in ["$default", "prod"] {
        let answer = host::lambda_event(&service, v2_event(stage, "/cities", "maxResults=many"))
            .await
            .unwrap();
        assert_eq!(answer["statusCode"], 400, "{answer}");
        let body: Value = serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["reason"], "cannotParse");
    }
}
