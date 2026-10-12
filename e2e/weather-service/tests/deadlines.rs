//! A request that runs past its deadline is stopped and answered, where the
//! deadline is the service's `request_timeout` after arrival or a host's (a
//! Lambda invocation's) less `DEADLINE_RESERVE`. Otherwise whatever waits in
//! front (CloudFront, a gateway) gives up first: the caller sees its timeout,
//! and the service keeps running for nobody and logs nothing about it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderValue, Request};
use crucible_server_runtime_rust::middlewares::{LogRecord, LogSink, ServiceLog};
use crucible_server_runtime_rust::{
    AsyncOperationAwareMiddleware, Crucible, DEADLINE_RESERVE, DeadlineExceeded,
    ExceptionTranslator, OperationJob, RequestMeta, Result, raw_before_fn,
};
use serde_json::{Value, json};
use weather_service::MemoryCities;
use weather_service::weather::ThrottlingError;

/// Hangs any request that carries `x-stall`, as a slow dependency would.
struct Stall;

#[crucible_server_runtime_rust::async_trait]
impl AsyncOperationAwareMiddleware for Stall {
    async fn before(&self, job: &mut OperationJob<'_>) -> Result<()> {
        if job.context().headers().contains_key("x-stall") {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        Ok(())
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

fn builder(records: &Records) -> crucible_server_runtime_rust::Builder {
    weather_service::builder(Arc::new(MemoryCities::default()))
        .middleware(ServiceLog::errors_only().sink(records.clone()))
        .operation_middleware(Stall)
}

fn ping(stall: bool) -> Request<Body> {
    let request = Request::get("/ping");
    let request = if stall {
        request.header("x-stall", "1")
    } else {
        request
    };
    request.body(Body::empty()).unwrap()
}

async fn answer(service: &Crucible, request: Request<Body>) -> (u16, Value) {
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_request_past_the_services_budget_is_stopped_answered_and_logged() {
    let records = Records::default();
    let service = builder(&records)
        .request_timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let started = Instant::now();
    let (status, body) = answer(&service, ping(true)).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "stopped, not waited out"
    );
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["__type"], "InternalFailure", "{body}");
    assert!(
        !body.to_string().contains("deadline"),
        "the cause is for the log, not the caller: {body}"
    );

    {
        let records = records.0.lock().unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0]["operation"], "Ping", "{records:?}");
        let cause = records[0]["cause"].as_str().unwrap();
        assert!(cause.contains("ran past its deadline"), "{cause}");
    }

    // A request inside its budget is untouched.
    assert_eq!(answer(&service, ping(false)).await.0, 200);
}

#[tokio::test]
async fn an_implementation_sees_the_deadline_the_framework_enforces() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let service = builder(&Records::default())
        .request_timeout(Duration::from_secs(10))
        .middleware(raw_before_fn(move |job| {
            let context = job.context();
            record
                .lock()
                .unwrap()
                .push(context.deadline().map(|d| d - context.received()));
            Ok(())
        }))
        .build()
        .unwrap();
    assert_eq!(answer(&service, ping(false)).await.0, 200);

    // A host deadline nearer than the budget wins, less the reserve.
    let mut request = ping(false);
    let host = Instant::now() + Duration::from_secs(2);
    request.extensions_mut().insert(RequestMeta {
        request_id: None,
        deadline: Some(host),
    });
    assert_eq!(answer(&service, request).await.0, 200);

    let seen = seen.lock().unwrap();
    assert_eq!(seen[0], Some(Duration::from_secs(10)));
    let budget = seen[1].unwrap();
    assert!(
        budget <= Duration::from_secs(2) - DEADLINE_RESERVE,
        "{budget:?}"
    );
    assert!(budget > Duration::from_secs(1), "{budget:?}");
}

#[tokio::test]
async fn a_lambda_invocation_is_answered_before_the_platform_stops_it() {
    let records = Records::default();
    // No budget of its own: the invocation's deadline alone stops it.
    let service = builder(&records).build().unwrap();
    let event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": "/ping",
        "rawQueryString": "",
        "headers": { "x-stall": "1" },
        "requestContext": {
            "accountId": "123456789012",
            "apiId": "api",
            "domainName": "example.test",
            "domainPrefix": "example",
            "http": { "method": "GET", "path": "/ping", "protocol": "HTTP/1.1", "sourceIp": "192.0.2.1", "userAgent": "test" },
            "requestId": "event-1",
            "routeKey": "$default",
            "stage": "$default",
            "time": "04/Oct/2026:12:00:00 +0000",
            "timeEpoch": 1790000000000_i64
        },
        "isBase64Encoded": false
    })
    .to_string();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(1);
    let answer = crucible_server_runtime_rust::host::lambda_event_json_with(
        &service,
        &event,
        RequestMeta {
            request_id: Some(HeaderValue::from_static("invocation-1")),
            deadline: Some(deadline),
        },
    )
    .await
    .unwrap();
    assert!(
        Instant::now() < deadline,
        "answered before the platform kills it"
    );
    let answer: Value = serde_json::from_str(answer.get()).unwrap();
    assert_eq!(answer["statusCode"], 500, "{answer}");
    let records = records.0.lock().unwrap();
    assert!(
        records[0]["cause"]
            .as_str()
            .unwrap()
            .contains("ran past its deadline"),
        "{records:?}"
    );
}

#[tokio::test]
async fn an_exception_rule_can_answer_a_modelled_error_instead() {
    // A real service would model a 503; Weather's retryable error stands in.
    let translator = ExceptionTranslator::new().translate(|late: &DeadlineExceeded| {
        assert!(late.elapsed() >= Duration::from_millis(50));
        ThrottlingError {
            message: "busy; retry".to_owned(),
            retry_after: Some(1),
        }
        .into()
    });
    let service = builder(&Records::default())
        .exception_translator(translator)
        .request_timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let (status, body) = answer(&service, ping(true)).await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["__type"], "ThrottlingError", "{body}");
}

#[tokio::test]
async fn a_budget_too_long_to_represent_is_no_budget() {
    let service = builder(&Records::default())
        .request_timeout(Duration::MAX)
        .build()
        .unwrap();
    assert_eq!(answer(&service, ping(false)).await.0, 200);
}
