use crucible_server_runtime_rust::__generated as generated;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Error, Hook, Job, Middleware, Result};

include!(concat!(env!("OUT_DIR"), "/cors.rs"));
use cors_service::{CorsError, ReadInput, ReadOperation, ReadOutput};

struct Reader;

impl ReadOperation for Reader {
    async fn read(&self, input: ReadInput) -> Result<ReadOutput> {
        if input.id == "error" {
            return Err(CorsError {
                message: Some("failed".to_owned()),
                header: Some("error-header".to_owned()),
            }
            .into());
        }
        Ok(ReadOutput {
            header: Some("output-header".to_owned()),
        })
    }
}

fn service() -> Crucible {
    Crucible::builder(cors_service::operations(Arc::new(Reader)))
        .defaults()
        .on_rejection(|_| {
            Box::new(CorsError {
                message: Some("rejected".to_owned()),
                header: None,
            })
        })
        .build()
        .unwrap()
}

#[tokio::test]
async fn preflight_bypasses_routing_and_matches_prefix_headers() {
    let service = service();
    for path in ["/cors/ok", "/cors/error", "/nowhere"] {
        let response = service
            .handle(
                Request::builder()
                    .method("OPTIONS")
                    .uri(path)
                    .header("origin", "https://browser.example")
                    .header("access-control-request-method", "GET")
                    .header(
                        "access-control-request-headers",
                        "x-input, X-Meta-Trace, x-forbidden",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{path}");
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        assert_eq!(response.headers()["access-control-allow-methods"], "GET");
        assert_eq!(response.headers()["access-control-max-age"], "600");
        let allowed = response.headers()["access-control-allow-headers"]
            .to_str()
            .unwrap();
        assert!(allowed.contains("x-input"));
        assert!(allowed.contains("X-Meta-Trace"));
        assert!(!allowed.contains("x-forbidden"));
        assert_eq!(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .len(),
            0
        );
    }
}

#[tokio::test]
async fn simple_errors_and_rejections_expose_protocol_headers() {
    let service = service();
    for (path, status) in [("/cors/ok", 200), ("/cors/error", 409), ("/nowhere", 404)] {
        let response = service
            .handle(
                Request::get(path)
                    .header("origin", "https://browser.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        let exposed = response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap();
        for header in ["x-crucible-error", "x-request-id", "x-output", "x-error"] {
            assert!(exposed.contains(header));
        }
        assert!(response.headers().contains_key("x-request-id"));
        if status == 409 {
            assert_eq!(response.headers()["x-crucible-error"], "CorsError");
        }
    }
    let rejected = service
        .handle(
            Request::get("/cors/ok")
                .header("origin", "https://browser.example")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await;
    assert_eq!(rejected.status().as_u16(), 409);
    assert_eq!(rejected.headers()["access-control-allow-origin"], "*");
    let response = service
        .handle(Request::get("/cors/ok").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
}

struct Reject;

#[tokio::test]
async fn explicit_builder_and_outer_tower_rejection_keep_cors() {
    let service = Crucible::builder(cors_service::operations(Arc::new(Reader)))
        .protocol(crucible_server_runtime_rust::JsonProtocol)
        .on_rejection(|_| {
            Box::new(CorsError {
                message: None,
                header: None,
            })
        })
        .tower_layer(tower::layer::layer_fn(
            |_: crucible_server_runtime_rust::Endpoint| {
                tower::service_fn(|_: Request<Body>| async {
                    Ok::<_, std::convert::Infallible>(
                        axum::response::Response::builder()
                            .status(403)
                            .body(Body::empty())
                            .unwrap(),
                    )
                })
            },
        ))
        .build()
        .unwrap();
    let response = service
        .handle(
            Request::get("/cors/ok")
                .header("origin", "https://browser.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
}

static EXPLICIT_CORS: crucible_server_runtime_rust::operation::CorsInfo =
    crucible_server_runtime_rust::operation::CorsInfo {
        origin: "https://trusted.example",
        max_age: -1,
        allow_credentials: true,
        allowed_headers: &["authorization"],
        allowed_header_prefixes: &[],
        exposed_headers: &["x-crucible-error", "x-request-id"],
        methods: &["GET"],
    };
static EXPLICIT_SERVICE: crucible_server_runtime_rust::ServiceInfo = {
    let base = &(cors_service::SERVICE);
    generated::service_info(
        base.id,
        base.version,
        Some(&EXPLICIT_CORS),
        None,
        base.operations,
        base.errors,
        base.classify,
        base.reject,
        base.internal,
        base.unknown,
    )
};

#[tokio::test]
async fn explicit_origin_credentials_no_cache_and_direct_invocations() {
    let operations = crucible_server_runtime_rust::Operations::builder(&EXPLICIT_SERVICE)
        .add::<cors_service::Read, Reader, _>(Arc::new(Reader), |implementation, input| {
            Box::pin(async move { implementation.read(input).await })
        })
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .defaults()
        .on_rejection(|_| {
            Box::new(CorsError {
                message: None,
                header: None,
            })
        })
        .build()
        .unwrap();
    for origin in ["https://trusted.example", "https://untrusted.example"] {
        let response = service
            .handle(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/nowhere")
                    .header("origin", origin)
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;

        if origin == "https://trusted.example" {
            assert_eq!(response.status().as_u16(), 204);
            assert_eq!(response.headers()["access-control-allow-origin"], origin);
            assert_eq!(
                response.headers()["access-control-allow-credentials"],
                "true"
            );
            assert_eq!(response.headers()["access-control-max-age"], "-1");
        } else {
            assert_eq!(
                response.headers()["access-control-allow-origin"],
                "https://trusted.example"
            );
            assert_ne!(response.headers()["access-control-allow-origin"], origin);
            assert_ne!(response.status().as_u16(), 204);
        }
    }
    assert!(
        service
            .invoke("Read", serde_json::json!({"id":"ok"}))
            .await
            .get("output")
            .is_some()
    );
    let request = Request::post("/")
        .header("origin", "https://trusted.example")
        .extension(crucible_server_runtime_rust::protocol::Invocation {
            operation: "Read".to_owned(),
        })
        .body(Body::from(r#"{"id":"ok"}"#))
        .unwrap();
    let response = service.handle(request).await;
    assert!(
        !response
            .headers()
            .contains_key("access-control-allow-origin")
    );
}

struct PanicCompletion;
impl Middleware for PanicCompletion {
    fn complete(&self, _: &Job, _: &axum::response::Response) {
        panic!("completion failed");
    }
}

#[tokio::test]
async fn recovered_completion_panic_keeps_cors() {
    let service = Crucible::builder(cors_service::operations(Arc::new(Reader)))
        .defaults()
        .middleware(PanicCompletion)
        .on_rejection(|_| {
            Box::new(CorsError {
                message: None,
                header: None,
            })
        })
        .build()
        .unwrap();
    let response = service
        .handle(
            Request::get("/cors/ok")
                .header("origin", "https://browser.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status().as_u16(), 500);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
}
impl Middleware for Reject {
    fn before<'a>(&'a self, _: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(Err(Error::internal("transport rejected")))
    }
}

#[tokio::test]
async fn cors_round_trips_over_http() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, service().into_router())
            .await
            .unwrap();
    });
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        for (method, path, status) in [("OPTIONS", "/nowhere", "204"), ("GET", "/cors/ok", "200"), ("GET", "/cors/error", "409")] {
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: https://browser.example\r\nAccess-Control-Request-Method: GET\r\nConnection: close\r\n\r\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {status}")), "{response}");
            let response = response.to_ascii_lowercase();
            assert!(response.contains("access-control-allow-origin: *\r\n"), "{response}");
            assert!(response.contains("access-control-expose-headers:"));
            assert!(response.contains("x-crucible-error"));
            if status == "409" { assert!(response.contains("x-crucible-error: corserror\r\n")); }
        }
    }).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn transport_rejection_keeps_cors_and_preflight_skips_authentication() {
    let service = Crucible::builder(cors_service::operations(Arc::new(Reader)))
        .defaults()
        .middleware(Reject)
        .on_rejection(|_| {
            Box::new(CorsError {
                message: None,
                header: None,
            })
        })
        .build()
        .unwrap();
    let response = service
        .handle(
            Request::get("/cors/ok")
                .header("origin", "https://browser.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
    let response = service
        .handle(
            Request::builder()
                .method("OPTIONS")
                .uri("/cors/ok")
                .header("origin", "https://browser.example")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
