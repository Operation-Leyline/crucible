//! Regression proof for transport, identification, decoding, and invocation boundaries.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::Response;
use crucible_server_runtime_rust::operation::{OperationInfo, Segment, ServiceInfo, Unit};
use crucible_server_runtime_rust::protocol::{Answers, ProtocolMatch, Route};
use crucible_server_runtime_rust::validate::{Rejection, Validate, Validator};
use crucible_server_runtime_rust::{
    Crucible, DecodedJob, ExceptionTranslator, Hook, Interceptor, InvokeProtocol, Job,
    JsonProtocol, Middleware, Operation, OperationAwareMiddleware, OperationJob, Operations,
    Protocol, ProtocolExt, Resolution, raw_before_fn,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weather_service::weather;

type Events = Arc<Mutex<Vec<String>>>;
fn record(events: &Events, event: impl Into<String>) {
    events.lock().unwrap().push(event.into());
}

#[derive(Serialize, Deserialize)]
struct ProbeInput {
    value: u64,
}
impl Validate for ProbeInput {
    fn validate(&self, validator: &mut Validator) {
        validator.at("value", |validator| {
            validator.range_integer(i128::from(self.value), Some(1), None)
        });
    }
}
static INFO: OperationInfo = OperationInfo {
    id: "phases#Probe",
    name: "Probe",
    resource: None,
    method: "POST",
    path: &[Segment::Literal("probe")],
    literal_query: &[],
    status: 200,
    input: &[],
    output: &[],
    errors: &[],
    readonly: false,
    idempotent: false,
    idempotency_token: None,
    traits: &[],
};
static SERVICE: ServiceInfo = ServiceInfo {
    id: "phases#Service",
    version: "1",
    operations: &[&INFO],
    errors: &[],
    classify: |_| None,
    reject: weather::SERVICE.reject,
    internal: weather::SERVICE.internal,
    unknown: weather::SERVICE.unknown,
};
struct Probe;
impl Operation for Probe {
    const INFO: &'static OperationInfo = &INFO;
    type Input = ProbeInput;
    type Output = Unit;
}
fn operations(events: &Events) -> Operations {
    Operations::builder(&SERVICE)
        .add::<Probe, _>(events.clone(), |events, input| {
            Box::pin(async move {
                record(&events, "operation");
                if input.value == 13 {
                    Err(SourceError("original").into())
                } else {
                    Ok(Unit {})
                }
            })
        })
        .build()
        .unwrap()
}
fn request(body: impl Into<Body>) -> Request<Body> {
    Request::post("/probe").body(body.into()).unwrap()
}
async fn text(response: Response) -> String {
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[derive(Clone)]
struct Marker;
struct Transport {
    events: Events,
}
impl Middleware for Transport {
    fn before<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        assert!(job.operation().is_none());
        job.context().set_attribute(Marker);
        record(&self.events, "transport.before");
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, _: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        record(&self.events, "transport.after");
        response
            .headers_mut()
            .insert("x-native", HeaderValue::from_static("transformed"));
        if response.status() == StatusCode::OK {
            *response.status_mut() = StatusCode::CREATED;
        }
        Hook::Ready(())
    }
    fn complete(&self, _: &Job, response: &Response) {
        assert_eq!(response.headers()["x-native"], "transformed");
        record(
            &self.events,
            format!("transport.complete:{}", response.status().as_u16()),
        );
    }
}
struct Admission {
    events: Events,
}
impl OperationAwareMiddleware for Admission {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        assert_eq!(job.operation().id, INFO.id);
        assert!(
            job.context().attribute::<Marker>().is_some(),
            "context attributes survive identification"
        );
        record(&self.events, "admission.before");
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, _: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        record(&self.events, "admission.after");
        Hook::Ready(())
    }
    fn complete(&self, _: &OperationJob<'_>, response: &Response) {
        record(
            &self.events,
            format!("admission.complete:{}", response.status().as_u16()),
        );
    }
}
struct Invocation {
    events: Events,
}
impl Interceptor for Invocation {
    fn before<'a>(
        &'a self,
        job: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        let input = job
            .input()
            .as_any()
            .downcast_ref::<ProbeInput>()
            .expect("typed input");
        assert!(input.value >= 1, "validation precedes invocation hooks");
        record(&self.events, "interceptor.before");
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, _: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        record(&self.events, "interceptor.after");
        Hook::Ready(())
    }
    fn complete(&self, _: &OperationJob<'_>, response: &Response) {
        record(
            &self.events,
            format!("interceptor.complete:{}", response.status().as_u16()),
        );
    }
}
struct Http {
    events: Events,
}
impl Protocol for Http {
    fn route(
        &self,
        request: &axum::http::request::Parts,
        operations: &Operations,
    ) -> Option<Route> {
        record(&self.events, "identify");
        JsonProtocol.route(request, operations)
    }
    fn decode<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        record(&self.events, "decode");
        JsonProtocol.decode(job)
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        record(&self.events, "encode");
        JsonProtocol.write(job, answers)
    }
}
fn service(events: &Events) -> Crucible {
    // Registration order across families cannot change the phase contract.
    Crucible::builder(operations(events))
        .interceptor(Invocation {
            events: events.clone(),
        })
        .protocol(Http {
            events: events.clone(),
        })
        .operation_middleware(Admission {
            events: events.clone(),
        })
        .middleware(Transport {
            events: events.clone(),
        })
        .build()
        .unwrap()
}

