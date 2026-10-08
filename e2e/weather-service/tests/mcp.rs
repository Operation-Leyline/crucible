use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::IntoResponse;
use crucible_server_runtime_rust::mcp::{EndpointAdmission, McpProtocol};
use crucible_server_runtime_rust::{
    AuthRefusal, Crucible, Hook, OperationAwareMiddleware, OperationJob,
};
use serde_json::{Value, json};
use weather_service::{MemoryCities, weather};

fn service(protocol: McpProtocol) -> Crucible {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(protocol)
        .build()
        .unwrap()
}

#[tokio::test]
async fn opt_in_custom_path_serves_json_over_real_http() {
    let default = weather_service::service(Arc::new(MemoryCities::default()));
    assert_eq!(
        request(&default, "POST", &rpc("ping", json!({})).to_string(), &[])
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let service = service(
        McpProtocol::new(weather::MCP_TOOLS)
            .path("/agents/mcp")
            .allow_any_origin(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, service.into_router()).await.unwrap();
    });
    // The client runtime installs the process TLS provider used by raw reqwest.
    drop(crucible_client_runtime_rust::Config::new(&endpoint));
    let http = reqwest::Client::new();
    for message in [
        rpc(
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        ),
        rpc("tools/list", json!({})),
        rpc("tools/call", json!({"name":"Ping","arguments":{}})),
    ] {
        let response = http
            .post(format!("{endpoint}/agents/mcp"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("origin", "https://explicit-wildcard.example")
            .header("mcp-protocol-version", "2025-11-25")
            .body(message.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(!response.headers().contains_key("mcp-session-id"));
        let response: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert!(response.get("result").is_some(), "{response}");
    }
    assert_eq!(
        http.get(format!("{endpoint}/agents/mcp"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        405
    );
    assert_eq!(
        http.post(format!("{endpoint}/mcp"))
            .body("{}")
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        400
    );
    server.abort();
}
fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":7,"method":method,"params":params})
}
async fn request(
    service: &Crucible,
    method: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .uri("/mcp")
        .method(method)
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = service
        .handle(request.body(Body::from(body.to_owned())).unwrap())
        .await;
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        to_bytes(body, usize::MAX).await.unwrap().to_vec(),
    )
}
async fn send(service: &Crucible, message: Value) -> Value {
    let (status, _, body) = request(service, "POST", &message.to_string(), &[]).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let response: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(response["id"], message["id"]);
    response
}
async fn call(service: &Crucible, tools: &Value, name: &str, arguments: Value) -> Value {
    let response = send(
        service,
        rpc("tools/call", json!({"name":name,"arguments":arguments})),
    )
    .await;
    if response["result"]["isError"] == false {
        let tool = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap();
        let output = &response["result"]["structuredContent"];
        jsonschema::validator_for(&tool["outputSchema"])
            .unwrap()
            .validate(output)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(
                response["result"]["content"][0]["text"].as_str().unwrap()
            )
            .unwrap(),
            *output
        );
    }
    response
}

#[tokio::test]
async fn discovery_and_every_operation_output_match_schemas() {
    let service = service(McpProtocol::new(weather::MCP_TOOLS).server_info("Weather", "1"));
    let init = send(&service,rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        init["result"]["serverInfo"],
        json!({"name":"Weather","version":"1"})
    );
    assert_eq!(
        init["result"]["capabilities"],
        json!({"tools":{"listChanged":false}})
    );
    let (status, _, body) = request(
        &service,
        "POST",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body.is_empty());
    assert_eq!(
        send(&service, rpc("ping", json!({}))).await["result"],
        json!({})
    );
    let tools = send(&service, rpc("tools/list", json!({}))).await["result"].clone();
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mcp_tools.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(
            &golden,
            format!("{}\n", serde_json::to_string_pretty(&tools).unwrap()),
        )
        .unwrap();
    }
    assert_eq!(
        tools,
        serde_json::from_str::<Value>(&std::fs::read_to_string(golden).unwrap()).unwrap()
    );
    let created = call(
        &service,
        &tools,
        "CreateCity",
        json!({"name":"Paris","expectedRevision":12,"clientToken":"stable"}),
    )
    .await;
    assert_eq!(created["result"]["isError"], false);
    let id = created["result"]["structuredContent"]["city"]["id"]
        .as_str()
        .unwrap();
    for (name, args) in [
        ("GetCity", json!({"cityId":id,"at":"2026-10-01T12:00:00Z"})),
        ("ListCities", json!({"maxResults":1,"nextToken":null})),
        ("GetCityPhoto", json!({"cityId":id})),
        (
            "PutCityPhoto",
            json!({"cityId":id,"photo":"YWJj","caption":"caption"}),
        ),
        ("GetCityPhoto", json!({"cityId":id})),
        ("Ping", json!({})),
        ("Status", json!({})),
    ] {
        assert_eq!(
            call(&service, &tools, name, args).await["result"]["isError"],
            false,
            "{name}"
        );
    }
    let error = call(&service, &tools, "GetCity", json!({"cityId":"c-00000099"})).await;
    assert_eq!(error["result"]["isError"], true, "{error}");
    let detail: Value =
        serde_json::from_str(error["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(detail["name"], "NotFoundError");
    assert_eq!(detail["message"], "no such city");
    assert!(error["result"].get("structuredContent").is_none());
    for args in [
        json!({}),
        json!({"name":"x".repeat(41)}),
        json!({"name":42}),
        json!({"name":"Paris","extra":true}),
    ] {
        let response = call(&service, &tools, "CreateCity", args.clone()).await;
        assert_eq!(response["error"]["code"], -32602, "{args}: {response}");
    }
    // The handler can return a value valid for its input override but invalid
    // for the output's narrower target constraints. Never publish that value.
    let invalid_output = call(&service, &tools, "CreateCity", json!({"name":""})).await;
    assert_eq!(invalid_output["error"]["code"], -32603);
    let conflict = call(&service, &tools, "CreateCity", json!({"name":"Paris"})).await;
    assert_eq!(conflict["result"]["isError"], true);
    assert!(
        conflict["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ConflictError")
    );
}

#[tokio::test]
async fn transport_security_and_json_rpc_errors() {
    let service =
        service(McpProtocol::new(weather::MCP_TOOLS).allowed_origins(["https://agent.example"]));
    for method in ["GET", "DELETE", "PUT"] {
        let (status, headers, _) = request(&service, method, "invalid", &[]).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers["allow"], "POST");
    }
    for headers in [
        vec![("origin", "https://evil.example")],
        vec![("origin", "null")],
        vec![
            ("origin", "https://agent.example"),
            ("origin", "https://evil.example"),
        ],
    ] {
        assert_eq!(
            request(&service, "POST", "invalid", &headers).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &service,
            "POST",
            "invalid",
            &[("origin", "https://agent.example")]
        )
        .await
        .0,
        StatusCode::OK
    );
    for version in ["2024-11-05", "garbage"] {
        assert_eq!(
            request(
                &service,
                "POST",
                "invalid",
                &[("mcp-protocol-version", version)]
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    for (body, code) in [
        ("{", -32700),
        ("[]", -32600),
        (r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#, -32600),
        (r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#, -32600),
    ] {
        let (_, _, body) = request(&service, "POST", body, &[]).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
            code
        );
    }
    for (method, params, code) in [
        ("unknown", json!({}), -32601),
        ("tools/call", json!({"name":"unknown"}), -32602),
        ("tools/call", json!({"name":"Ping","arguments":[]}), -32602),
        ("tools/list", json!({"cursor":"x"}), -32602),
        ("initialize", json!({}), -32602),
        ("ping", json!([]), -32602),
    ] {
        assert_eq!(
            send(&service, rpc(method, params)).await["error"]["code"],
            code
        );
    }
    let init=send(&service,rpc("initialize",json!({"protocolVersion":"unknown","capabilities":{},"clientInfo":{"name":"test","version":"1"}}))).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
}

struct OperationAuth;
impl OperationAwareMiddleware for OperationAuth {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"weather\"".parse().unwrap(),
        );
        let refusal = if job.context().headers().contains_key("x-forbidden") {
            AuthRefusal::forbidden(headers)
        } else {
            AuthRefusal::unauthorized(headers)
        };
        Hook::Ready(Err(refusal.into()))
    }
}

struct FrameworkOperationAuth;
impl OperationAwareMiddleware for FrameworkOperationAuth {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let refusal = if job.context().headers().contains_key("x-forbidden") {
            crucible_server_runtime_rust::FrameworkError::Forbidden
        } else {
            crucible_server_runtime_rust::FrameworkError::Unauthenticated
        };
        Hook::Ready(Err(refusal.into()))
    }
}

struct TransportRefusal;
impl crucible_server_runtime_rust::Middleware for TransportRefusal {
    fn before<'a>(
        &'a self,
        _job: &'a mut crucible_server_runtime_rust::Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        Hook::Ready(Err(weather::ConflictError {
            message: "transport refused".into(),
        }
        .into()))
    }
}

