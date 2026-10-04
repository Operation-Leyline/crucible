//! The framework, in process: direct invokes, the handler chain and its
//! extension points, error translation, the service log. HTTP over a socket
//! is tools/test-e2e.sh's job.

use std::sync::{Arc, Mutex};

use crucible::handlers::{
    ExceptionTranslator, Fields, LogPolicy, LogSink, OperationHandler, ServiceLog, before_fn,
    for_operation,
};
use crucible::{Context, Crucible, HandlerExt, InvokeProtocol, JsonProtocol};
use serde_json::{Value, json};
use weather_service::api::{self, model};
use weather_service::{MemoryCities, Weather};

fn memory() -> Arc<MemoryCities> {
    Arc::new(MemoryCities::default())
}

fn service() -> Crucible {
    weather_service::service(memory())
}

async fn get(service: &Crucible, uri: &str) -> (u16, Value) {
    let request = axum::http::Request::get(uri)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn direct_invokes_round_trip_through_the_chain() {
    let service = service();
    let created = service
        .invoke("CreateCity", json!({ "name": "Oslo" }))
        .await;
    let id = created["output"]["city"]["id"].as_str().unwrap().to_owned();
    let read = service.invoke("GetCity", json!({ "cityId": id })).await;
    assert_eq!(read["output"]["city"]["name"], "Oslo");

    let missing = service
        .invoke("GetCity", json!({ "cityId": "c-99999999" }))
        .await;
    assert_eq!(missing["error"]["__type"], "NotFoundError");
    assert_eq!(missing["error"]["resourceId"], "c-99999999");

    let invalid = service.invoke("GetCity", json!({ "cityId": "nope" })).await;
    assert_eq!(invalid["error"]["__type"], "ValidationError");
    assert_eq!(invalid["error"]["fieldList"][0]["path"], "cityId");

    let unknown = service.invoke("Teleport", json!({})).await;
    assert_eq!(unknown["error"]["__type"], "UnknownOperation");
}

#[tokio::test]
async fn an_unknown_route_is_a_404_in_the_protocol_format() {
    let (status, body) = get(&service(), "/nowhere").await;
    assert_eq!(status, 404);
    assert_eq!(body["__type"], "UnknownOperation");
}

#[derive(Debug)]
struct Database(&'static str);

impl std::fmt::Display for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "database said: {}", self.0)
    }
}

impl std::error::Error for Database {}

fn failing(translator: ExceptionTranslator) -> Crucible {
    Crucible::builder(api::operations(Arc::new(Weather::new(memory()))))
        .handler(translator)
        .handler(before_fn(|_| Err(Database("password=hunter2").into())).only(&["Ping"]))
        .handler(JsonProtocol)
        .handler(InvokeProtocol)
        .build()
        .unwrap()
}

#[tokio::test]
async fn an_internal_failure_never_reaches_the_caller() {
    let (status, body) = get(&failing(ExceptionTranslator::new()), "/ping").await;
    assert_eq!(status, 500);
    assert_eq!(body["__type"], "InternalFailure");
    assert!(!body.to_string().contains("hunter2"), "{body}");
}

#[tokio::test]
async fn the_exception_translator_maps_failures_to_modelled_errors() {
    let translator = ExceptionTranslator::new().translate(|error: &Database| {
        api::errors::ThrottlingError {
            message: format!("busy ({} chars hidden)", error.0.len()),
            retry_after: Some(1),
        }
        .into()
    });
    let (status, body) = get(&failing(translator), "/ping").await;
    assert_eq!(status, 429);
    assert_eq!(body["__type"], "ThrottlingError");
}

#[tokio::test]
async fn scoped_handlers_run_only_where_they_apply() {
    let service = failing(ExceptionTranslator::new());
    let (status, _) = get(&service, "/status").await;
    assert_eq!(status, 200, "the failing handler is scoped to Ping");
}

#[derive(Clone, Default)]
struct Records(Arc<Mutex<Vec<Value>>>);

impl LogSink for Records {
    fn write(&self, record: &Value) {
        self.0.lock().unwrap().push(record.clone());
    }
}

