//! The log line for an answer too large for Lambda says how large it was,
//! and of what: the body's own length when that is known ahead (never the
//! limit plus one it stopped reading at), or the whole answer's when the
//! body fit and the answer around it did not. Its own test binary, since it installs the
//! global tracing subscriber.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::response::Response;
use crucible_server_runtime_rust::{Crucible, host, raw_before_fn};
use serde_json::{Value, json};
use weather_service::MemoryCities;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The weather service, every request answered by `respond`.
fn service(respond: fn() -> Body) -> Crucible {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(raw_before_fn(move |job| {
            job.respond(Response::new(respond()));
            Ok(())
        }))
        .build()
        .unwrap()
}

async fn answer(service: &Crucible) -> Value {
    let event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/big",
        "rawQueryString": "",
        "headers": {},
        "requestContext": {
            "http": { "method": "GET", "path": "/big", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1" },
            "requestId": "big-request",
            "stage": "$default"
        },
        "isBase64Encoded": false
    });
    host::lambda_event(service, event).await.unwrap()
}

#[tokio::test]
async fn the_log_reports_the_size_of_the_replaced_answer() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    // A body whose length is known ahead.
    let known = answer(&service(|| Body::from(vec![b'x'; 7 * 1024 * 1024]))).await;
    assert_eq!(known["statusCode"], 500, "{known}");
    // A stream: read only up to the limit, so its length is not known.
    let streamed = answer(&service(|| {
        let chunks = (0..8).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 1024 * 1024]));
        Body::from_stream(futures_util::stream::iter(chunks))
    }))
    .await;
    assert_eq!(streamed["statusCode"], 500, "{streamed}");
    // A body that fits, in an answer that does not.
    let fits = answer(&service(|| Body::from(vec![b'x'; 6 * 1024 * 1024 - 16]))).await;
    assert_eq!(fits["statusCode"], 500, "{fits}");

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let records: Vec<Value> = logs
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| {
            record["fields"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("answered 500 instead"))
        })
        .collect();
    assert_eq!(records.len(), 3, "{logs}");
    let fields = |index: usize| &records[index]["fields"];
    assert_eq!(fields(0)["body_size"], 7 * 1024 * 1024, "{logs}");
    assert_eq!(fields(0)["limit"], 6 * 1024 * 1024, "{logs}");
    assert_eq!(fields(0)["request_id"], "big-request", "{logs}");
    assert!(fields(1).get("body_size").is_none(), "{logs}");
    assert_eq!(fields(1)["body_at_least"], 6 * 1024 * 1024 + 1, "{logs}");
    // The whole answer, which is what passed the limit.
    let answer_size = fields(2)["answer_size"].as_u64().unwrap();
    assert!(answer_size > 6 * 1024 * 1024, "{logs}");
    assert!(fields(2).get("body_size").is_none(), "{logs}");
}
