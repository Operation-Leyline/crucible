use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::{
    Crucible, Hook, JsonProtocol, Operation, OperationAwareMiddleware, OperationJob,
};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/audience_server.rs"));
}
use generated::api;

struct Handlers;
impl api::PublicOperation for Handlers {
    async fn public(
        &self,
        input: api::PublicInput,
    ) -> crucible_server_runtime_rust::Result<api::PublicOutput> {
        if input.value.as_deref() == Some("internal-error") {
            return Err(api::InternalFault {
                message: Some("internal detail".into()),
            }
            .into());
        }
        let private_value = if input.value.as_deref() == Some("force-private") {
            Some("server secret".into())
        } else {
            input.private_value
        };
        Ok(api::PublicOutput {
            value: input.value,
            private_value,
            choice: None,
        })
    }
}
impl api::SecretOperation for Handlers {
    async fn secret(&self, input: api::SecretInput) -> crucible_server_runtime_rust::Result<()> {
        assert_eq!(input.secret.as_deref(), Some("secret header"));
        Ok(())
    }
}

// An application decides how callers become internal. The framework only
// exposes the lowered annotation; it has no built-in caller authorization.
struct InternalCallers;

#[tokio::test]
async fn explorer_description_defaults_to_internal_audience() {
    use crucible_server_runtime_rust::explorer::Explorer;
    let service = Crucible::builder(api::operations(Arc::new(Handlers)))
        .on_rejection(|rejection| {
            Box::new(api::PublicFault {
                message: Some(rejection.summary()),
            })
        })
        .protocol(JsonProtocol)
        .explorer(Explorer::new(api::DESCRIPTION).allow_remote())
        .build()
        .unwrap();
    let response = service
        .handle(
            Request::get("/explorer/describe.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        body["operations"]
            .as_object()
            .unwrap()
            .values()
            .any(|op| op["name"] == "Secret" && op["internal"] == true)
    );
}
impl OperationAwareMiddleware for InternalCallers {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let internal_caller = job
            .context()
            .headers()
            .get("x-example-caller")
            .is_some_and(|v| v == "internal");
        if job.operation().internal && !internal_caller {
            Hook::Ready(Err(api::PublicFault {
                message: Some("internal caller required".into()),
            }
            .into()))
        } else {
            Hook::Ready(Ok(()))
        }
    }
}

#[tokio::test]
async fn mcp_uses_external_projection_and_hides_internal_operations_and_members() {
    use crucible_server_runtime_rust::mcp::McpConfig;
    use serde_json::{Value, json};
    let service = Crucible::builder(api::operations(Arc::new(Handlers)))
        .on_rejection(|rejection| {
            Box::new(api::PublicFault {
                message: Some(rejection.summary()),
            })
        })
        .mcp(McpConfig::new().allowed_origins(["https://example.com"]))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    for (method, params) in [
        ("tools/list", json!({})),
        ("tools/call", json!({"name":"Secret","arguments":{}})),
        (
            "tools/call",
            json!({"name":"Public","arguments":{"value":"force-private","privateValue":"must not enter"}}),
        ),
        (
            "tools/call",
            json!({"name":"Public","arguments":{"value":"internal-error"}}),
        ),
    ] {
        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
            ))
            .unwrap();
        let response = service.handle(request).await;
        assert_eq!(response.status().as_u16(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        if method == "tools/list" {
            assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 1);
            assert_eq!(response["result"]["tools"][0]["name"], "Public");
            assert!(!String::from_utf8_lossy(&bytes).contains("privateValue"));
        } else if params["name"] == "Secret" {
            assert_eq!(response["error"]["code"], -32602);
        } else if params["arguments"]["value"] == "internal-error" {
            assert_eq!(response["error"]["code"], -32603);
            assert!(!String::from_utf8_lossy(&bytes).contains("internal detail"));
        } else {
            let output = &response["result"]["structuredContent"];
            assert_eq!(output["value"], "force-private");
            assert!(output.get("privateValue").is_none());
        }
    }
    let request = Request::post("/mcp")
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from("bad JSON"))
        .unwrap();
    assert_eq!(service.handle(request).await.status().as_u16(), 403);
}

#[tokio::test]
async fn mcp_endpoint_guard_answers_before_credentials_for_an_allowed_origin() {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use crucible_server_runtime_rust::mcp::{EndpointAdmission, McpConfig};

    let service = Crucible::builder(api::operations(Arc::new(Handlers)))
        .on_rejection(|rejection| {
            Box::new(api::PublicFault {
                message: Some(rejection.summary()),
            })
        })
        .mcp(
            McpConfig::new()
                .allowed_origins(["https://example.com"])
                .endpoint_guard(|parts: &axum::http::request::Parts| {
                    if parts.headers.contains_key("authorization") {
                        EndpointAdmission::Admit
                    } else {
                        EndpointAdmission::Respond(StatusCode::UNAUTHORIZED.into_response())
                    }
                }),
        )
        .protocol(JsonProtocol)
        .build()
        .unwrap();

    let request = Request::post("/mcp")
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from("bad JSON"))
        .unwrap();
    assert_eq!(
        service.handle(request).await.status(),
        StatusCode::FORBIDDEN
    );
    for method in ["GET", "POST", "OPTIONS"] {
        let request = Request::builder()
            .method(method)
            .uri("/mcp")
            .header("origin", "https://example.com")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            service.handle(request).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let request = Request::post("/mcp")
        .header("origin", "https://example.com")
        .header("authorization", "Bearer good")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
        .unwrap();
    assert_eq!(service.handle(request).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn middleware_uses_generated_visibility_and_server_retains_every_operation() {
    assert!(api::Secret::INFO.internal);
    assert!(!api::Public::INFO.internal);
    assert_eq!(api::SERVICE.operations.len(), 2);
    let service = Crucible::builder(api::operations(Arc::new(Handlers)))
        .on_rejection(|rejection| {
            Box::new(api::PublicFault {
                message: Some(rejection.summary()),
            })
        })
        .protocol(JsonProtocol)
        .operation_middleware(InternalCallers)
        .build()
        .unwrap();
    for (method, path, caller, status) in [
        ("POST", "/public", "external", 200),
        ("DELETE", "/secret", "external", 400),
        ("DELETE", "/secret", "internal", 200),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("x-example-caller", caller)
            .header("x-secret", "secret header")
            .body(Body::from(r#"{"privateValue":"private output"}"#))
            .unwrap();
        let response = service.handle(request).await;
        assert_eq!(response.status().as_u16(), status, "{path} for {caller}");
        if path == "/public" {
            assert_eq!(response.headers()["x-private-output"], "private output");
        }
    }
}
