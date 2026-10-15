//! Public framework consumers exercising HTTP bindings and protocol selection.
use crucible_server_runtime_rust::__generated as generated;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request},
};
use crucible_server_runtime_rust::before_fn;
use crucible_server_runtime_rust::operation::{
    ErrorInfo, Fault, Location, MemberInfo, OperationInfo, Redaction, Scalar, Segment, ServiceInfo,
    Unit,
};
use crucible_server_runtime_rust::protocol::Invocation;
use crucible_server_runtime_rust::validate::{Validate, Validator};
use crucible_server_runtime_rust::{
    Crucible, InvokeProtocol, JsonProtocol, Operation, Operations, Protocol, ProtocolExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const fn member(name: &'static str, location: Location, scalar: Scalar) -> MemberInfo {
    generated::member_info(name, location, scalar, &Redaction::Public, &[])
}

static OUTPUT: &[MemberInfo] = &[member("observed", Location::Body, Scalar::Json)];

const fn operation(
    name: &'static str,
    path: &'static [Segment],
    query: &'static [(&'static str, Option<&'static str>)],
) -> OperationInfo {
    generated::operation_info(
        "audit#Operation",
        name,
        None,
        "POST",
        path,
        query,
        200,
        &[],
        OUTPUT,
        &[],
        false,
        false,
        false,
        None,
        &[],
    )
}

static BINDINGS: OperationInfo = {
    let base = &(operation("Bindings", &[Segment::Literal("bindings")], &[]));
    generated::operation_info(
        base.id,
        base.name,
        base.resource,
        base.method,
        base.path,
        base.literal_query,
        base.status,
        &[
            member(
                "headerDisplay",
                Location::Header("x-required"),
                Scalar::String,
            ),
            member("queryDisplay", Location::Query("q"), Scalar::String),
            member("bodyDisplay", Location::Body, Scalar::String),
            member(
                "strings",
                Location::Header("x-strings"),
                Scalar::List(&Scalar::String),
            ),
            member(
                "dates",
                Location::Header("x-dates"),
                Scalar::List(&Scalar::HttpDate),
            ),
            member(
                "encoded",
                Location::Header("x-encoded"),
                Scalar::List(&Scalar::Base64),
            ),
            member(
                "numbers",
                Location::Header("x-numbers"),
                Scalar::List(&Scalar::Integer),
            ),
        ],
        base.output,
        base.errors,
        base.readonly,
        base.idempotent,
        base.internal,
        base.idempotency_token,
        base.traits,
    )
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BindingInput {
    #[serde(rename = "headerDisplay")]
    header: String,
    #[serde(rename = "queryDisplay")]
    query: Option<String>,
    #[serde(rename = "bodyDisplay")]
    body: String,
    #[serde(default)]
    strings: Vec<String>,
    #[serde(default)]
    dates: Vec<String>,
    #[serde(default)]
    encoded: Vec<String>,
    #[serde(default)]
    numbers: Vec<i32>,
}

impl Validate for BindingInput {
    fn validate(&self, validator: &mut Validator) {
        validator.at("bodyDisplay", |validator| {
            validator.length(self.body.len(), Some(1), None)
        });
    }
}

struct Bindings;
impl Operation for Bindings {
    const INFO: &'static OperationInfo = &BINDINGS;
    type Input = BindingInput;
    type Output = Value;
}

static ERROR: ErrorInfo = generated::error_info(
    "Invalid",
    400,
    Fault::Client,
    false,
    false,
    &[member("message", Location::Body, Scalar::String)],
);
#[derive(Debug)]
struct Invalid(String);
impl std::fmt::Display for Invalid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for Invalid {}
impl crucible_server_runtime_rust::ModelledError for Invalid {
    fn info(&self) -> &'static ErrorInfo {
        &ERROR
    }
    fn to_json(&self) -> Value {
        json!({"message": self.0})
    }
}
static SERVICE: ServiceInfo = generated::service_info(
    "audit#Bindings",
    "1",
    None,
    None,
    &[&BINDINGS],
    &[&ERROR],
    |_| None,
    Some(|rejection| Box::new(Invalid(rejection.summary()))),
    None,
    None,
);

fn service(calls: Arc<AtomicUsize>) -> Crucible {
    let operations = Operations::builder(&SERVICE)
        .add::<Bindings, _, _>(calls, |calls, input| {
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"observed": input}))
            })
        })
        .build()
        .unwrap();
    Crucible::builder(operations)
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .build()
        .unwrap()
}