#[tokio::test]
async fn selected_tool_uses_precomputed_operation_body_limit() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(McpProtocol::new(weather::MCP_TOOLS))
        .operation_body_limit::<weather::Ping>(1)
        .build()
        .unwrap();
    // Discovery has no selected operation and keeps the service envelope limit.
    assert!(send(&service, rpc("tools/list", json!({}))).await["result"]["tools"].is_array());
    let message = rpc("tools/call", json!({"name":"Ping","arguments":{}}));
    let (status, _, body) = request(&service, "POST", &message.to_string(), &[]).await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], -32602);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("larger than 1 bytes")
    );
}

#[tokio::test]
async fn pre_dispatch_failures_preserve_http_errors() {
    let limited = weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(McpProtocol::new(weather::MCP_TOOLS))
        .body_limit(1)
        .build()
        .unwrap();
    let refused = weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(McpProtocol::new(weather::MCP_TOOLS))
        .middleware(TransportRefusal)
        .build()
        .unwrap();
    for (service, status, error) in [
        (limited, StatusCode::BAD_REQUEST, "ValidationError"),
        (refused, StatusCode::CONFLICT, "ConflictError"),
    ] {
        let (actual, headers, body) =
            request(&service, "POST", &rpc("ping", json!({})).to_string(), &[]).await;
        assert_eq!(actual, status);
        assert!(headers.contains_key("x-request-id"));
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["__type"], error);
        assert!(body.get("jsonrpc").is_none());
    }
}

