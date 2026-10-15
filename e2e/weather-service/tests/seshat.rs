//! A real request through `ServiceLog` and `Metrics` with the Seshat adapter
//! prints Seshat contract lines: one `crucible.request` line and one counter
//! increment per request.

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::middlewares::{LogPolicy, Metrics, ServiceLog};
use crucible_seshat::{SeshatLog, SeshatMetrics};
use serde_json::Value;
use weather_service::MemoryCities;

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
async fn requests_print_seshat_contract_lines() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    // Flattened JSON, as seshat-tracing prints it. This test owns its process,
    // so the global subscriber is set before any call site registers.
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(Metrics::new(SeshatMetrics::new("Requests")))
        .middleware(ServiceLog::new(LogPolicy::everything()).sink(SeshatLog))
        .build()
        .expect("the weather service builds");
    let response = service
        .handle(Request::get("/ping").body(Body::empty()).unwrap())
        .await;
    assert!(response.status().is_success());
    let missing = service
        .handle(
            Request::get("/cities/c-00000099")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(missing.status().as_u16(), 404);

    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let requests: Vec<&Value> = lines
        .iter()
        .filter(|l| l["event"] == "crucible.request")
        .collect();
    let counts: Vec<&Value> = lines
        .iter()
        .filter(|l| l["event"] == "seshat.metric")
        .collect();

    assert_eq!(requests.len(), 2, "{text}");
    let ping = requests[0];
    assert_eq!(ping["operation"], "Ping");
    assert_eq!(ping["status"], 200);
    assert_eq!(ping["outcome"], "success");
    assert!(ping["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(ping["latency_ms"].is_number());
    let missing = requests[1];
    assert_eq!(missing["status"], 404);
    assert_eq!(missing["error"], "NotFoundError");

    assert_eq!(counts.len(), 2, "{text}");
    assert_eq!(counts[0]["metric"], "Requests");
    assert_eq!(counts[0]["value"], 1);
    assert_eq!(counts[0]["operation"], "Ping");
    assert_eq!(counts[0]["class"], "success");
    assert_eq!(counts[1]["class"], "error");
}