#[tokio::test]
async fn phases_and_reverse_unwind_preserve_context_and_observe_final_response() {
    let events = Events::default();
    let response = service(&events).handle(request(r#"{"value":7}"#)).await;
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["x-native"], "transformed");
    assert_eq!(
        *events.lock().unwrap(),
        [
            "transport.before",
            "identify",
            "admission.before",
            "decode",
            "interceptor.before",
            "operation",
            "interceptor.after",
            "admission.after",
            "encode",
            "transport.after",
            "interceptor.complete:201",
            "admission.complete:201",
            "transport.complete:201",
        ]
    );
}

#[tokio::test]
async fn malformed_and_constraint_invalid_inputs_never_enter_interceptors() {
    for body in [r#"{"value":0}"#, r#"{"value":"invalid"}"#, "{"] {
        let events = Events::default();
        let response = service(&events).handle(request(body)).await;
        assert_eq!(response.status(), 400);
        assert_eq!(
            *events.lock().unwrap(),
            [
                "transport.before",
                "identify",
                "admission.before",
                "decode",
                "admission.after",
                "encode",
                "transport.after",
                "admission.complete:400",
                "transport.complete:400",
            ]
        );
    }
}

#[derive(Debug)]
struct SourceError(&'static str);
impl std::fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for SourceError {}

#[tokio::test]
async fn first_transport_failure_is_translated_even_without_entered_hooks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let translator = ExceptionTranslator::new().translate(move |_: &SourceError| {
        count.fetch_add(1, Ordering::SeqCst);
        Rejection::malformed("", "translated early failure").into()
    });
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .middleware(raw_before_fn(|_| Err(SourceError("PRIVATE_SECRET").into())))
        .exception_translator(translator)
        .protocol(Http {
            events: events.clone(),
        })
        .build()
        .unwrap();
    let response = service.handle(request(r#"{"value":7}"#)).await;
    assert_eq!(response.status(), 400);
    let body = text(response).await;
    assert!(body.contains("translated early failure"));
    assert!(!body.contains("PRIVATE_SECRET"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*events.lock().unwrap(), ["encode"]);
}

struct ReplaceFailure;
impl OperationAwareMiddleware for ReplaceFailure {
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        let Some(Resolution::Rejected(rejection)) = job.resolution() else {
            panic!("the original failure must be normalized before return hooks");
        };
        assert_eq!(rejection.summary(), "mapped original");
        job.replace_error(SourceError("replacement").into());
        Hook::Ready(())
    }
}
struct MappedObserver(Events);
impl OperationAwareMiddleware for MappedObserver {
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        let Some(resolution @ Resolution::Rejected(_)) = job.resolution() else {
            panic!("outer hooks must observe normalized errors");
        };
        let status = resolution.status(Some(job.operation()));
        let Resolution::Rejected(rejection) = resolution else {
            unreachable!()
        };
        record(&self.0, format!("{}:{status}", rejection.summary()));
        Hook::Ready(())
    }
}
#[tokio::test]
async fn error_replacements_are_normalized_once_before_outer_hooks_and_encoding() {
    let translated = Events::default();
    let captured = translated.clone();
    let translator = ExceptionTranslator::new().translate(move |error: &SourceError| {
        record(&captured, error.0);
        Rejection::malformed("", format!("mapped {}", error.0)).into()
    });
    let events = Events::default();
    let observed = Events::default();
    let service = Crucible::builder(operations(&events))
        .operation_middleware(MappedObserver(observed.clone()))
        .operation_middleware(ReplaceFailure)
        .exception_translator(translator)
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = service.handle(request(r#"{"value":13}"#)).await;
    assert_eq!(response.status(), 400);
    assert!(text(response).await.contains("mapped replacement"));
    assert_eq!(*translated.lock().unwrap(), ["original", "replacement"]);
    assert_eq!(*observed.lock().unwrap(), ["mapped replacement:400"]);
}

#[derive(Clone)]
struct Envelope(Value);
struct Rpc {
    parsed: Arc<AtomicUsize>,
    decoded: Arc<AtomicUsize>,
}
impl Protocol for Rpc {
    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, crucible_server_runtime_rust::Result<ProtocolMatch>> {
        if job.request().uri.path() != "/rpc" {
            return Hook::Ready(Ok(ProtocolMatch::Declined));
        }
        Hook::wait(async move {
            job.read_body().await?;
            self.parsed.fetch_add(1, Ordering::SeqCst);
            let envelope: Value = serde_json::from_slice(job.body())
                .map_err(|_| Rejection::malformed("", "malformed RPC envelope"))?;
            let name = envelope
                .get("operation")
                .and_then(Value::as_str)
                .ok_or_else(|| Rejection::malformed("operation", "missing operation"))?;
            let Some(operation) = operations.service().operation(name) else {
                return Ok(ProtocolMatch::Claimed(None));
            };
            if !applies(operation) {
                return Ok(ProtocolMatch::Declined);
            }
            job.context().set_attribute(Envelope(envelope));
            Ok(ProtocolMatch::Claimed(Some(Route {
                operation: operation.name,
                labels: Vec::new(),
                rejection: None,
            })))
        })
    }
    fn decode<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.decoded.fetch_add(1, Ordering::SeqCst);
        let result = (|| {
            let envelope = job
                .context()
                .attribute::<Envelope>()
                .expect("retained parsed envelope");
            let input = job.decode(envelope.0["input"].clone())?;
            job.set_input(input);
            Ok(())
        })();
        Hook::Ready(result)
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        let mut response = JsonProtocol.write(job, answers);
        response
            .headers_mut()
            .insert("x-protocol", HeaderValue::from_static("rpc"));
        response
    }
}
struct Fallback {
    identified: Arc<AtomicUsize>,
}
impl Protocol for Fallback {
    fn route(&self, _: &axum::http::request::Parts, _: &Operations) -> Option<Route> {
        self.identified.fetch_add(1, Ordering::SeqCst);
        Some(Route {
            operation: "Probe",
            labels: Vec::new(),
            rejection: None,
        })
    }
    fn decode<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        JsonProtocol.decode(job)
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        JsonProtocol.write(job, answers)
    }
}
fn rpc(parsed: &Arc<AtomicUsize>, decoded: &Arc<AtomicUsize>) -> Rpc {
    Rpc {
        parsed: parsed.clone(),
        decoded: decoded.clone(),
    }
}
fn rpc_request(body: impl Into<Body>) -> Request<Body> {
    Request::post("/rpc").body(body.into()).unwrap()
}

#[tokio::test]
async fn async_rpc_identification_retains_input_and_reads_the_transport_once() {
    let parsed = Arc::new(AtomicUsize::new(0));
    let decoded = Arc::new(AtomicUsize::new(0));
    let frames = Arc::new(AtomicUsize::new(0));
    let count = frames.clone();
    let body = Body::from_stream(futures_util::stream::once(async move {
        count.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::io::Error>(Bytes::from_static(
            br#"{"operation":"Probe","input":{"value":7}}"#,
        ))
    }));
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .middleware(Transport {
            events: events.clone(),
        })
        .protocol(rpc(&parsed, &decoded))
        .operation_middleware(Admission {
            events: events.clone(),
        })
        .interceptor(Invocation {
            events: events.clone(),
        })
        .build()
        .unwrap();
    assert_eq!(service.handle(rpc_request(body)).await.status(), 201);
    assert_eq!(parsed.load(Ordering::SeqCst), 1);
    assert_eq!(decoded.load(Ordering::SeqCst), 1);
    assert_eq!(frames.load(Ordering::SeqCst), 1);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "operation")
    );
}

