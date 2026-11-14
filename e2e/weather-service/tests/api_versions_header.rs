//! The model's release carrier and missing-release policy over HTTP.
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Result, WithContext};
use serde_json::{Value, json};

#[allow(dead_code)]
mod named {
    include!(concat!(env!("OUT_DIR"), "/version_header_named.rs"));
}
#[allow(dead_code)]
mod required {
    include!(concat!(env!("OUT_DIR"), "/version_header_required.rs"));
}
#[allow(dead_code)]
mod named_client {
    include!(concat!(env!("OUT_DIR"), "/version_header_named_client.rs"));
}
#[allow(dead_code)]
mod required_client {
    include!(concat!(
        env!("OUT_DIR"),
        "/version_header_required_client.rs"
    ));
}

struct Reader;
impl named::probe::ReadOperation for Reader {
    async fn read(&self) -> Result<named::probe::ReadOutput> {
        Ok(named::probe::ReadOutput {
            value: Some(json!(self.context().api_version().unwrap().name())),
        })
    }
}
impl required::probe::ReadOperation for Reader {
    async fn read(&self) -> Result<required::probe::ReadOutput> {
        Ok(required::probe::ReadOutput {
            value: Some(json!(self.context().api_version().unwrap().name())),
        })
    }
}
impl required::probe::NotifyOperation for Reader {
    async fn notify(
        &self,
        _: required::probe::NotifyInput,
    ) -> Result<required::probe::NotifyOutput> {
        Ok(required::probe::NotifyOutput {
            value: Some(json!(self.context().api_version().unwrap().name())),
        })
    }
}
fn named() -> Crucible {
    Crucible::builder(named::probe::operations(Arc::new(Reader)))
        .defaults()
        .build()
        .unwrap()
}
fn required() -> Crucible {
    Crucible::builder(required::probe::operations(Arc::new(Reader)))
        .defaults()
        .build()
        .unwrap()
}
async fn call(
    service: &Crucible,
    request: axum::http::request::Builder,
) -> (StatusCode, HeaderMap, Value) {
    let response = service.handle(request.body(Body::empty()).unwrap()).await;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}
fn refused(status: StatusCode, body: &Value, header: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["reason"], "cannotParse");
    assert_eq!(body["fieldList"][0]["name"], header);
}

#[tokio::test]
async fn a_named_header_selects_and_echoes_the_requested_release() {
    let (status, headers, body) =
        call(&named(), Request::get("/read").header("acme-version", "v2")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v2");
    assert_eq!(headers["acme-version"], "v2");
    assert_eq!(headers["vary"], "acme-version");
    assert!(!headers.contains_key("crucible-version"));
}
#[tokio::test]
async fn a_named_header_selects_an_older_release() {
    let (status, headers, body) =
        call(&named(), Request::get("/read").header("acme-version", "v1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v1");
    assert_eq!(headers["acme-version"], "v1");
    assert_eq!(headers["vary"], "acme-version");
}
#[tokio::test]
async fn an_unknown_release_is_refused_at_the_named_header() {
    let (status, _, body) = call(
        &named(),
        Request::get("/read").header("acme-version", "unknown"),
    )
    .await;
    refused(status, &body, "Acme-Version");
}
#[tokio::test]
async fn no_named_header_selects_the_oldest_release() {
    let (status, headers, body) = call(&named(), Request::get("/read")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v1");
    assert_eq!(headers["acme-version"], "v1");
}
#[tokio::test]
async fn the_old_header_is_not_an_alias_for_the_named_header() {
    let (status, _, body) = call(
        &named(),
        Request::get("/read").header("crucible-version", "v2"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v1");
}
#[tokio::test]
async fn a_single_release_required_model_refuses_a_missing_header() {
    let (status, _, body) = call(&required(), Request::get("/read")).await;
    refused(status, &body, "Crucible-Version");
}
#[tokio::test]
async fn a_single_release_required_model_accepts_its_release() {
    let (status, headers, body) = call(
        &required(),
        Request::get("/read").header("crucible-version", "v1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v1");
    assert_eq!(headers["crucible-version"], "v1");
}
#[tokio::test]
async fn sender_delivery_remains_exempt_from_a_required_header() {
    let (status, _, body) = call(
        &required(),
        Request::post("/notify").header("acme-signature", "signature"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "v1");
}
#[tokio::test]
async fn the_rust_client_sends_the_named_header() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        named(),
        listener,
    ));
    let client =
        named_client::ProbeClient::new(crucible_client_runtime_rust::Config::new(endpoint));
    let result = client.read().await;
    server.abort();
    assert_eq!(
        serde_json::to_value(result.unwrap()).unwrap(),
        json!({"value": "v2"})
    );
}
#[tokio::test]
async fn the_rust_client_sends_the_release_required_by_the_model() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        required(),
        listener,
    ));
    let client =
        required_client::ProbeClient::new(crucible_client_runtime_rust::Config::new(endpoint));
    let result = client.read().await;
    server.abort();
    assert_eq!(
        serde_json::to_value(result.unwrap()).unwrap(),
        json!({"value": "v1"})
    );
}

#[tokio::test]
async fn direct_invocation_keeps_the_current_release_under_required() {
    assert_eq!(
        required().invoke("Read", json!({})).await,
        json!({"output": {"value": "v1"}})
    );
}

#[tokio::test]
async fn mcp_keeps_the_current_release_and_the_named_response_header() {
    let service = Crucible::builder(named::probe::operations(Arc::new(Reader)))
        .defaults()
        .mcp(crucible_server_runtime_rust::mcp::McpConfig::new())
        .build()
        .unwrap();
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("acme-version", "v1")
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "Read", "arguments": {}}})
            .to_string(),
        ))
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["acme-version"], "v2");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["structuredContent"]["value"], "v2");
}

#[tokio::test]
async fn mcp_is_exempt_from_the_models_required_release_policy() {
    let service = Crucible::builder(required::probe::operations(Arc::new(Reader)))
        .defaults()
        .mcp(crucible_server_runtime_rust::mcp::McpConfig::new())
        .build()
        .unwrap();
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "Read", "arguments": {}}})
            .to_string(),
        ))
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["result"]["structuredContent"]["value"], "v1");
}

async fn explorer_case(pattern: &'static str) {
    let service = Crucible::builder(required::probe::operations(Arc::new(Reader)))
        .defaults()
        .explorer(crucible_server_runtime_rust::explorer::Explorer::new(
            required::probe::DESCRIPTION,
        ))
        .build()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg("--test")
            .arg("--test-reporter=tap")
            .arg("--test-name-pattern")
            .arg(pattern)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/crucible-server-runtime-rust/tests/explorer/version-header.test.mjs"
            ))
            .env("EXPLORER_VERSION_URL", endpoint)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("# pass 1\n"),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_request_honours_the_required_release_policy() {
    explorer_case("^Try it succeeds against a service requiring its release header$").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_sender_request_names_the_current_release() {
    explorer_case("^Try it preserves sender requests while naming the current release$").await;
}
