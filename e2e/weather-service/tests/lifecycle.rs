//! Admission, protocol ownership, and cleanup through the public middleware and protocol APIs.
use crucible_server_runtime_rust::__generated as generated;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::task::Poll;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Request, header};
use axum::response::Response;
use crucible_server_runtime_rust::middlewares::{LogSink, ServiceLog};
use crucible_server_runtime_rust::operation::{OperationInfo, Segment, ServiceInfo, Unit};
use crucible_server_runtime_rust::protocol::{Answers, Protocol, Route};
use crucible_server_runtime_rust::{
    Crucible, DecodedJob, Hook, Interceptor, Job, JsonProtocol, Operation,
    OperationAwareMiddleware, OperationJob, ProtocolExt, before_fn,
};
use futures_util::stream;
use serde_json::Value;
use weather_service::weather;

#[test]
fn job_remains_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Job>();
}

static INFO: OperationInfo = generated::operation_info(
    "lifecycle#Probe",
    "Probe",
    None,
    "POST",
    &[Segment::Literal("probe")],
    &[],
    200,
    &[],
    &[],
    &[],
    false,
    false,
    false,
    None,
    &[],
);
static SERVICE: ServiceInfo = generated::service_info(
    "lifecycle#Service",
    "1",
    None,
    None,
    &[&INFO],
    &[],
    |_| None,
    weather::SERVICE.reject,
    weather::SERVICE.internal,
    weather::SERVICE.unknown,
);
struct Probe;
impl Operation for Probe {
    const INFO: &'static OperationInfo = &INFO;
    type Input = Unit;
    type Output = Unit;
}
fn operations() -> crucible_server_runtime_rust::Operations {
    crucible_server_runtime_rust::Operations::builder(&SERVICE)
        .add::<Probe, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .build()
        .unwrap()
}
fn request(path: &str, body: Body) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .body(body)
        .unwrap()
}
fn pending_body(polls: Arc<AtomicUsize>) -> Body {
    Body::from_stream(stream::poll_fn(move |_| {
        polls.fetch_add(1, Ordering::SeqCst);
        Poll::<Option<Result<Bytes, std::io::Error>>>::Pending
    }))
}
type Events = Arc<Mutex<Vec<String>>>;
struct Observe {
    name: &'static str,
    events: Events,
    fail: bool,
}
impl Observe {
    fn event(&self, phase: &str) {
        self.events
            .lock()
            .unwrap()
            .push(format!("{}.{phase}", self.name));
    }
}
impl OperationAwareMiddleware for Observe {
    fn before<'a>(
        &'a self,
        _: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.event("before");
        Hook::Ready(if self.fail {
            Err(crucible_server_runtime_rust::Error::internal("refused"))
        } else {
            Ok(())
        })
    }
    fn after<'a>(&'a self, _: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        self.event("after");
        Hook::Ready(())
    }
    fn complete(&self, _: &OperationJob<'_>, _: &Response) {
        self.event("complete");
    }
}
fn observe(name: &'static str, events: &Events, fail: bool) -> Observe {
    Observe {
        name,
        events: events.clone(),
        fail,
    }
}
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u16>>>);
impl LogSink for Logs {
    fn write(&self, record: &crucible_server_runtime_rust::middlewares::LogRecord<'_>) {
        self.0.lock().unwrap().push(record.status);
    }
}

#[tokio::test]
async fn admission_can_reject_or_relay_without_polling_a_pending_body() {
    for status in [401, 202] {
        let polls = Arc::new(AtomicUsize::new(0));
        let events = Events::default();
        let logs = Logs::default();
        let service = Crucible::builder(operations())
            .middleware(ServiceLog::errors_only().sink(logs.clone()))
            .operation_middleware(observe("outer", &events, false))
            .operation_middleware(before_fn(move |job| {
                assert!(job.body().is_empty());
                job.respond(
                    Response::builder()
                        .status(status)
                        .body(Body::from("answer"))
                        .unwrap(),
                );
                Ok(())
            }))
            .operation_middleware(observe("never", &events, false))
            .protocol(JsonProtocol)
            .build()
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_millis(100),
            service.handle(request("/probe", pending_body(polls.clone()))),
        )
        .await
        .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert_eq!(
            *events.lock().unwrap(),
            ["outer.before", "outer.after", "outer.complete"]
        );
        if status == 401 {
            assert_eq!(*logs.0.lock().unwrap(), [401]);
        }
    }
}