#[tokio::test]
async fn malformed_or_unknown_rpc_claims_do_not_fall_through_to_another_protocol() {
    for body in [
        "{",
        r#"{"input":{}}"#,
        r#"{"operation":"Missing","input":{}}"#,
    ] {
        let parsed = Arc::new(AtomicUsize::new(0));
        let decoded = Arc::new(AtomicUsize::new(0));
        let fallback = Arc::new(AtomicUsize::new(0));
        let events = Events::default();
        let service = Crucible::builder(operations(&events))
            .protocol(rpc(&parsed, &decoded))
            .protocol(Fallback {
                identified: fallback.clone(),
            })
            .interceptor(Invocation {
                events: events.clone(),
            })
            .build()
            .unwrap();
        let response = service.handle(rpc_request(body)).await;
        assert_eq!(response.status(), 400);
        assert_eq!(response.headers()["x-protocol"], "rpc");
        assert_eq!(parsed.load(Ordering::SeqCst), 1);
        assert_eq!(decoded.load(Ordering::SeqCst), 0);
        assert_eq!(fallback.load(Ordering::SeqCst), 0);
        assert!(events.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn smaller_operation_limits_also_reject_bodies_buffered_during_identification() {
    let parsed = Arc::new(AtomicUsize::new(0));
    let decoded = Arc::new(AtomicUsize::new(0));
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .body_limit(1024)
        .operation_body_limit::<Probe>(2)
        .protocol(rpc(&parsed, &decoded))
        .middleware(Transport {
            events: events.clone(),
        })
        .operation_middleware(Admission {
            events: events.clone(),
        })
        .interceptor(Invocation {
            events: events.clone(),
        })
        .build()
        .unwrap();
    let response = service
        .handle(rpc_request(r#"{"operation":"Probe","input":{"value":7}}"#))
        .await;
    assert_eq!(response.status(), 400);
    assert!(text(response).await.contains("larger than 2 bytes"));
    assert_eq!(parsed.load(Ordering::SeqCst), 1);
    assert_eq!(decoded.load(Ordering::SeqCst), 0);
    assert_eq!(
        *events.lock().unwrap(),
        [
            "transport.before",
            "admission.before",
            "admission.after",
            "transport.after",
            "admission.complete:400",
            "transport.complete:400",
        ]
    );
}

#[tokio::test]
async fn an_async_protocol_scope_can_decline_an_excluded_operation() {
    let parsed = Arc::new(AtomicUsize::new(0));
    let decoded = Arc::new(AtomicUsize::new(0));
    let fallback = Arc::new(AtomicUsize::new(0));
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .protocol(rpc(&parsed, &decoded).only(&["Other"]))
        .protocol(Fallback {
            identified: fallback.clone(),
        })
        .build()
        .unwrap();
    let response = service
        .handle(rpc_request(r#"{"operation":"Probe","value":7}"#))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(parsed.load(Ordering::SeqCst), 1);
    assert_eq!(decoded.load(Ordering::SeqCst), 0);
    assert_eq!(fallback.load(Ordering::SeqCst), 1);
    assert_eq!(*events.lock().unwrap(), ["operation"]);
}

struct StreamTransform {
    polled: Arc<AtomicUsize>,
}
impl Middleware for StreamTransform {
    fn after<'a>(&'a self, _: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        let body = std::mem::replace(response.body_mut(), Body::empty());
        let polled = self.polled.clone();
        let prefix = futures_util::stream::once(async move {
            polled.fetch_add(1, Ordering::SeqCst);
            Ok::<_, axum::Error>(Bytes::from_static(b"native:"))
        });
        *response.body_mut() = Body::from_stream(prefix.chain(body.into_data_stream()));
        Hook::Ready(())
    }
    fn complete(&self, _: &Job, _: &Response) {
        assert_eq!(
            self.polled.load(Ordering::SeqCst),
            0,
            "response completion does not consume its body"
        );
    }
}
#[tokio::test]
async fn native_response_middleware_wraps_streams_without_claiming_body_completion() {
    let polled = Arc::new(AtomicUsize::new(0));
    let service = Crucible::builder(operations(&Events::default()))
        .middleware(StreamTransform {
            polled: polled.clone(),
        })
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = service.handle(request(r#"{"value":7}"#)).await;
    assert_eq!(response.status(), 200);
    assert_eq!(polled.load(Ordering::SeqCst), 0);
    assert_eq!(text(response).await, "native:");
    assert_eq!(polled.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_scoped_invoke_protocol_leaves_known_excluded_operations_to_the_next_protocol() {
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .protocol(InvokeProtocol.only(&["Other"]))
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    let response = service.invoke("Probe", json!({ "value": 7 })).await;
    assert_eq!(response, json!({ "output": {} }));
    assert_eq!(*events.lock().unwrap(), ["operation"]);
}

struct Snapshot;
impl OperationAwareMiddleware for Snapshot {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        job.snapshot_input();
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        assert_eq!(job.input_snapshot(), Some(&json!({ "value": 7 })));
        Hook::Ready(())
    }
}
struct RefuseInvocation;
impl Interceptor for RefuseInvocation {
    fn before<'a>(
        &'a self,
        _: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        Hook::Ready(Err(
            Rejection::constraint("", "invocation policy refused").into()
        ))
    }
}
#[tokio::test]
async fn requested_input_snapshots_survive_interceptor_rejection() {
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .operation_middleware(Snapshot)
        .interceptor(RefuseInvocation)
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = service.handle(request(r#"{"value":7}"#)).await;
    assert_eq!(response.status(), 400);
    assert!(events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn raw_middleware_cannot_prepopulate_input_to_bypass_protocol_validation() {
    let events = Events::default();
    let service = Crucible::builder(operations(&events))
        .middleware(raw_before_fn(|job| {
            job.set_input(Box::new(ProbeInput { value: 7 }));
            Ok(())
        }))
        .interceptor(Invocation {
            events: events.clone(),
        })
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = service.handle(request(r#"{"value":0}"#)).await;
    assert_eq!(response.status(), 400);
    assert!(events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn service_modeled_errors_from_transport_middleware_keep_their_status_before_identification()
{
    let weather = Arc::new(weather_service::Weather::new(Arc::new(
        weather_service::MemoryCities::default(),
    )));
    let service = Crucible::builder(weather::operations(weather))
        .middleware(raw_before_fn(|job| {
            assert!(job.operation().is_none());
            Err(weather::NotFoundError {
                message: "transport resource absent".into(),
                resource_id: None,
            }
            .into())
        }))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    for path in ["/ping", "/no-operation"] {
        let response = service
            .handle(Request::get(path).body(Body::empty()).unwrap())
            .await;
        assert_eq!(response.status(), 404);
        let body: Value = serde_json::from_str(&text(response).await).unwrap();
        assert_eq!(body["__type"], "NotFoundError");
        assert_eq!(body["message"], "transport resource absent");
    }
}

#[tokio::test]
async fn unmodeled_transport_failures_are_sanitized_before_any_operation_is_known() {
    let service = Crucible::builder(operations(&Events::default()))
        .middleware(raw_before_fn(|_| {
            Err(SourceError("PRIVATE_EARLY_FAILURE").into())
        }))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    let response = service
        .handle(Request::get("/no-operation").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status(), 500);
    let body = text(response).await;
    assert!(body.contains("InternalFailure"));
    assert!(!body.contains("PRIVATE_EARLY_FAILURE"));
}