#[tokio::test]
async fn the_service_log_writes_failures_by_default_and_more_where_asked() {
    let records = Records::default();
    let log = ServiceLog::errors_only()
        .operation::<api::CreateCity>(LogPolicy::everything().output(Fields::Only(vec!["city"])))
        .sink(records.clone());
    let service = Crucible::builder(api::operations(Arc::new(Weather::new(memory()))))
        .handler(log)
        .handler(JsonProtocol)
        .handler(InvokeProtocol)
        .build()
        .unwrap();

    service.invoke("Ping", json!({})).await;
    service
        .invoke("GetCity", json!({ "cityId": "c-00000042" }))
        .await;
    service
        .invoke("CreateCity", json!({ "name": "Bergen" }))
        .await;

    let records = records.0.lock().unwrap();
    assert_eq!(
        records.len(),
        2,
        "a successful Ping is not logged: {records:?}"
    );
    assert_eq!(records[0]["operation"], "GetCity");
    assert_eq!(records[0]["outcome"], "error");
    assert_eq!(records[0]["error"], "NotFoundError");
    assert_eq!(records[1]["operation"], "CreateCity");
    assert_eq!(records[1]["input"]["name"], "Bergen");
    assert_eq!(records[1]["output"]["city"]["name"], "Bergen");
}

struct Audit(Arc<Mutex<Vec<String>>>);

#[crucible::async_trait]
impl OperationHandler<api::GetCity> for Audit {
    async fn before(&self, input: &model::GetCityInput, context: &Context) -> crucible::Result<()> {
        assert!(!context.request_id().is_empty());
        self.0.lock().unwrap().push(input.city_id.clone());
        Ok(())
    }
}

#[tokio::test]
async fn typed_operation_handlers_see_their_input() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let service = Crucible::builder(api::operations(Arc::new(Weather::new(memory()))))
        .handler(JsonProtocol)
        .handler(InvokeProtocol)
        .handler(for_operation(Audit(seen.clone())))
        .build()
        .unwrap();
    service
        .invoke("GetCity", json!({ "cityId": "c-00000007" }))
        .await;
    service.invoke("Ping", json!({})).await;
    assert_eq!(*seen.lock().unwrap(), ["c-00000007"]);
}

#[tokio::test]
async fn a_service_split_across_types_names_what_it_left_out() {
    let weather = Arc::new(Weather::new(memory()));
    let missing = api::builder()
        .ping(weather.clone())
        .get_city(weather)
        .build()
        .unwrap_err();
    assert!(missing.0.contains(&"CreateCity"), "{missing}");
}

#[tokio::test]
async fn lambda_answers_direct_invokes_and_http_events_alike() {
    let service = service();
    let invoked = crucible::host::lambda_event(
        &service,
        json!({ "operation": "CreateCity", "input": { "name": "Lima" } }),
    )
    .await
    .unwrap();
    let id = invoked["output"]["city"]["id"].as_str().unwrap().to_owned();

    // An API Gateway HTTP API (payload 2.0) event, as a Function URL sends too.
    let event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": format!("/cities/{id}"),
        "rawQueryString": "",
        "headers": { "accept": "application/json" },
        "requestContext": {
            "accountId": "123456789012",
            "apiId": "api",
            "domainName": "example.test",
            "domainPrefix": "example",
            "http": { "method": "GET", "path": format!("/cities/{id}"), "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1", "userAgent": "test" },
            "requestId": "id",
            "routeKey": "$default",
            "stage": "$default",
            "time": "04/Oct/2026:12:00:00 +0000",
            "timeEpoch": 1790000000000_i64
        },
        "isBase64Encoded": false,
        // Unrelated to crucible: only a string `operation` is a direct invoke.
        "operation": 42
    });
    let answered = crucible::host::lambda_event(&service, event).await.unwrap();
    assert_eq!(answered["statusCode"], 200, "{answered}");
    let body: Value = serde_json::from_str(answered["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["city"]["name"], "Lima");
    assert!(
        answered["headers"]["x-request-id"].is_string(),
        "{answered}"
    );
}

#[tokio::test]
async fn a_rejected_request_carries_its_request_id_in_the_body_too() {
    let request = axum::http::Request::get("/cities/nope")
        .body(axum::body::Body::empty())
        .unwrap();
    let response = service().handle(request).await;
    assert_eq!(response.status(), 400);
    let header = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["requestId"], header.as_str(), "{body}");
}