#[tokio::test]
async fn body_rejection_unwinds_only_successfully_entered_handlers_and_still_logs() {
    let events = Events::default();
    let logs = Logs::default();
    let service = Crucible::builder(operations())
        .body_limit(1)
        .middleware(ServiceLog::errors_only().sink(logs.clone()))
        .operation_middleware(observe("outer", &events, false))
        .protocol(JsonProtocol)
        .interceptor(NeverDecoded)
        .build()
        .unwrap();
    let response = service.handle(request("/probe", Body::from("{}"))).await;
    assert_eq!(response.status(), 400);
    assert_eq!(
        *events.lock().unwrap(),
        ["outer.before", "outer.after", "outer.complete"]
    );
    assert_eq!(*logs.0.lock().unwrap(), [400]);
}

#[tokio::test]
async fn a_failing_before_is_not_unwound_and_later_hooks_never_enter() {
    let events = Events::default();
    let polls = Arc::new(AtomicUsize::new(0));
    let service = Crucible::builder(operations())
        .operation_middleware(observe("outer", &events, false))
        .operation_middleware(observe("failure", &events, true))
        .operation_middleware(observe("never", &events, false))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = tokio::time::timeout(
        Duration::from_millis(100),
        service.handle(request("/probe", pending_body(polls.clone()))),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 500);
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(
        *events.lock().unwrap(),
        [
            "outer.before",
            "failure.before",
            "outer.after",
            "outer.complete"
        ]
    );
}

struct Reader {
    name: &'static str,
    path: &'static str,
    events: Events,
}
impl Protocol for Reader {
    fn route(
        &self,
        request: &axum::http::request::Parts,
        _: &crucible_server_runtime_rust::Operations,
    ) -> Option<Route> {
        (request.uri.path() == self.path).then_some(Route {
            rejection: None,
            operation: "Probe",
            labels: Vec::new(),
        })
    }
    fn decode<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.events
            .lock()
            .unwrap()
            .push(format!("{}.decode", self.name));
        assert_eq!(job.body().as_ref(), b"RAW");
        Hook::Ready(match job.decode_json(b"{}") {
            Ok(input) => {
                job.set_input(input);
                Ok(())
            }
            Err(error) => Err(error.into()),
        })
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        let mut response = JsonProtocol.write(job, answers);
        response
            .headers_mut()
            .insert("x-protocol", HeaderValue::from_static(self.name));
        response
    }
}
fn reader(name: &'static str, path: &'static str, events: &Events) -> Reader {
    Reader {
        name,
        path,
        events: events.clone(),
    }
}

#[tokio::test]
async fn only_the_selected_protocol_decodes_and_writes() {
    let events = Events::default();
    let service = Crucible::builder(operations())
        .operation_middleware(observe("outer", &events, false))
        .protocol(reader("unselected", "/other", &events))
        .protocol(JsonProtocol)
        .protocol(reader("selected", "/raw-probe", &events))
        .build()
        .unwrap();
    let response = service
        .handle(request("/raw-probe", Body::from("RAW")))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-protocol"], "selected");
    assert_eq!(
        *events.lock().unwrap(),
        [
            "outer.before",
            "selected.decode",
            "outer.after",
            "outer.complete"
        ]
    );
}

