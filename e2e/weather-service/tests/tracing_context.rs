//! Keep the global tracing subscriber isolated from other integration tests.

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible::before_fn;
use crucible::middlewares::{LogPolicy, ServiceLog};
use crucible::{Context, Crucible, InvokeProtocol, JsonProtocol};
use serde_json::{Value, json};

mod api {
    include!(concat!(env!("OUT_DIR"), "/observability.rs"));
}

struct Echo;
impl api::EchoOperation for Echo {
    async fn echo(&self, input: api::model::Payload) -> crucible::Result<api::model::Payload> {
        tracing::info!("application event");
        let context = Context::current();
        tokio::spawn(context.scope(async {
            tracing::info!("spawned event");
        }))
        .await
        .unwrap();
        if input.profile.as_ref().and_then(|p| p.name.as_deref()) == Some("fail") {
            return Err(api::errors::SecretError {
                profile: input.profile,
                message: Some("conflict".into()),
            }
            .into());
        }
        Ok(input)
    }
}
impl api::OtherOperation for Echo {
    async fn other(&self, input: api::model::Payload) -> crucible::Result<api::model::Payload> {
        Ok(input)
    }
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn tracing_has_structured_fields_levels_and_request_context_without_duplicate_faults() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    // Install before any requests run. This test owns its process so other
    // tests cannot initialize shared tracing callsites during setup.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let service = Crucible::builder(api::operations(Arc::new(Echo)))
        .request_id(|_| Some(axum::http::HeaderValue::from_static("tracing-test")))
        .middleware(ServiceLog::new(
            LogPolicy::everything().client_error_level(tracing::Level::WARN),
        ))
        .operation_middleware(before_fn(|job| {
            if job.request().headers.contains_key("x-fail") {
                return Err(crucible::Error::internal("test fault"));
            }
            Ok(())
        }))
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    async {
        service.invoke("Echo", json!({})).await;
        service
            .invoke("Echo", json!({"profile":{"name":"fail"}}))
            .await;
        service
            .handle(
                Request::post("/echo?mode=echo&flag")
                    .header("x-fail", "1")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
    }
    .await;
    let bytes = buffer.0.lock().unwrap().clone();
    let events: Vec<Value> = String::from_utf8_lossy(&bytes)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect();
    let completed: Vec<_> = events
        .iter()
        .filter(|e| e["target"] == "crucible::service_log")
        .collect();
    assert_eq!(completed.len(), 3, "{events:?}");
    assert_eq!(completed[0]["fields"]["status"], 200);
    assert!(completed[0]["fields"]["latency_ms"].is_number());
    assert_eq!(completed[1]["level"], "WARN");
    assert_eq!(completed[2]["level"], "ERROR");
    assert_eq!(events.iter().filter(|e| e["level"] == "ERROR").count(), 1);
    for event in events.iter().filter(|e| {
        e["fields"]["message"] == "application event" || e["fields"]["message"] == "spawned event"
    }) {
        assert_eq!(event["span"]["operation"], "Echo", "{events:#?}");
        assert_eq!(event["span"]["request_id"], "tracing-test", "{events:#?}");
    }
    for message in ["application event", "spawned event"] {
        assert_eq!(
            events
                .iter()
                .filter(|e| e["fields"]["message"] == message)
                .count(),
            2,
            "{events:#?}"
        );
    }
}
