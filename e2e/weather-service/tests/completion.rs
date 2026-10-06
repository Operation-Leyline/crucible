//! Completion observes the protocol response; streaming direct invokes fail safely.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Request, header::SET_COOKIE};
use axum::response::Response;
use crucible::before_fn;
use crucible::middlewares::{Metrics, Recorder, RequestRecord};
use crucible::operation::{Location, MemberInfo, Redaction, Scalar, Segment, Unit};
use crucible::validate::{Validate, Validator};
use crucible::{
    ByteStream, Crucible, InvokeProtocol, JsonProtocol, Operation, OperationInfo, ServiceInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weather_service::contract;

const BODY: MemberInfo = MemberInfo {
    name: "body",
    location: Location::StreamingPayload {
        media_type: "application/octet-stream",
        requires_length: false,
        minimum: None,
        maximum: None,
    },
    scalar: Scalar::Json,
    redaction: &Redaction::Public,
    traits: &[],
};
const TAG: MemberInfo = MemberInfo {
    name: "tag",
    location: Location::Header("x-tag"),
    scalar: Scalar::String,
    redaction: &Redaction::Public,
    traits: &[],
};
static UPLOAD: OperationInfo = OperationInfo {
    id: "test.completion#Upload",
    name: "Upload",
    method: "PUT",
    path: &[Segment::Literal("upload")],
    input: &[BODY, TAG],
    ..*contract::Ping::INFO
};
static DOWNLOAD: OperationInfo = OperationInfo {
    id: "test.completion#Download",
    name: "Download",
    path: &[Segment::Literal("download")],
    output: &[BODY],
    ..*contract::Ping::INFO
};
static BROKEN: OperationInfo = OperationInfo {
    id: "test.completion#Broken",
    name: "Broken",
    path: &[Segment::Literal("broken")],
    output: &[MemberInfo {
        name: "value",
        location: Location::Body,
        scalar: Scalar::String,
        redaction: &Redaction::Public,
        traits: &[],
    }],
    ..*contract::Ping::INFO
};
static FAULT: OperationInfo = OperationInfo {
    id: "test.completion#Fault",
    name: "Fault",
    path: &[Segment::Literal("fault")],
    errors: &["ConflictError"],
    ..*contract::Ping::INFO
};
static SERVICE: ServiceInfo = ServiceInfo {
    id: "test.completion#Service",
    operations: &[&UPLOAD, &DOWNLOAD, &BROKEN, &FAULT],
    ..contract::SERVICE
};

#[derive(Serialize, Deserialize)]
struct StreamInput {
    body: ByteStream,
    tag: String,
}
impl Validate for StreamInput {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize)]
struct StreamOutput {
    body: ByteStream,
}
struct Upload;
impl Operation for Upload {
    const INFO: &'static OperationInfo = &UPLOAD;
    type Input = StreamInput;
    type Output = ();
    fn input_stream(input: &StreamInput) -> Option<ByteStream> {
        Some(input.body.clone())
    }
    fn set_input_stream(input: &mut StreamInput, body: ByteStream) {
        input.body = body;
    }
}
struct Download;
impl Operation for Download {
    const INFO: &'static OperationInfo = &DOWNLOAD;
    type Input = Unit;
    type Output = StreamOutput;
    fn output_stream(output: &StreamOutput) -> Option<ByteStream> {
        Some(output.body.clone())
    }
}
struct Broken;
struct BrokenOutput;
impl Serialize for BrokenOutput {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        std::thread::sleep(Duration::from_millis(20));
        Err(serde::ser::Error::custom("failed output serialization"))
    }
}
impl Operation for Broken {
    const INFO: &'static OperationInfo = &BROKEN;
    type Input = Unit;
    type Output = BrokenOutput;
}
struct Fault;
impl Operation for Fault {
    const INFO: &'static OperationInfo = &FAULT;
    type Input = Unit;
    type Output = ();
}
#[derive(Default)]
struct Calls {
    uploads: AtomicUsize,
    downloads: AtomicUsize,
}
const BYTES: &[u8] = &[0xff, 0, 1];
fn builder(calls: Arc<Calls>) -> crucible::Builder {
    let downloads = calls.clone();
    let operations = crucible::Operations::builder(&SERVICE)
        .add::<Upload, _>(calls, |calls, input| {
            Box::pin(async move {
                calls.uploads.fetch_add(1, Ordering::SeqCst);
                assert_eq!(input.tag, "ordinary field");
                let bytes = axum::body::to_bytes(Body::from_stream(input.body.open()?), 16).await?;
                assert_eq!(bytes.as_ref(), BYTES);
                Ok(())
            })
        })
        .add::<Download, _>(downloads, |calls, _| {
            Box::pin(async move {
                calls.downloads.fetch_add(1, Ordering::SeqCst);
                Ok(StreamOutput {
                    body: ByteStream::from_bytes(BYTES),
                })
            })
        })
        .add::<Broken, _>(Arc::new(()), |_, _| Box::pin(async { Ok(BrokenOutput) }))
        .add::<Fault, _>(Arc::new(()), |_, _| {
            Box::pin(async {
                Err(contract::errors::ConflictError {
                    message: "modelled conflict".into(),
                }
                .into())
            })
        })
        .build()
        .unwrap();
    Crucible::builder(operations).on_rejection(|rejection| {
        Box::new(contract::errors::ConflictError {
            message: rejection.summary(),
        })
    })
}

