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

/// API Gateway passes the path as it has it, which may hold what a URI does
/// not carry raw: a space, `<`, a `?` decoded from `%3F`, non-ASCII. The
/// event is answered as the same request over HTTP with each of those
/// percent-encoded, never as a failed invocation.
#[tokio::test]
async fn a_path_with_characters_a_uri_does_not_carry_is_routed_as_sent() {
    use axum::body::Body;
    use axum::http::Request;

    let service = weather_service::service(Arc::new(MemoryCities::default()));
    // Answers with the request's path and query as the service received them.
    let echo = weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(crucible_server_runtime_rust::raw_before_fn(|job| {
            let uri = job.request().uri.path_and_query().unwrap().to_string();
            job.respond(axum::response::Response::new(Body::from(uri)));
            Ok(())
        }))
        .build()
        .unwrap();
    for (raw, encoded) in [
        ("/cities/a b", "/cities/a%20b"),
        ("/cities/<b>", "/cities/%3Cb%3E"),
        ("/cities/a?b", "/cities/a%3Fb"),
        ("/cities/caf\u{e9}", "/cities/caf%C3%A9"),
        ("/pi ng", "/pi%20ng"),
    ] {
        let over_http = service
            .handle(Request::get(encoded).body(Body::empty()).unwrap())
            .await;
        let status = over_http.status().as_u16();
        let body = axum::body::to_bytes(over_http.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut body: Value = serde_json::from_slice(&body).unwrap();
        body.as_object_mut().unwrap().remove("requestId");
        let rest = json!({
            "httpMethod": "GET",
            "path": raw,
            "headers": {},
            "requestContext": { "stage": "prod", "requestId": "path-request", "httpMethod": "GET" },
            "isBase64Encoded": false
        });
        for event in [rest, v2_event("prod", raw, "")] {
            let answer = host::lambda_event(&service, event.clone())
                .await
                .unwrap_or_else(|error| panic!("{raw}: {error}"));
            assert_eq!(answer["statusCode"], status, "{raw}: {answer}");
            let mut answered: Value =
                serde_json::from_str(answer["body"].as_str().unwrap()).unwrap();
            answered.as_object_mut().unwrap().remove("requestId");
            assert_eq!(answered, body, "{raw}");
            // The service sees the whole path, encoded: a `?` in it does not
            // start a query.
            let answer = host::lambda_event(&echo, event).await.unwrap();
            assert_eq!(answer["body"], encoded, "{raw}: {answer}");
        }
    }
}
