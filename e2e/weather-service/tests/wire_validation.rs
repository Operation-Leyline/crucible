//! Independent consumers of malformed HTTP input and output handling.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::Request;
use crucible_server_runtime_rust::middlewares::{
    LogRecord, LogSink, Metrics, Recorder, RequestRecord, ServiceLog,
};
use crucible_server_runtime_rust::operation::{
    ErrorInfo, Fault, Location, MemberInfo, Redaction, Scalar, Segment, Unit,
};
use crucible_server_runtime_rust::validate::{Validate, Validator};
use crucible_server_runtime_rust::{
    ByteStream, Crucible, InvokeProtocol, JsonProtocol, ModelledError, Operation, OperationInfo,
    ServiceInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weather_service::weather;

const fn member(name: &'static str, location: Location) -> MemberInfo {
    MemberInfo {
        name,
        location,
        scalar: Scalar::String,
        redaction: &Redaction::Public,
        traits: &[],
    }
}
const BODY: MemberInfo = member("body", Location::Body);
const HEADER: MemberInfo = member("header", Location::Header("x-header"));
const PREFIX: MemberInfo = member("prefix", Location::PrefixHeaders("x-prefix-"));
static BODY_ONLY: OperationInfo = OperationInfo {
    name: "BodyOnly",
    path: &[Segment::Literal("body")],
    input: &[BODY],
    output: &[BODY],
    ..*weather::Ping::INFO
};
static BOUND: OperationInfo = OperationInfo {
    name: "Bound",
    path: &[Segment::Literal("bound")],
    input: &[BODY, HEADER],
    output: &[BODY],
    ..*weather::Ping::INFO
};
static REQUIRED: OperationInfo = OperationInfo {
    name: "Required",
    path: &[Segment::Literal("required")],
    input: &[BODY],
    ..*weather::Ping::INFO
};
static REQUIRED_BOUND: OperationInfo = OperationInfo {
    name: "RequiredBound",
    path: &[Segment::Literal("required-bound")],
    input: &[BODY, HEADER],
    ..*weather::Ping::INFO
};
static QUERIES: OperationInfo = OperationInfo {
    name: "Queries",
    path: &[Segment::Literal("queries")],
    input: &[
        member("queries", Location::QueryParams),
        MemberInfo {
            scalar: Scalar::List(&Scalar::String),
            ..member("tags", Location::Query("tag"))
        },
    ],
    output: &[
        member("queries", Location::Body),
        member("tags", Location::Body),
    ],
    ..*weather::Ping::INFO
};
static HEADERS: OperationInfo = OperationInfo {
    name: "Headers",
    path: &[Segment::Literal("headers")],
    output: &[BODY, HEADER, PREFIX],
    errors: &["HeaderError"],
    ..*weather::Ping::INFO
};
static STREAM: OperationInfo = OperationInfo {
    name: "Stream",
    path: &[Segment::Literal("stream")],
    output: &[
        member(
            "body",
            Location::StreamingPayload {
                media_type: "application/octet-stream",
                requires_length: false,
                minimum: None,
                maximum: None,
            },
        ),
        HEADER,
        PREFIX,
    ],
    ..*weather::Ping::INFO
};
static TEXT: OperationInfo = OperationInfo {
    name: "Text",
    path: &[Segment::Literal("text")],
    input: &[member("body", Location::TextPayload("text/plain"))],
    output: &[BODY],
    ..*weather::Ping::INFO
};
static BLOB: OperationInfo = OperationInfo {
    name: "Blob",
    path: &[Segment::Literal("blob")],
    input: &[member(
        "body",
        Location::Payload("application/octet-stream"),
    )],
    output: &[BODY],
    ..*weather::Ping::INFO
};
static DOCUMENT: OperationInfo = OperationInfo {
    name: "Document",
    path: &[Segment::Literal("document")],
    input: &[member(
        "body",
        Location::DocumentPayload("application/json"),
    )],
    output: &[BODY],
    ..*weather::Ping::INFO
};
static SERVICE: ServiceInfo = ServiceInfo {
    cors: None,
    operations: &[
        &BODY_ONLY,
        &BOUND,
        &REQUIRED,
        &REQUIRED_BOUND,
        &QUERIES,
        &HEADERS,
        &STREAM,
        &TEXT,
        &BLOB,
        &DOCUMENT,
    ],
    classify: |error| {
        error
            .downcast_ref::<HeaderError>()
            .map(|error| error as &dyn ModelledError)
    },
    ..weather::SERVICE
};

#[derive(Serialize, Deserialize)]
struct OptionalBody {
    body: Option<String>,
    header: Option<String>,
}
impl Validate for OptionalBody {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct RequiredBody {
    body: String,
    header: Option<String>,
}
impl Validate for RequiredBody {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct QueryMap {
    #[serde(default)]
    queries: BTreeMap<String, String>,
    tags: Option<Vec<String>>,
}
impl Validate for QueryMap {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct TextBody {
    body: String,
}
impl Validate for TextBody {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct BlobBody {
    body: crucible_server_runtime_rust::Blob,
}
impl Validate for BlobBody {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct DocumentBody {
    body: Option<Value>,
}
impl Validate for DocumentBody {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize)]
struct HeaderOutput {
    body: String,
    header: String,
    prefix: BTreeMap<String, String>,
}
#[derive(Serialize)]
struct StreamOutput {
    body: ByteStream,
    header: String,
    prefix: BTreeMap<String, String>,
}
macro_rules! operation {
    ($name:ident, $info:ident, $input:ty, $output:ty) => {
        struct $name;
        impl Operation for $name {
            const INFO: &'static OperationInfo = &$info;
            type Input = $input;
            type Output = $output;
        }
    };
}
operation!(BodyOnly, BODY_ONLY, OptionalBody, OptionalBody);
operation!(Bound, BOUND, OptionalBody, OptionalBody);
operation!(Required, REQUIRED, RequiredBody, Unit);
operation!(RequiredBound, REQUIRED_BOUND, RequiredBody, Unit);
operation!(Queries, QUERIES, QueryMap, QueryMap);
operation!(Headers, HEADERS, Unit, HeaderOutput);
operation!(Text, TEXT, TextBody, TextBody);
operation!(Blob, BLOB, BlobBody, Value);
operation!(Document, DOCUMENT, DocumentBody, DocumentBody);
struct Stream;
impl Operation for Stream {
    const INFO: &'static OperationInfo = &STREAM;
    type Input = Unit;
    type Output = StreamOutput;
    fn output_stream(output: &StreamOutput) -> Option<ByteStream> {
        Some(output.body.clone())
    }
}

static ERROR: ErrorInfo = ErrorInfo {
    name: "HeaderError",
    status: 409,
    fault: Fault::Client,
    retryable: false,
    throttling: false,
    members: &[BODY, HEADER, PREFIX],
};
#[derive(Debug, Serialize)]
struct HeaderError {
    body: String,
    header: String,
    prefix: BTreeMap<String, String>,
}
impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("header error")
    }
}
impl std::error::Error for HeaderError {}
impl ModelledError for HeaderError {
    fn info(&self) -> &'static ErrorInfo {
        &ERROR
    }
    fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
}

#[derive(Clone, Default)]
struct Records(Arc<Mutex<Vec<Value>>>);
impl LogSink for Records {
    fn write(&self, record: &LogRecord<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(serde_json::to_value(record).unwrap());
    }
}
impl Recorder for Records {
    fn record(&self, record: &RequestRecord<'_>) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"status":record.status,"class":record.class,"error":record.error}));
    }
}

fn service(mode: &'static str, logs: Records, metrics: Records) -> Crucible {
    let headers = move || {
        let header = if mode == "value" {
            "private\r\ninvalid"
        } else {
            "valid"
        }
        .to_owned();
        let prefix = if mode == "name" {
            BTreeMap::from([("bad name".to_owned(), "private".to_owned())])
        } else {
            BTreeMap::new()
        };
        (header, prefix)
    };
    let operations = crucible_server_runtime_rust::Operations::builder(&SERVICE)
        .add::<BodyOnly, _, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<Bound, _, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<Required, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<RequiredBound, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Queries, _, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<Text, _, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<Document, _, _>(Arc::new(()), |_, input| Box::pin(async { Ok(input) }))
        .add::<Blob, _, _>(Arc::new(()), |_, input| {
            Box::pin(async move { Ok(json!({"body": input.body.as_ref()})) })
        })
        .add::<Headers, _, _>(Arc::new((headers, mode)), |state, _| {
            Box::pin(async move {
                let (header, prefix) = (state.0)();
                if crucible_server_runtime_rust::Context::current()
                    .headers()
                    .contains_key("x-error")
                {
                    return Err(HeaderError {
                        body: "private".into(),
                        header,
                        prefix,
                    }
                    .into());
                }
                Ok(HeaderOutput {
                    body: "private".into(),
                    header,
                    prefix,
                })
            })
        })
        .add::<Stream, _, _>(Arc::new(headers), |headers, _| {
            Box::pin(async move {
                let (header, prefix) = headers();
                Ok(StreamOutput {
                    body: ByteStream::from_bytes("private stream"),
                    header,
                    prefix,
                })
            })
        })
        .build()
        .unwrap();
    Crucible::builder(operations)
        .middleware(ServiceLog::errors_only().sink(logs))
        .middleware(Metrics::new(metrics))
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap()
}

async fn answer(service: &Crucible, path: &str, bytes: &'static [u8]) -> (u16, Value) {
    let response = service
        .handle(
            Request::get(path)
                .header("content-type", "application/json")
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await;
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn empty_json_and_required_members_are_consistent_across_http_bindings_and_invoke() {
    let service = service("valid", Records::default(), Records::default());
    for bytes in [b"".as_slice(), b" \t\r\n"] {
        for path in ["/body", "/bound", "/document"] {
            assert_eq!(answer(&service, path, bytes).await.0, 200);
        }
        for path in ["/required", "/required-bound"] {
            let (status, body) = answer(&service, path, bytes).await;
            assert_eq!(status, 400, "{path}: {body}");
            assert_eq!(body["reason"], "fieldValidationFailed");
            assert!(body["message"].as_str().unwrap().contains("body"));
        }
        let answer: Value =
            serde_json::from_slice(&service.invoke_json("BodyOnly", bytes).await).unwrap();
        assert!(answer.get("output").is_some());
    }
    for bytes in [b"\x0b".as_slice(), b"\x0c", b" \x0b "] {
        for path in [
            "/body",
            "/bound",
            "/required",
            "/required-bound",
            "/document",
        ] {
            let (status, body) = answer(&service, path, bytes).await;
            assert_eq!(status, 400, "{path}: {body}");
            assert_eq!(body["reason"], "cannotParse");
        }
    }
    for bytes in [b" \t\r\n".as_slice(), b"\x0b\x0c"] {
        assert_eq!(
            answer(&service, "/text", bytes).await,
            (200, json!({"body":std::str::from_utf8(bytes).unwrap()}))
        );
        assert_eq!(
            answer(&service, "/blob", bytes).await,
            (200, json!({"body":bytes}))
        );
    }
}

#[tokio::test]
async fn json_document_payloads_also_reject_incompatible_media_types() {
    let service = service("valid", Records::default(), Records::default());
    for path in ["/body", "/bound", "/document"] {
        let response = service
            .handle(
                Request::get(path)
                    .header("content-type", "text/plain")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 415, "{path}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["__type"], "UnsupportedMediaTypeException");
    }
}

#[tokio::test]
async fn query_maps_reject_malformed_keys_and_values_and_preserve_valid_unicode() {
    let service = service("valid", Records::default(), Records::default());
    for path in [
        "/queries?%FF=value",
        "/queries?key=%FE",
        "/queries?key=%GG",
        "/queries?key=%",
        "/queries?tag=valid&tag=%FF",
    ] {
        let (status, body) = answer(&service, path, b"").await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["reason"], "cannotParse");
    }
    assert_eq!(
        answer(&service, "/queries?caf%C3%A9=%EF%BF%BD+%2B", b"").await,
        (200, json!({"queries":{"café":"� +"}, "tags":null}))
    );
    assert_eq!(
        answer(&service, "/queries?tag=caf%C3%A9&tag=a+b&key=value", b"").await,
        (
            200,
            json!({"queries":{"key":"value"},"tags":["café","a b"]})
        )
    );
}

#[tokio::test]
async fn invalid_success_error_and_stream_headers_become_observable_internal_failures() {
    for mode in ["value", "name"] {
        let logs = Records::default();
        let metrics = Records::default();
        let service = service(mode, logs.clone(), metrics.clone());
        for (path, modelled_error) in [("/headers", false), ("/headers", true), ("/stream", false)]
        {
            let mut request = Request::get(path);
            if modelled_error {
                request = request.header("x-error", "yes");
            }
            let response = service.handle(request.body(Body::empty()).unwrap()).await;
            assert_eq!(response.status(), 500);
            assert_eq!(response.headers()["content-type"], "application/json");
            assert!(!response.headers().contains_key("x-header"));
            assert!(!response.headers().contains_key("content-length"));
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["__type"], "InternalFailure");
            assert!(
                !String::from_utf8(bytes.to_vec())
                    .unwrap()
                    .contains("private")
            );
        }
        let logs = logs.0.lock().unwrap();
        let metrics = metrics.0.lock().unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(metrics.len(), 3);
        for log in logs.iter() {
            assert_eq!(log["status"], 500);
            assert_eq!(log["outcome"], "fault");
            assert_eq!(log["error"], "InternalFailure");
            assert!(
                log["cause"]
                    .as_str()
                    .unwrap()
                    .contains("invalid response header")
            );
            assert!(!log["cause"].as_str().unwrap().contains("private"));
        }
        for metric in metrics.iter() {
            assert_eq!(
                metric,
                &json!({"status":500,"class":"fault","error":"InternalFailure"})
            );
        }
    }
}
