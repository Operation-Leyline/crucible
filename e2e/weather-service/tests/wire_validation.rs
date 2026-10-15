//! Independent consumers of malformed HTTP input and output handling.
// The fixture deprecates `Ping`; these tests still drive it.
#![allow(deprecated)]
use crucible_server_runtime_rust::__generated as generated;
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
    Crucible, InvokeProtocol, JsonProtocol, ModelledError, Operation, OperationInfo, ServiceInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weather_service::weather;

const fn member(name: &'static str, location: Location) -> MemberInfo {
    generated::member_info(name, location, Scalar::String, &Redaction::Public, &[])
}
const BODY: MemberInfo = member("body", Location::Body);
const HEADER: MemberInfo = member("header", Location::Header("x-header"));
static BODY_ONLY: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "BodyOnly",
        base.resource,
        base.method,
        &[Segment::Literal("body")],
        base.literal_query,
        base.status,
        &[BODY],
        &[BODY],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static BOUND: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Bound",
        base.resource,
        base.method,
        &[Segment::Literal("bound")],
        base.literal_query,
        base.status,
        &[BODY, HEADER],
        &[BODY],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static REQUIRED: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Required",
        base.resource,
        base.method,
        &[Segment::Literal("required")],
        base.literal_query,
        base.status,
        &[BODY],
        base.output,
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static REQUIRED_BOUND: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "RequiredBound",
        base.resource,
        base.method,
        &[Segment::Literal("required-bound")],
        base.literal_query,
        base.status,
        &[BODY, HEADER],
        base.output,
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static QUERIES: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Queries",
        base.resource,
        base.method,
        &[Segment::Literal("queries")],
        base.literal_query,
        base.status,
        &const {
            [{
                let base = &(member("tags", Location::Query("tag")));
                generated::member_info(
                    base.name,
                    base.location,
                    Scalar::List(&Scalar::String),
                    base.redaction,
                    base.traits,
                )
            }]
        },
        &[member("tags", Location::Body)],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static HEADERS: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Headers",
        base.resource,
        base.method,
        &[Segment::Literal("headers")],
        base.literal_query,
        base.status,
        base.input,
        &[BODY, HEADER],
        &["HeaderError"],
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static TEXT: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Text",
        base.resource,
        base.method,
        &[Segment::Literal("text")],
        base.literal_query,
        base.status,
        &[member("body", Location::TextPayload("text/plain"))],
        &[BODY],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static BLOB: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Blob",
        base.resource,
        base.method,
        &[Segment::Literal("blob")],
        base.literal_query,
        base.status,
        &[member(
            "body",
            Location::Payload("application/octet-stream"),
        )],
        &[BODY],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static DOCUMENT: OperationInfo = {
    let base = weather::Ping::INFO;
    generated::operation_info(
        base.id,
        "Document",
        base.resource,
        base.method,
        &[Segment::Literal("document")],
        base.literal_query,
        base.status,
        &[member(
            "body",
            Location::DocumentPayload("application/json"),
        )],
        &[BODY],
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};
static SERVICE: ServiceInfo = {
    let base = &(weather::SERVICE);
    generated::service_info(
        base.id,
        base.version,
        None,
        &[
            &BODY_ONLY,
            &BOUND,
            &REQUIRED,
            &REQUIRED_BOUND,
            &QUERIES,
            &HEADERS,
            &TEXT,
            &BLOB,
            &DOCUMENT,
        ],
        base.errors,
        |error| {
            error
                .downcast_ref::<HeaderError>()
                .map(|error| error as &dyn ModelledError)
        },
        base.reject,
        base.internal,
        base.unknown,
    )
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
struct QueryList {
    tags: Option<Vec<String>>,
}
impl Validate for QueryList {
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
operation!(Queries, QUERIES, QueryList, QueryList);
operation!(Headers, HEADERS, Unit, HeaderOutput);
operation!(Text, TEXT, TextBody, TextBody);
operation!(Blob, BLOB, BlobBody, Value);
operation!(Document, DOCUMENT, DocumentBody, DocumentBody);
static ERROR: ErrorInfo = generated::error_info(
    "HeaderError",
    409,
    Fault::Client,
    false,
    false,
    &[BODY, HEADER],
);
#[derive(Debug, Serialize)]
struct HeaderError {
    body: String,
    header: String,
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
        if mode == "value" {
            "private\r\ninvalid"
        } else {
            "valid"
        }
        .to_owned()
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
                let header = (state.0)();
                if crucible_server_runtime_rust::Context::current()
                    .headers()
                    .contains_key("x-error")
                {
                    return Err(HeaderError {
                        body: "private".into(),
                        header,
                    }
                    .into());
                }
                Ok(HeaderOutput {
                    body: "private".into(),
                    header,
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
async fn query_lists_reject_malformed_values_and_preserve_valid_unicode() {
    let service = service("valid", Records::default(), Records::default());
    for path in [
        "/queries?tag=%FE",
        "/queries?tag=%GG",
        "/queries?tag=%",
        "/queries?tag=valid&tag=%FF",
    ] {
        let (status, body) = answer(&service, path, b"").await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["reason"], "cannotParse");
    }
    assert_eq!(
        answer(&service, "/queries?tag=caf%C3%A9&tag=a+b&other=value", b"").await,
        (200, json!({"tags":["café","a b"]}))
    );
}

#[tokio::test]
async fn invalid_success_and_error_headers_become_observable_internal_failures() {
    for mode in ["value"] {
        let logs = Records::default();
        let metrics = Records::default();
        let service = service(mode, logs.clone(), metrics.clone());
        for (path, modelled_error) in [("/headers", false), ("/headers", true)] {
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
        assert_eq!(logs.len(), 2);
        assert_eq!(metrics.len(), 2);
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

/// A rejection built from the request's query lists at most 100
/// violations and counts the rest, and names a client's key `[key]`.
fn assert_capped(body: &Value, total: usize, path: &str) {
    let fields = body["fieldList"].as_array().unwrap();
    assert_eq!(fields.len(), 101, "{body}");
    assert!(
        fields[..100].iter().all(|field| field["path"] == path),
        "{body}"
    );
    assert_eq!(fields[100]["path"], "", "{body}");
    assert_eq!(
        fields[100]["message"],
        format!("{} more violations are not listed", total - 100)
    );
    let message = body["message"].as_str().unwrap();
    assert!(
        message.ends_with(&format!("(and {} more)", total - 1)),
        "{message}"
    );
    assert!(!body.to_string().contains("hunter"), "{body}");
}

#[tokio::test]
async fn query_rejections_are_capped_and_name_no_client_key() {
    let logs = Records::default();
    let service = service("valid", logs.clone(), Records::default());
    let query = |key: &str| {
        (0..250)
            .map(|index| format!("{key}{index}=%zz"))
            .collect::<Vec<_>>()
            .join("&")
    };
    // An undeclared key is no parameter at all.
    let (status, body) = answer(&service, &format!("/body?{}", query("hunter")), b"").await;
    assert_eq!(status, 400, "{body}");
    assert_capped(&body, 250, "query");
    // A declared parameter keeps the model's name; a key that does not
    // decode is no parameter either.
    let (_, body) = answer(&service, "/queries?tag=%zz", b"").await;
    assert_eq!(body["fieldList"][0]["path"], "tag", "{body}");
    let (_, body) = answer(&service, "/body?hunter%zz=1", b"").await;
    assert_eq!(body["fieldList"][0]["path"], "query", "{body}");

    let logs = serde_json::to_string(&*logs.0.lock().unwrap()).unwrap();
    assert!(!logs.contains("hunter"), "{logs}");
}

// serde's derived struct visitor also takes a sequence, so `[59.9, 10.7]`
// once read as `Coordinates` (#292). Smithy encodes a structure as a JSON
// object: Crucible JSON refuses an array in its place, at any depth.
#[tokio::test]
async fn crucible_json_refuses_an_array_where_a_structure_is_modelled() {
    let service = Crucible::builder(weather::operations(Arc::new(
        weather_service::Weather::new(Arc::new(weather_service::MemoryCities::default())),
    )))
    .protocol(JsonProtocol)
    .protocol(InvokeProtocol)
    .build()
    .unwrap();
    let send = |body: String| {
        let service = &service;
        async move {
            let response = service
                .handle(
                    Request::post("/cities")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await;
            let status = response.status().as_u16();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let invoke = |body: String| {
        let service = &service;
        async move {
            let answer = service.invoke_json("CreateCity", body).await;
            serde_json::from_slice::<Value>(&answer).unwrap()
        }
    };
    let city = |name: &str, coordinates: &str| {
        format!(r#"{{"name":"{name}","location":{{"coordinates":{coordinates}}}}}"#)
    };
    let valid = city("crucible-json", r#"{"latitude":59.9,"longitude":10.7}"#);
    let (status, body) = send(valid).await;
    assert!((200..300).contains(&status), "{status} {body}");
    for coordinates in ["[59.9, 10.7]", "[]"] {
        let (status, body) = send(city("crucible-json", coordinates)).await;
        assert_eq!(status, 400, "{coordinates}: {body}");
    }
    // The whole input as an array is no structure either.
    let (status, body) = send(r#"["Oslo"]"#.to_owned()).await;
    assert_eq!(status, 400, "{body}");
    let answer = invoke(city("invoke", r#"{"latitude":59.9,"longitude":10.7}"#)).await;
    assert!(answer.get("output").is_some(), "{answer}");
    for body in [city("invoke", "[59.9, 10.7]"), r#"["Oslo"]"#.to_owned()] {
        let answer = invoke(body.clone()).await;
        assert!(answer.get("error").is_some(), "{body}: {answer}");
    }
}