#[tokio::test]
async fn streaming_direct_invokes_are_rejected_before_implementation_and_http_still_streams() {
    let calls = Arc::new(Calls::default());
    let service = builder(calls.clone())
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    let input = StreamInput {
        body: ByteStream::from_bytes(BYTES),
        tag: "ordinary field".into(),
    };
    for (operation, input) in [
        ("Upload", serde_json::to_value(input).unwrap()),
        ("Download", json!({})),
    ] {
        let answer = service.invoke(operation, input).await;
        assert_eq!(answer["error"]["__type"], "ConflictError");
        assert_eq!(
            answer["error"]["message"],
            "direct invocation does not support streaming payloads; use HTTP"
        );
        assert!(answer.get("output").is_none());
    }
    assert_eq!(calls.uploads.load(Ordering::SeqCst), 0);
    assert_eq!(calls.downloads.load(Ordering::SeqCst), 0);
    let response = service
        .handle(
            Request::put("/upload")
                .header("x-tag", "ordinary field")
                .body(Body::from(BYTES))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 200);
    let response = service
        .handle(Request::get("/download").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 16)
            .await
            .unwrap()
            .as_ref(),
        BYTES
    );
    assert_eq!(calls.uploads.load(Ordering::SeqCst), 1);
    assert_eq!(calls.downloads.load(Ordering::SeqCst), 1);
}

fn http_v2_event() -> Value {
    json!({"version":"2.0", "routeKey":"$default", "rawPath":"/ping", "rawQueryString":"", "headers":{},
        "requestContext":{"accountId":"123456789012", "apiId":"api", "domainName":"example.test",
            "domainPrefix":"example", "http":{"method":"GET", "path":"/ping", "protocol":"HTTP/1.1",
                "sourceIp":"192.0.2.1", "userAgent":"test"}, "requestId":"id", "routeKey":"$default",
            "stage":"$default", "time":"04/Oct/2026:12:00:00 +0000", "timeEpoch":1790000000000_i64},
        "isBase64Encoded":false})
}
#[tokio::test]
async fn http_and_lambda_v2_preserve_cookies_and_repeated_headers_for_normal_and_answered_responses()
 {
    let expected = [
        "session=a; Path=/",
        "csrf=b; Path=/",
        "label=present; Path=/café",
    ];
    for job_headers in [true, false] {
        let service = weather_service::builder(Arc::new(weather_service::MemoryCities::default()))
            .operation_middleware(before_fn(move |job| {
                let mut response = Response::new(Body::empty());
                let headers = if job_headers {
                    job.response_headers_mut()
                } else {
                    response.headers_mut()
                };
                for cookie in expected {
                    headers.append(
                        SET_COOKIE,
                        HeaderValue::from_bytes(cookie.as_bytes()).unwrap(),
                    );
                }
                headers.append("x-values", HeaderValue::from_static("first"));
                headers.append(
                    "x-values",
                    HeaderValue::from_bytes("café".as_bytes()).unwrap(),
                );
                headers.insert(
                    "x-single",
                    HeaderValue::from_bytes("séul".as_bytes()).unwrap(),
                );
                if !job_headers {
                    job.respond(response);
                }
                Ok(())
            }))
            .build()
            .unwrap();
        let response = service
            .handle(Request::get("/ping").body(Body::empty()).unwrap())
            .await;
        let cookies: Vec<_> = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(|value| std::str::from_utf8(value.as_bytes()).unwrap())
            .collect();
        assert_eq!(cookies, expected);
        let values: Vec<_> = response
            .headers()
            .get_all("x-values")
            .iter()
            .map(|value| value.as_bytes())
            .collect();
        assert_eq!(values, [b"first".as_slice(), "café".as_bytes()]);
        assert_eq!(response.headers()["x-single"].as_bytes(), "séul".as_bytes());
        let event = crucible::host::lambda_event(&service, http_v2_event())
            .await
            .unwrap();
        assert_eq!(event["cookies"], json!(expected));
        assert_eq!(event["headers"]["x-values"], "first,café");
        assert_eq!(event["headers"]["x-single"], "séul");
        assert!(event["headers"].get("set-cookie").is_none());
        assert_eq!(event["statusCode"], 200);
    }
}