async fn answer(service: &Crucible, request: Request<Body>) -> (u16, Value) {
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn absent_http_bindings_cannot_be_supplied_in_the_json_body() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(calls.clone());
    let (status, _) = answer(
        &service,
        Request::post("/bindings")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"headerDisplay":"injected","queryDisplay":"injected","bodyDisplay":"body"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (status, body) = answer(&service, Request::post("/bindings").header("x-required", "header")
        .header("content-type", "application/json").body(Body::from(r#"{"headerDisplay":"ignored","queryDisplay":"injected","strings":["injected"],"bodyDisplay":"body"}"#)).unwrap()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["observed"]["headerDisplay"], "header");
    assert!(body["observed"]["queryDisplay"].is_null());
    assert_eq!(body["observed"]["strings"], json!([]));

    let (status, body) = answer(
        &service,
        Request::post("/bindings?q=query")
            .header("x-required", "header")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"queryDisplay":"ignored","bodyDisplay":"body"}"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["observed"]["queryDisplay"], "query");
    assert_eq!(body["observed"]["bodyDisplay"], "body");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn repeated_header_fields_preserve_lists_quotes_dates_and_media_type_strings() {
    let service = service(Arc::new(AtomicUsize::new(0)));
    let request = Request::post("/bindings")
        .header("x-required", "header")
        .header("x-strings", "plain,\"a,b\"")
        .header("x-strings", "\"say \\\"hi\\\"\",last")
        .header("x-dates", "Mon, 16 Dec 2019 23:48:18 GMT")
        .header("x-dates", "Tue, 17 Dec 2019 23:48:18 GMT")
        .header("x-encoded", "YQ==,Yg==")
        .header("x-encoded", "Yyxk")
        .header("x-numbers", "1,2")
        .header("x-numbers", "3")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"bodyDisplay":"body"}"#))
        .unwrap();
    let (status, body) = answer(&service, request).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["observed"]["strings"],
        json!(["plain", "a,b", "say \"hi\"", "last"])
    );
    assert_eq!(
        body["observed"]["dates"],
        json!([
            "Mon, 16 Dec 2019 23:48:18 GMT",
            "Tue, 17 Dec 2019 23:48:18 GMT"
        ])
    );
    assert_eq!(body["observed"]["encoded"], json!(["a", "b", "c,d"]));
    assert_eq!(body["observed"]["numbers"], json!([1, 2, 3]));
}

#[tokio::test]
async fn malformed_later_header_fields_and_body_constraints_are_rejected_before_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(calls.clone());
    for (header, value) in [
        ("x-numbers", "not-a-number"),
        ("x-dates", "not-a-date"),
        ("x-encoded", "not-base64!"),
    ] {
        let request = Request::post("/bindings")
            .header("x-required", "header")
            .header(
                header,
                match header {
                    "x-numbers" => "1",
                    "x-dates" => "Mon, 16 Dec 2019 23:48:18 GMT",
                    _ => "YQ==",
                },
            )
            .header(header, value)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"bodyDisplay":"body"}"#))
            .unwrap();
        let (status, body) = answer(&service, request).await;
        assert_eq!(status, 400, "{header}: {body}");
        assert!(body["message"].as_str().unwrap().contains(header), "{body}");
    }
    let mut request = Request::post("/bindings")
        .header("x-required", "header")
        .header("x-strings", "first")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"bodyDisplay":"body"}"#))
        .unwrap();
    request
        .headers_mut()
        .append("x-strings", HeaderValue::from_bytes(&[0x80]).unwrap());
    assert_eq!(answer(&service, request).await.0, 400);
    assert_eq!(
        answer(
            &service,
            Request::post("/bindings")
                .header("x-required", "header")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"bodyDisplay":""}"#))
                .unwrap()
        )
        .await
        .0,
        400
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

static EARLY: OperationInfo = operation(
    "ZEarly",
    &[
        Segment::Literal("abc"),
        Segment::Literal("bcd"),
        Segment::Label("x"),
    ],
    &[],
);
static LATE: OperationInfo = operation(
    "ALate",
    &[
        Segment::Label("x"),
        Segment::Literal("bcd"),
        Segment::Literal("cde"),
    ],
    &[("mode", Some("on"))],
);
static REGULAR: OperationInfo = operation(
    "Regular",
    &[
        Segment::Literal("greedy"),
        Segment::Label("x"),
        Segment::Label("y"),
    ],
    &[],
);
static GREEDY: OperationInfo = operation(
    "Greedy",
    &[
        Segment::Literal("greedy"),
        Segment::GreedyLabel("x"),
        Segment::Literal("tail"),
    ],
    &[],
);
static FLAG: OperationInfo = operation("AFlag", &[Segment::Literal("flag")], &[("mode", None)]);
static EXACT: OperationInfo = operation(
    "ZExact",
    &[Segment::Literal("flag")],
    &[("mode", Some("on"))],
);
macro_rules! route_operation {
    ($type:ident, $info:ident) => {
        struct $type;
        impl Operation for $type {
            const INFO: &'static OperationInfo = &$info;
            type Input = Unit;
            type Output = Unit;
        }
    };
}
route_operation!(Early, EARLY);
route_operation!(Late, LATE);
route_operation!(Regular, REGULAR);
route_operation!(Greedy, GREEDY);
route_operation!(Flag, FLAG);
route_operation!(Exact, EXACT);
static ROUTES: ServiceInfo = generated::service_info(
    "audit#Routes",
    "1",
    None,
    None,
    &[&EARLY, &LATE, &REGULAR, &GREEDY, &FLAG, &EXACT],
    &[],
    |_| None,
    None,
    None,
    None,
);

fn route_operations() -> Operations {
    Operations::builder(&ROUTES)
        .add::<Early, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Late, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Regular, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Greedy, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Flag, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Exact, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .build()
        .unwrap()
}

#[test]
fn routing_compares_path_segments_before_query_and_greedy_labels() {
    let operations = route_operations();
    for (uri, expected, labels) in [
        (
            "/abc/bcd/cde?mode=on",
            "ZEarly",
            vec![("x", "cde".to_owned())],
        ),
        (
            "/other/bcd/cde?mode=on",
            "ALate",
            vec![("x", "other".to_owned())],
        ),
        (
            "/greedy/a/tail",
            "Regular",
            vec![("x", "a".to_owned()), ("y", "tail".to_owned())],
        ),
        ("/greedy/a/b/tail", "Greedy", vec![("x", "a/b".to_owned())]),
        ("/flag?mode=on", "ZExact", vec![]),
        ("/flag?mode=other", "AFlag", vec![]),
    ] {
        let (parts, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&parts, &operations).unwrap();
        assert_eq!(route.operation, expected, "{uri}");
        assert_eq!(route.labels, labels, "{uri}");
    }
}

#[test]
fn routing_normalizes_trailing_slashes_and_encoded_literals() {
    let operations = route_operations();
    for uri in ["/abc/bcd/value/", "/%61bc/bcd/value", "/abc/bcd/value///"] {
        let (parts, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&parts, &operations).expect(uri);
        assert_eq!(route.operation, "ZEarly");
        assert_eq!(route.labels, vec![("x", "value".to_owned())]);
    }
    let (parts, _) = Request::post("/greedy/a/b/tail/")
        .body(())
        .unwrap()
        .into_parts();
    assert_eq!(
        JsonProtocol.route(&parts, &operations).unwrap().labels,
        vec![("x", "a/b".to_owned())]
    );
    for uri in ["/abc//value", "/%2Fabc/bcd/value", "/%zzbc/bcd/value"] {
        let (parts, _) = Request::post(uri).body(()).unwrap().into_parts();
        assert!(JsonProtocol.route(&parts, &operations).is_none(), "{uri}");
    }
}

#[test]
fn explicit_head_routes_take_precedence_over_get_fallback() {
    static GET: OperationInfo = {
        let base = &(operation("Get", &[Segment::Literal("head")], &[]));
        generated::operation_info(
            base.id,
            base.name,
            base.resource,
            "GET",
            base.path,
            base.literal_query,
            base.status,
            base.input,
            base.output,
            base.errors,
            base.readonly,
            base.idempotent,
            base.internal,
            base.idempotency_token,
            base.traits,
        )
    };
    static HEAD: OperationInfo = {
        let base = &(operation("Head", &[Segment::Literal("head")], &[]));
        generated::operation_info(
            base.id,
            base.name,
            base.resource,
            "HEAD",
            base.path,
            base.literal_query,
            base.status,
            base.input,
            base.output,
            base.errors,
            base.readonly,
            base.idempotent,
            base.internal,
            base.idempotency_token,
            base.traits,
        )
    };
    static SERVICE: ServiceInfo = {
        let base = &(ROUTES);
        generated::service_info(
            base.id,
            base.version,
            None,
            None,
            &[&GET, &HEAD],
            base.errors,
            base.classify,
            base.reject,
            base.internal,
            base.unknown,
        )
    };
    route_operation!(Get, GET);
    route_operation!(Head, HEAD);
    let operations = Operations::builder(&SERVICE)
        .add::<Get, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Head, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .build()
        .unwrap();
    let (parts, _) = Request::head("/head").body(()).unwrap().into_parts();
    assert_eq!(
        JsonProtocol.route(&parts, &operations).unwrap().operation,
        "Head"
    );
    assert_eq!(
        JsonProtocol
            .route_scoped(&parts, &operations, &|op| op.name == "Get")
            .unwrap()
            .operation,
        "Get"
    );
}

#[tokio::test]
async fn known_targets_return_405_with_allow_and_head_uses_get_without_a_body() {
    {
        let operations = weather_service::weather::operations(Arc::new(
            weather_service::Weather::new(Arc::new(weather_service::MemoryCities::default())),
        ));
        let builder = Crucible::builder(operations);
        let service = builder.protocol(JsonProtocol).build().unwrap();
        let response = service
            .handle(Request::delete("/cities/").body(Body::empty()).unwrap())
            .await;
        assert_eq!(response.status(), 405);
        assert_eq!(response.headers()["allow"], "GET, HEAD, POST");
        for uri in ["/cities", "/%63ities/"] {
            let get = service
                .handle(Request::get(uri).body(Body::empty()).unwrap())
                .await;
            let head = service
                .handle(Request::head(uri).body(Body::empty()).unwrap())
                .await;
            assert_eq!(get.status(), 200);
            assert_eq!(head.status(), get.status());
            assert_eq!(
                head.headers()["content-type"],
                get.headers()["content-type"]
            );
            let bytes = to_bytes(get.into_body(), usize::MAX).await.unwrap();
            assert!(!bytes.is_empty());
            assert_eq!(head.headers()["content-length"], bytes.len().to_string());
            assert!(
                to_bytes(head.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        let response = service
            .handle(
                Request::delete("/no-such-path")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 400);
        assert!(!response.headers().contains_key("allow"));
        let response = service
            .handle(Request::head("/no-such-path").body(Body::empty()).unwrap())
            .await;
        assert!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn json_bodies_require_a_compatible_content_type_before_invocation() {
    {
        let operations = weather_service::weather::operations(Arc::new(
            weather_service::Weather::new(Arc::new(weather_service::MemoryCities::default())),
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let builder = Crucible::builder(operations).interceptor(CountInvocations(calls.clone()));
        let service = builder.protocol(JsonProtocol).build().unwrap();
        for content_type in [None, Some("text/plain"), Some("application/hal+json")] {
            let mut request = Request::post("/cities");
            if let Some(value) = content_type {
                request = request.header("content-type", value);
            }
            let response = service
                .handle(request.body(Body::from(r#"{"name":"Oslo"}"#)).unwrap())
                .await;
            assert_eq!(response.status(), 415, "{content_type:?}");
        }
        let response = service
            .handle(
                Request::post("/cities")
                    .header("content-type", "application/json")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"Oslo"}"#))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 415, "duplicate content-type");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for (index, content_type) in [
            "application/json",
            "Application/JSON; charset=utf-8",
            " APPLICATION/JSON ; charset=\"UTF-8\"; custom=parameter",
        ]
        .into_iter()
        .enumerate()
        {
            let response = service
                .handle(
                    Request::post("/cities")
                        .header("content-type", content_type)
                        .body(Body::from(format!(r#"{{"name":"Oslo{index}"}}"#)))
                        .unwrap(),
                )
                .await;
            assert_eq!(response.status(), 201, "{content_type}");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let response = service
            .handle(
                Request::get("/cities")
                    .header("content-type", "Application/JSON; charset=utf-8")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 200, "HTTP-bound input");
        for content_type in [None, Some("application/json"), Some("text/plain")] {
            let mut request = Request::get("/cities");
            if let Some(value) = content_type {
                request = request.header("content-type", value);
            }
            let response = service.handle(request.body(Body::empty()).unwrap()).await;
            assert_eq!(response.status(), 200, "empty body, {content_type:?}");
        }
    }
}

struct CountInvocations(Arc<AtomicUsize>);

impl crucible_server_runtime_rust::Interceptor for CountInvocations {
    fn before<'a>(
        &'a self,
        _: &'a mut crucible_server_runtime_rust::DecodedJob<'_>,
    ) -> crucible_server_runtime_rust::Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        crucible_server_runtime_rust::Hook::Ready(Ok(()))
    }
}

#[tokio::test]
async fn malformed_uri_components_claim_their_route_and_reject_before_calling_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let service = Crucible::builder(route_operations())
        .on_rejection(|rejection| {
            assert!(rejection.is_malformed());
            Box::new(Invalid(rejection.summary()))
        })
        .operation_middleware(before_fn(|job| {
            let operation = job.operation().name;
            job.response_headers_mut()
                .insert("x-selected-operation", HeaderValue::from_static(operation));
            Ok(())
        }))
        .protocol(JsonProtocol)
        .interceptor(CountInvocations(observed))
        .build()
        .unwrap();
    for (uri, operation, field) in [
        ("/abc/bcd/%FF", "ZEarly", "x"),
        ("/abc/bcd/%", "ZEarly", "x"),
        ("/greedy/a/%FE/tail", "Greedy", "x"),
        ("/greedy/a/%GG/tail", "Greedy", "x"),
        ("/flag?mode=%FF", "ZExact", "mode"),
        ("/flag?%GG=on", "ZExact", "query"),
        // An undeclared key is the client's text: not named.
        ("/flag?mode=on&ignored=%FE", "ZExact", "query"),
    ] {
        let response = service
            .handle(
                Request::post(uri)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 400, "{uri}");
        assert_eq!(
            response.headers()["x-selected-operation"],
            operation,
            "{uri}"
        );
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert!(
            body["message"].as_str().unwrap().starts_with(field),
            "{body}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_selected_route(&service, "/abc/bcd/%EF%BF%BD", "ZEarly").await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let pending = futures_util::stream::pending::<Result<axum::body::Bytes, std::io::Error>>();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        service.handle(
            Request::post("/abc/bcd/%FF")
                .body(Body::from_stream(pending))
                .unwrap(),
        ),
    )
    .await
    .expect("malformed routing must not wait for request body bytes");
    assert_eq!(response.status(), 400);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_modeled_queries_are_rejected_and_valid_unicode_is_preserved() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(calls.clone());
    for uri in [
        "/bindings?q=%FF",
        "/bindings?q=%FE",
        "/bindings?q=%",
        "/bindings?%FF=value",
    ] {
        let (status, body) = answer(
            &service,
            Request::post(uri)
                .header("x-required", "header")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"bodyDisplay":"body"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, 400, "{uri}: {body}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (status, body) = answer(
        &service,
        Request::post("/bindings?q=caf%C3%A9+%EF%BF%BD%2B")
            .header("x-required", "header")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"bodyDisplay":"body"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["observed"]["queryDisplay"], "café �+");
}

#[tokio::test]
async fn malformed_queries_preserve_valid_flags_path_specificity_and_protocol_scopes() {
    let operations = route_operations();
    for (uri, expected) in [
        ("/flag?mode=off&ignored=%FE", "AFlag"),
        ("/flag?mode=off&%GG=x", "AFlag"),
        ("/flag?mode=%FF", "ZExact"),
        ("/flag?%GG=on", "ZExact"),
        ("/abc/bcd/cde?mode=on&ignored=%FF", "ZEarly"),
        ("/greedy/a/tail?ignored=%FF", "Regular"),
    ] {
        let (parts, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&parts, &operations).unwrap();
        assert_eq!(route.operation, expected, "{uri}");
        assert!(route.rejection.unwrap().is_malformed());
    }
    let service = Crucible::builder(operations)
        .on_rejection(|rejection| Box::new(Invalid(rejection.summary())))
        .operation_middleware(before_fn(|job| {
            let operation = job.operation().name;
            job.response_headers_mut()
                .insert("x-selected-operation", HeaderValue::from_static(operation));
            Ok(())
        }))
        .protocol(JsonProtocol.only(&["ALate", "AFlag"]))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    for (uri, expected) in [
        ("/abc/bcd/cde?mode=on&ignored=%FF", "ALate"),
        ("/flag?mode=off&ignored=%FE", "AFlag"),
        ("/abc/bcd/other?ignored=%FF", "ZEarly"),
    ] {
        let response = service
            .handle(
                Request::post(uri)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 400, "{uri}");
        assert_eq!(
            response.headers()["x-selected-operation"],
            expected,
            "{uri}"
        );
    }
}

async fn assert_selected_route(service: &Crucible, uri: &str, expected: &str) {
    let response = service
        .handle(
            Request::post(uri)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 200, "{uri}");
    assert_eq!(
        response.headers()["x-selected-operation"],
        expected,
        "{uri}"
    );
}

#[test]
fn malformed_literal_fallback_keeps_path_specificity_ahead_of_query_preference() {
    static SPECIFIC: OperationInfo = operation(
        "Specific",
        &[Segment::Literal("specific"), Segment::Literal("item")],
        &[("mode", Some("on"))],
    );
    static GENERIC: OperationInfo = operation(
        "Generic",
        &[Segment::Label("root"), Segment::Label("item")],
        &[],
    );
    static UNCONSTRAINED_GREEDY: OperationInfo =
        operation("UnconstrainedGreedy", &[Segment::GreedyLabel("path")], &[]);
    static SERVICE: ServiceInfo = {
        let base = &(ROUTES);
        generated::service_info(
            base.id,
            base.version,
            None,
            None,
            &[&SPECIFIC, &GENERIC, &UNCONSTRAINED_GREEDY],
            base.errors,
            base.classify,
            base.reject,
            base.internal,
            base.unknown,
        )
    };
    route_operation!(Specific, SPECIFIC);
    route_operation!(Generic, GENERIC);
    route_operation!(UnconstrainedGreedy, UNCONSTRAINED_GREEDY);
    let operations = Operations::builder(&SERVICE)
        .add::<Specific, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Generic, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<UnconstrainedGreedy, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .build()
        .unwrap();
    for (uri, expected) in [
        ("/specific/item?mode=%FF", "Specific"),
        ("/specific/item?%GG=on", "Specific"),
        ("/specific/item?mode=on&ignored=%FF", "Specific"),
        ("/specific/item?mode=wrong&ignored=%FF", "Generic"),
    ] {
        let (parts, _) = Request::post(uri).body(()).unwrap().into_parts();
        let route = JsonProtocol.route(&parts, &operations).unwrap();
        assert_eq!(route.operation, expected, "{uri}");
        assert!(route.rejection.unwrap().is_malformed());
    }
}

#[tokio::test]
async fn scoped_json_routes_choose_allowed_overlapping_path_and_query_candidates() {
    let service = Crucible::builder(route_operations())
        .on_rejection(|rejection| Box::new(Invalid(rejection.summary())))
        .operation_middleware(before_fn(|job| {
            let name = job.operation().name;
            job.response_headers_mut()
                .insert("x-selected-operation", HeaderValue::from_static(name));
            Ok(())
        }))
        .protocol(JsonProtocol.only(&["ALate", "AFlag"]))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    // The unrestricted winners are ZEarly and ZExact. They must not hide
    // the less specific candidates accepted by the first protocol's scope.
    assert_selected_route(&service, "/abc/bcd/cde?mode=on", "ALate").await;
    assert_selected_route(&service, "/flag?mode=on", "AFlag").await;
    // A request with no candidate in that scope still reaches the fallback.
    assert_selected_route(&service, "/abc/bcd/other", "ZEarly").await;
}

#[tokio::test]
async fn nested_json_scopes_intersect_before_choosing_overlapping_candidates() {
    let service = Crucible::builder(route_operations())
        .on_rejection(|rejection| Box::new(Invalid(rejection.summary())))
        .operation_middleware(before_fn(|job| {
            let name = job.operation().name;
            job.response_headers_mut()
                .insert("x-selected-operation", HeaderValue::from_static(name));
            Ok(())
        }))
        .protocol(
            JsonProtocol
                .only(&["ALate", "AFlag", "ZExact"])
                .except(&["ZExact"])
                .when(|operation| operation.name != "ZEarly"),
        )
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    assert_selected_route(&service, "/abc/bcd/cde?mode=on", "ALate").await;
    assert_selected_route(&service, "/flag?mode=on", "AFlag").await;

    // The outer scope cannot widen the inner scope: their empty intersection
    // leaves both matching query candidates to the fallback protocol.
    let service = Crucible::builder(route_operations())
        .on_rejection(|rejection| Box::new(Invalid(rejection.summary())))
        .operation_middleware(before_fn(|job| {
            let name = job.operation().name;
            job.response_headers_mut()
                .insert("x-selected-operation", HeaderValue::from_static(name));
            Ok(())
        }))
        .protocol(JsonProtocol.only(&["AFlag"]).only(&["ZExact"]))
        .protocol(JsonProtocol)
        .build()
        .unwrap();
    assert_selected_route(&service, "/flag?mode=on", "ZExact").await;
}

#[tokio::test]
async fn invocation_extensions_always_use_the_invoke_protocol_even_on_an_http_route() {
    let service = service(Arc::new(AtomicUsize::new(0)));
    let mut request = Request::post("/bindings")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"headerDisplay":"direct","queryDisplay":"direct","bodyDisplay":"direct"}"#,
        ))
        .unwrap();
    request.extensions_mut().insert(Invocation {
        operation: "Bindings".to_owned(),
    });
    let (status, body) = answer(&service, request).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["output"]["observed"]["headerDisplay"], "direct");
    assert_eq!(body["output"]["observed"]["queryDisplay"], "direct");
    let mut request = Request::post("/bindings")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    request.extensions_mut().insert(Invocation {
        operation: "Missing".to_owned(),
    });
    let (status, body) = answer(&service, request).await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"]["__type"], "UnknownOperation");
}