#[tokio::test]
async fn framework_auth_refusals_keep_http_status_in_mcp() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(McpProtocol::new(weather::MCP_TOOLS))
        .operation_middleware(FrameworkOperationAuth)
        .build()
        .unwrap();
    for (headers, status) in [
        (vec![], StatusCode::UNAUTHORIZED),
        (vec![("x-forbidden", "1")], StatusCode::FORBIDDEN),
    ] {
        let (actual, _, body) = request(
            &service,
            "POST",
            &rpc("tools/call", json!({"name":"CreateCity","arguments":{}})).to_string(),
            &headers,
        )
        .await;
        assert_eq!(actual, status);
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("jsonrpc").is_none(), "{body}");
    }
}

#[tokio::test]
async fn endpoint_and_operation_auth_remain_http_refusals() {
    let guarded = service(McpProtocol::new(weather::MCP_TOOLS).endpoint_guard(
        |parts: &axum::http::request::Parts| {
            if parts
                .headers
                .get("authorization")
                .is_some_and(|v| v == "Bearer good")
            {
                EndpointAdmission::Admit
            } else {
                EndpointAdmission::Respond(
                    (
                        StatusCode::UNAUTHORIZED,
                        [(header::WWW_AUTHENTICATE, "Bearer")],
                    )
                        .into_response(),
                )
            }
        },
    ));
    for method in [
        "initialize",
        "tools/list",
        "ping",
        "tools/call",
        "notifications/initialized",
    ] {
        let (status, headers, body) =
            request(&guarded, "POST", &rpc(method, json!({})).to_string(), &[]).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers["www-authenticate"], "Bearer");
        assert!(body.is_empty());
    }
    assert_eq!(
        request(&guarded, "POST", "invalid", &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&guarded, "GET", "", &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &guarded,
            "POST",
            &rpc("ping", json!({})).to_string(),
            &[("authorization", "Bearer good")]
        )
        .await
        .0,
        StatusCode::OK
    );
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .mcp(McpProtocol::new(weather::MCP_TOOLS))
        .operation_middleware(OperationAuth)
        .build()
        .unwrap();
    for (headers, status) in [
        (vec![], StatusCode::UNAUTHORIZED),
        (vec![("x-forbidden", "1")], StatusCode::FORBIDDEN),
    ] {
        let (actual, headers, body) = request(
            &service,
            "POST",
            &rpc("tools/call", json!({"name":"CreateCity","arguments":{}})).to_string(),
            &headers,
        )
        .await;
        assert_eq!(actual, status);
        assert!(headers.contains_key(header::WWW_AUTHENTICATE));
        assert!(body.is_empty());
    }
}