#[tokio::test]
async fn lambda_v2_rejects_non_utf8_cookies_instead_of_emitting_empty_values() {
    let service = weather_service::builder(Arc::new(weather_service::MemoryCities::default()))
        .operation_middleware(before_fn(|job| {
            let mut response = Response::new(Body::empty());
            response
                .headers_mut()
                .append(SET_COOKIE, HeaderValue::from_bytes(&[0x80]).unwrap());
            job.respond(response);
            Ok(())
        }))
        .build()
        .unwrap();
    let response = service
        .handle(Request::get("/ping").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.headers()[SET_COOKIE].as_bytes(), &[0x80]);
    assert!(
        crucible::host::lambda_event(&service, http_v2_event())
            .await
            .is_err()
    );
}

#[derive(Debug, Clone)]
struct Record {
    status: u16,
    class: &'static str,
    error: Option<&'static str>,
    latency: Duration,
}
#[derive(Clone, Default)]
struct Records(Arc<Mutex<Vec<Record>>>);
impl Recorder for Records {
    fn record(&self, record: &RequestRecord<'_>) {
        self.0.lock().unwrap().push(Record {
            status: record.status,
            class: record.class,
            error: record.error,
            latency: record.latency,
        });
    }
}
#[tokio::test]
async fn metrics_record_serialization_failure_actual_validation_status_and_relay_completion() {
    let records = Records::default();
    let service = builder(Arc::new(Calls::default()))
        .middleware(Metrics::new(records.clone()))
        .operation_middleware(before_fn(|job| {
            if job.request().headers.contains_key("x-relay") {
                job.respond(
                    Response::builder()
                        .status(202)
                        .body(Body::from("relayed"))
                        .unwrap(),
                );
            }
            Ok(())
        }))
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    for (uri, body, status) in [
        ("/broken", "", 500),
        ("/fault", "", 409),
        ("/upload", "", 409),
    ] {
        let response = service
            .handle(
                Request::builder()
                    .method(if uri == "/upload" { "PUT" } else { "GET" })
                    .uri(uri)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), status);
    }
    let answer = service.invoke("Broken", json!({})).await;
    assert_eq!(answer["error"]["__type"], "InternalFailure");
    let response = service
        .handle(
            Request::get("/broken")
                .header("x-relay", "yes")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 202);
    let records = records.0.lock().unwrap();
    assert_eq!(
        records.len(),
        5,
        "exactly one metric per completed response, including a relay"
    );
    assert_eq!(
        (records[0].status, records[0].class, records[0].error),
        (500, "fault", Some("InternalFailure"))
    );
    assert!(
        records[0].latency >= Duration::from_millis(20),
        "serialization belongs to request latency"
    );
    assert_eq!(
        (records[1].status, records[1].class, records[1].error),
        (409, "error", Some("ConflictError"))
    );
    assert_eq!(
        (records[2].status, records[2].class, records[2].error),
        (409, "error", Some("ValidationError"))
    );
    assert_eq!(
        (records[3].status, records[3].class, records[3].error),
        (500, "fault", Some("InternalFailure"))
    );
    assert!(records[3].latency >= Duration::from_millis(20));
    assert_eq!(
        (records[4].status, records[4].class, records[4].error),
        (202, "success", None)
    );
}
