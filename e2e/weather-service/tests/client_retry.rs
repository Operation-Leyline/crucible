//! Both client runtimes consume this same HTTP retry decision table.
use crucible_server_runtime_rust::__generated as generated;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use crucible_client_runtime_rust::operation::{ErrorInfo, Fault};
use crucible_client_runtime_rust::{
    Config, Error, Operation, OperationErrors, OperationInfo, Retry, Transport,
};
use serde::Deserialize;
use serde_json::{Value, json};
use weather_service::weather::StatusInput;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    safe: bool,
    status: u16,
    error: Option<String>,
    failures: u32,
    max_attempts: u32,
    attempts: u32,
    success: bool,
}

struct Request<const SAFE: bool>;
impl<const SAFE: bool> Operation for Request<SAFE> {
    type Input = StatusInput;
    type Output = Value;
    const INFO: &'static OperationInfo = &generated::operation_info(
        "conformance#RetryCase",
        "RetryCase",
        None,
        "POST",
        &[],
        &[],
        200,
        &[],
        &[],
        &["Ordinary", "Transient", "Throttled"],
        SAFE,
        false,
        false,
        None,
        &[],
    );
}

static ORDINARY: ErrorInfo =
    generated::error_info("Ordinary", 400, Fault::Client, false, false, &[]);
static TRANSIENT: ErrorInfo =
    generated::error_info("Transient", 400, Fault::Client, true, false, &[]);
static THROTTLED: ErrorInfo =
    generated::error_info("Throttled", 429, Fault::Client, true, true, &[]);

#[derive(Debug)]
struct Modelled(&'static ErrorInfo);
impl OperationErrors for Modelled {
    fn decode(status: u16, name: Option<&str>, _: &HeaderMap, body: &[u8]) -> Option<Self> {
        if serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|v| v.get("malformed").cloned())
            == Some(json!(true))
        {
            return None;
        }
        Self::classify(status, name).map(Self)
    }
    fn classify(status: u16, name: Option<&str>) -> Option<&'static ErrorInfo> {
        match name.or_else(|| {
            crucible_client_runtime_rust::by_status(status, &[&ORDINARY, &TRANSIENT, &THROTTLED])
        })? {
            "Ordinary" => Some(&ORDINARY),
            "Transient" => Some(&TRANSIENT),
            "Throttled" => Some(&THROTTLED),
            _ => None,
        }
    }
    fn info(&self) -> &'static ErrorInfo {
        self.0
    }
}

#[tokio::test]
async fn malformed_throttling_errors_still_retry_unsafe_operations() {
    let attempts = Arc::new(AtomicU32::new(0));
    let count = attempts.clone();
    let app = Router::new().route(
        "/",
        post(move || {
            let n = count.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        r#"{"__type":"Throttled","message":42,"malformed":true}"#,
                    )
                } else {
                    (StatusCode::OK, "{}")
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let transport = Transport::new(Config::new(format!("http://{address}")).retry(Retry {
        max_attempts: 2,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    }));
    let result = transport
        .send::<Request<false>, Modelled>(&StatusInput {})
        .await;
    server.abort();
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn shared_http_retry_cases() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../../tests/conformance/retry.json")).unwrap();
    let mut mismatches = Vec::new();
    for c in cases {
        let attempts = Arc::new(AtomicU32::new(0));
        let count = attempts.clone();
        let status = StatusCode::from_u16(c.status).unwrap();
        let body = c
            .error
            .map_or_else(|| json!({}), |name| json!({"__type": name}))
            .to_string();
        let app = Router::new().route(
            "/",
            post(move || {
                let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                let body = body.clone();
                async move {
                    (
                        if n <= c.failures {
                            status
                        } else {
                            StatusCode::OK
                        },
                        body,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let transport = Transport::new(Config::new(format!("http://{address}")).retry(Retry {
            max_attempts: c.max_attempts,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }));
        let result = if c.safe {
            transport
                .send::<Request<true>, Modelled>(&StatusInput {})
                .await
        } else {
            transport
                .send::<Request<false>, Modelled>(&StatusInput {})
                .await
        };
        server.abort();
        assert!(
            matches!(
                &result,
                Ok(_) | Err(Error::Service(_)) | Err(Error::Unknown(_))
            ),
            "{}: {result:?}",
            c.name
        );
        let actual = (attempts.load(Ordering::SeqCst), result.is_ok());
        if actual != (c.attempts, c.success) {
            mismatches.push(format!(
                "{}: expected {:?}, got {actual:?}",
                c.name,
                (c.attempts, c.success)
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