#[tokio::test]
async fn scoped_protocols_leave_excluded_routes_to_the_fallback() {
    let events = Events::default();
    let service = Crucible::builder(operations())
        .protocol(reader("excluded", "/raw-probe", &events).only(&["Other"]))
        .protocol(reader("fallback", "/raw-probe", &events))
        .build()
        .unwrap();
    let response = service
        .handle(request("/raw-probe", Body::from("RAW")))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["x-protocol"], "fallback");
    assert_eq!(*events.lock().unwrap(), ["fallback.decode"]);
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol.only(&["Other"]))
        .build()
        .unwrap();
    let response = service.handle(request("/probe", Body::from("{}"))).await;
    assert_eq!(response.status(), 400);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["reason"], "unknownOperation");
}

#[tokio::test]
async fn job_response_headers_preserve_repeated_values_and_replace_existing_groups() {
    for answered in [false, true] {
        let service = Crucible::builder(operations())
            .operation_middleware(before_fn(move |job| {
                job.response_headers_mut()
                    .append(header::SET_COOKIE, HeaderValue::from_static("a=1"));
                job.response_headers_mut()
                    .append(header::SET_COOKIE, HeaderValue::from_static("b=2"));
                if answered {
                    job.respond(
                        Response::builder()
                            .status(202)
                            .header(header::SET_COOKIE, "old=0")
                            .body(Body::empty())
                            .unwrap(),
                    );
                }
                Ok(())
            }))
            .protocol(JsonProtocol)
            .build()
            .unwrap();
        let response = service.handle(request("/probe", Body::from("{}"))).await;
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(cookies, ["a=1", "b=2"]);
    }
}

struct InspectBody;
impl OperationAwareMiddleware for InspectBody {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        Hook::wait(async move {
            assert!(job.body().is_empty());
            job.read_body().await?;
            assert_eq!(job.body().as_ref(), b"{}");
            job.read_body().await?;
            assert_eq!(job.body().as_ref(), b"{}");
            Ok(())
        })
    }
}
#[tokio::test]
async fn admission_can_explicitly_read_once_before_the_protocol() {
    let service = Crucible::builder(operations())
        .operation_middleware(InspectBody)
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    assert_eq!(
        service
            .handle(request("/probe", Body::from("{}")))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn body_transport_failure_is_not_reported_as_a_length_limit_or_leaked() {
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let body = Body::from_stream(stream::once(async {
        Err::<Bytes, _>(std::io::Error::other("PRIVATE_BODY_READ_DETAIL"))
    }));
    let response = service.handle(request("/probe", body)).await;
    assert_eq!(response.status(), 400);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("could not read the request body"));
    assert!(!text.contains("larger"));
    assert!(!text.contains("PRIVATE_BODY_READ_DETAIL"));
}

struct CancelRead;
impl OperationAwareMiddleware for CancelRead {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        Hook::wait(async move {
            assert!(
                tokio::time::timeout(Duration::from_millis(10), job.read_body())
                    .await
                    .is_err()
            );
            let first = job
                .read_body()
                .await
                .expect_err("cancelled body is not empty success");
            let second = job
                .read_body()
                .await
                .expect_err("repeated reads retain the rejection");
            assert_eq!(first.to_string(), second.to_string());
            Err(first)
        })
    }
}

#[tokio::test]
async fn cancelled_explicit_body_reads_remain_rejected_on_retry() {
    let events = Events::default();
    let polls = Arc::new(AtomicUsize::new(0));
    let service = Crucible::builder(operations())
        .operation_middleware(observe("outer", &events, false))
        .operation_middleware(CancelRead)
        .operation_middleware(observe("never", &events, false))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = tokio::time::timeout(
        Duration::from_millis(100),
        service.handle(request("/probe", pending_body(polls.clone()))),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 400);
    assert!(polls.load(Ordering::SeqCst) > 0);
    assert_eq!(
        *events.lock().unwrap(),
        ["outer.before", "outer.after", "outer.complete"]
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains("request body reading did not complete")
    );
}

struct NeverDecoded;
impl Interceptor for NeverDecoded {
    fn before<'a>(
        &'a self,
        _: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        panic!("invalid input must never enter the interceptor chain");
    }
}
