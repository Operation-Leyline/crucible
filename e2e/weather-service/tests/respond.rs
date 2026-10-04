//! A handler that answers the request itself: a relay that hands back a
//! response from elsewhere, verbatim, before the input is read.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, Response};
use crucible::handlers::{LogPolicy, LogSink, ServiceLog, before_fn};
use crucible::{Crucible, Handler, Hook, InvokeProtocol, Job, JsonProtocol, Resolution};
use serde_json::{Value, json};
use weather_service::contract;
use weather_service::{MemoryCities, Weather};

/// Answers requests carrying `x-relay` with a canned response, as a handler
/// forwarding to another region would with the response it got back.
struct Relay(Arc<Mutex<Vec<String>>>);

impl Handler for Relay {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, crucible::Result<()>> {
        if let Some(id) = job.request().headers.get("x-relay").cloned() {
            let mut response = Response::builder()
                .status(202)
                .header("content-type", "text/plain")
                .header("x-relayed-from", "home-b");
            if id != "fresh" {
                response = response.header("x-request-id", id);
            }
            job.respond(response.body(Body::from("relayed bytes")).unwrap());
        }
        Hook::Ready(Ok(()))
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        if let Some(Resolution::Answered { status }) = job.resolution() {
            self.0.lock().unwrap().push(format!("relay after {status}"));
        }
        Hook::Ready(())
    }
}

#[derive(Clone, Default)]
struct Records(Arc<Mutex<Vec<Value>>>);

impl LogSink for Records {
    fn write(&self, record: &Value) {
        self.0.lock().unwrap().push(record.clone());
    }
}

fn relaying() -> (Crucible, Arc<Mutex<Vec<String>>>, Records) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let records = Records::default();
    let inner = seen.clone();
    let service = Crucible::builder(contract::operations(Arc::new(Weather::new(Arc::new(
        MemoryCities::default(),
    )))))
    .handler(
        ServiceLog::errors_only()
            .operation::<contract::CreateCity>(LogPolicy::everything())
            .sink(records.clone()),
    )
    .handler(Relay(seen.clone()))
    .handler(before_fn(move |_| {
        inner.lock().unwrap().push("inner before".to_owned());
        Ok(())
    }))
    .handler(JsonProtocol)
    .handler(InvokeProtocol)
    .build()
    .unwrap();
    (service, seen, records)
}

async fn send(service: &Crucible, request: Request<Body>) -> (Response<Body>, String) {
    let response = service.handle(request).await;
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (
        Response::from_parts(parts, Body::empty()),
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

fn create(relay: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/cities").header("content-type", "application/json");
    if let Some(relay) = relay {
        request = request.header("x-relay", relay);
    }
    request
        .body(Body::from(json!({ "name": "Lima" }).to_string()))
        .unwrap()
}

#[tokio::test]
async fn an_answered_request_is_sent_verbatim_and_the_operation_never_runs() {
    let (service, seen, records) = relaying();
    let (response, body) = send(&service, create(Some("fresh"))).await;
    assert_eq!(response.status(), 202);
    assert_eq!(body, "relayed bytes", "not rewritten by the protocol");
    assert_eq!(response.headers()["x-relayed-from"], "home-b");
    assert!(
        response.headers().contains_key("x-request-id"),
        "an id is added when the answer has none"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        ["relay after 202"],
        "later before hooks skipped; the answering handler's after ran"
    );

    let (_, listed) = send(
        &service,
        Request::get("/cities").body(Body::empty()).unwrap(),
    )
    .await;
    let listed: Value = serde_json::from_str(&listed).unwrap();
    assert_eq!(listed["cities"], json!([]), "CreateCity never ran");

    let records = records.0.lock().unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["operation"], "CreateCity");
    assert_eq!(records[0]["status"], 202);
    assert_eq!(records[0]["outcome"], "success");
}

#[tokio::test]
async fn an_answer_keeps_its_own_request_id() {
    let (service, _, _) = relaying();
    let (response, _) = send(&service, create(Some("id-from-home-b"))).await;
    assert_eq!(response.headers()["x-request-id"], "id-from-home-b");
}

#[tokio::test]
async fn requests_the_handler_does_not_answer_run_as_usual() {
    let (service, seen, _) = relaying();
    let (response, body) = send(&service, create(None)).await;
    assert_eq!(response.status(), 201, "{body}");
    assert_eq!(*seen.lock().unwrap(), ["inner before"]);
}
