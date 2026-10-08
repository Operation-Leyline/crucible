//! The stable surface, used the way a real service uses it. Each test names the
//! extension point it exists to keep compiling and behaving.

use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crucible_auth::test_support::TestTokenIssuer;
use crucible_auth::{Authenticator, JwtAuthenticator};
use crucible_server_runtime_rust::{Crucible, host};
use leyline_service::Audit;
use serde_json::{Value, json};

fn issuer() -> &'static TestTokenIssuer {
    static ISSUER: OnceLock<TestTokenIssuer> = OnceLock::new();
    ISSUER.get_or_init(|| TestTokenIssuer::new("https://issuer.example", "rsa-1").unwrap())
}

fn authenticator() -> Arc<dyn Authenticator> {
    let mut config = issuer().access_config(vec!["client".into()]);
    config.algorithms = vec![crucible_auth::Algorithm::RS256];
    Arc::new(JwtAuthenticator::with_jwks(config, issuer().jwks()).unwrap())
}

fn token(scopes: &str) -> String {
    let mut claims = issuer().access_claims("person", "client");
    claims["scope"] = json!(scopes);
    format!("Bearer {}", issuer().mint(&claims).unwrap())
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

async fn call(
    service: &Crucible,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    let response = service.handle(request.body(body).unwrap()).await;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

fn service() -> (Crucible, Audit) {
    let audit = Audit::default();
    (
        leyline_service::service(vec![authenticator()], audit.clone()),
        audit,
    )
}

#[tokio::test]
async fn public_operation_runs_the_async_middleware() {
    let (service, _) = service();
    let reply = call(&service, "GET", "/health", &[], None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["status"], "ok");
    // RequestTag is an AsyncMiddleware: set in `before`, echoed in `after`.
    assert!(
        reply.headers["x-leyline-tag"]
            .to_str()
            .unwrap()
            .starts_with("tag-")
    );
}

#[tokio::test]
async fn authenticator_chain_refuses_with_the_modelled_body() {
    let (service, audit) = service();
    for (credential, challenge) in [
        (None, "Bearer"),
        (Some("Bearer garbage"), "Bearer error=\"invalid_token\""),
    ] {
        let headers: Vec<_> = credential
            .map(|credential| ("authorization", credential))
            .into_iter()
            .collect();
        let reply = call(&service, "GET", "/graphs/g-1", &headers, None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.headers["www-authenticate"], challenge);
        assert_eq!(reply.body["__type"], "AccessDeniedException");
    }
    assert!(
        audit.entries().is_empty(),
        "refusals never reach interceptors"
    );
}

#[tokio::test]
async fn scopes_gate_operations_and_the_interceptor_observes_them() {
    let (service, audit) = service();
    let write = token("graphs:write");
    let read = token("graphs:read");
    let body = || Some(json!({ "name": "roads" }));

    let denied = call(
        &service,
        "POST",
        "/graphs",
        &[("authorization", &read)],
        body(),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    let created = call(
        &service,
        "POST",
        "/graphs",
        &[("authorization", &write)],
        body(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["name"], "roads");
    let id = created.body["graphId"].as_str().unwrap();

    let fetched = call(
        &service,
        "GET",
        &format!("/graphs/{id}"),
        &[("authorization", &read)],
        None,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body["name"], "roads");

    let missing = call(
        &service,
        "GET",
        "/graphs/nope",
        &[("authorization", &read)],
        None,
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.body["__type"], "GraphNotFound");

    assert_eq!(
        audit.entries(),
        ["CreateGraph", "GetGraph", "GetGraph"],
        "the scope refusal stopped before the audit interceptor"
    );
}

#[tokio::test]
async fn validation_exception_carries_typed_reasons() {
    let (service, _) = service();
    let write = token("graphs:write");
    let headers = [("authorization", write.as_str())];

    // Refused by the framework from the model's `@length(min: 1)`.
    let empty = call(
        &service,
        "POST",
        "/graphs",
        &headers,
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);
    assert_eq!(empty.body["__type"], "ValidationException");
    assert_eq!(empty.body["reason"], "fieldValidationFailed");
    assert_eq!(empty.body["fieldList"][0]["name"], "name");

    // Refused by the operation with the generated error type and enum.
    let blank = call(
        &service,
        "POST",
        "/graphs",
        &headers,
        Some(json!({ "name": "  " })),
    )
    .await;
    assert_eq!(blank.status, StatusCode::BAD_REQUEST);
    assert_eq!(blank.body["reason"], "fieldValidationFailed");
    assert_eq!(blank.body["fieldList"][0]["message"], "must not be blank");

    let malformed = call(&service, "POST", "/graphs", &headers, None).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed.body["reason"], "fieldValidationFailed");
}

#[tokio::test]
async fn operation_aware_middleware_runs_before_the_credential_check() {
    let (service, audit) = service();
    let read = token("graphs:read");
    let reply = call(
        &service,
        "GET",
        "/graphs/g-1",
        &[("authorization", &read), ("x-leyline-tenant", "suspended")],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(audit.entries().is_empty());
}

#[tokio::test]
async fn lambda_events_reach_the_same_pipeline() {
    let (service, _) = service();
    let direct = host::lambda_event(&service, json!({ "operation": "Health", "input": {} }))
        .await
        .unwrap();
    assert_eq!(direct["output"]["status"], "ok", "{direct}");

    let http = host::lambda_event(
        &service,
        json!({
            "version": "2.0",
            "routeKey": "$default",
            "rawPath": "/graphs/g-1",
            "rawQueryString": "",
            "headers": {},
            "requestContext": {
                "http": { "method": "GET", "path": "/graphs/g-1", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1" },
                "requestId": "leyline-event",
                "stage": "$default"
            },
            "isBase64Encoded": false
        }),
    )
    .await
    .unwrap();
    assert_eq!(http["statusCode"], 401, "{http}");
    let body: Value = serde_json::from_str(http["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["__type"], "AccessDeniedException");
}
