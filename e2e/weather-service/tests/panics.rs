// The fixture deprecates `Ping`; these tests still drive it.
#![allow(deprecated)]
use crucible_server_runtime_rust::__generated as generated;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use crucible_server_runtime_rust::middlewares::{
    LogRecord, LogSink, Metrics, Recorder, RequestRecord, ServiceLog,
};
use crucible_server_runtime_rust::operation::ServiceInfo;
use crucible_server_runtime_rust::protocol::{Answers, ProtocolMatch};
use crucible_server_runtime_rust::{
    Crucible, DecodedJob, Error, ExceptionTranslator, Hook, Interceptor, InvokeProtocol, Job,
    JsonProtocol, Middleware, Operation, OperationAwareMiddleware, OperationJob, Operations,
    Protocol, before_fn, host,
};
use serde_json::{Value, json};
use weather_service::weather;

const SECRET: &str = "panic secret must stay in logs";
static SERVICE: ServiceInfo = {
    let base = &(weather::SERVICE);
    generated::service_info(
        base.id,
        base.version,
        None,
        &[weather::Ping::INFO],
        base.errors,
        base.classify,
        base.reject,
        base.internal,
        base.unknown,
    )
};

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

#[derive(Clone, Default)]
struct Counts(Arc<Mutex<Vec<u16>>>);
impl Recorder for Counts {
    fn record(&self, record: &RequestRecord<'_>) {
        self.0.lock().unwrap().push(record.status);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Handler,
    Before,
    AsyncBefore,
    After,
    AsyncAfter,
    Complete,
}

struct PanicHook(Phase);
impl Middleware for PanicHook {
    fn before<'a>(&'a self, _: &'a mut Job) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        if self.0 == Phase::Before {
            panic!("{SECRET}");
        }
        Hook::wait(async move {
            if self.0 == Phase::AsyncBefore {
                tokio::task::yield_now().await;
                panic!("{SECRET}");
            }
            Ok(())
        })
    }
    fn after<'a>(&'a self, _: &'a mut Job, _: &'a mut Response) -> Hook<'a, ()> {
        if self.0 == Phase::After {
            panic!("{SECRET}");
        }
        Hook::wait(async move {
            if self.0 == Phase::AsyncAfter {
                tokio::task::yield_now().await;
                panic!("{SECRET}");
            }
        })
    }
    fn complete(&self, _: &Job, _: &Response) {
        if self.0 == Phase::Complete {
            panic!("{SECRET}");
        }
    }
}

struct PanicAdmission(Phase);
impl OperationAwareMiddleware for PanicAdmission {
    fn before<'a>(
        &'a self,
        _: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        if self.0 == Phase::Before {
            panic!("{SECRET}");
        }
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, _: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if self.0 == Phase::After {
            panic!("{SECRET}");
        }
        Hook::Ready(())
    }
    fn complete(&self, _: &OperationJob<'_>, _: &Response) {
        if self.0 == Phase::Complete {
            panic!("{SECRET}");
        }
    }
}

struct PanicInterceptor(Phase);
impl Interceptor for PanicInterceptor {
    fn before<'a>(
        &'a self,
        _: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        if self.0 == Phase::Before {
            panic!("{SECRET}");
        }
        Hook::Ready(Ok(()))
    }
    fn after<'a>(&'a self, _: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if self.0 == Phase::After {
            panic!("{SECRET}");
        }
        Hook::Ready(())
    }
    fn complete(&self, _: &OperationJob<'_>, _: &Response) {
        if self.0 == Phase::Complete {
            panic!("{SECRET}");
        }
    }
}

fn builder(
    phase: Phase,
    records: &Records,
    counts: &Counts,
) -> crucible_server_runtime_rust::Builder {
    let operations = Operations::builder(&SERVICE)
        .add::<weather::Ping, _, _>(Arc::new(phase), |phase, _| {
            Box::pin(async move {
                tokio::task::yield_now().await;
                if *phase == Phase::Handler {
                    panic!("{SECRET}");
                }
                Ok(())
            })
        })
        .build()
        .unwrap();
    Crucible::builder(operations)
        .defaults()
        .middleware(ServiceLog::errors_only().sink(records.clone()))
        .middleware(Metrics::new(counts.clone()))
}

fn assert_observers(records: &Records, counts: &Counts, expected: usize) {
    let records = records.0.lock().unwrap();
    assert_eq!(records.len(), expected, "{records:?}");
    for record in records.iter() {
        assert_eq!(record["error"], "InternalFailure");
        assert_eq!(record["status"], 500);
        assert!(record["cause"].as_str().unwrap().contains(SECRET));
    }
    assert_eq!(*counts.0.lock().unwrap(), vec![500; expected]);
}

fn event() -> Value {
    json!({
        "version": "2.0", "rawPath": "/ping", "rawQueryString": "", "headers": {},
        "requestContext": {
            "http": { "method": "GET", "path": "/ping", "protocol": "HTTP/1.1" },
            "stage": "$default", "requestId": "panic-request"
        },
        "isBase64Encoded": false
    })
}

#[tokio::test]
async fn panicking_handler_returns_internal_failure_over_tcp_and_logs() {
    let records = Records::default();
    let counts = Counts::default();
    let service = builder(Phase::Handler, &records, &counts).build().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(host::http_on(service, listener));
    let answer = tokio::task::spawn_blocking(move || {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\nGET /ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    })
    .await
    .unwrap();
    server.abort();
    assert!(answer.starts_with("HTTP/1.1 500"), "{answer}");
    assert_eq!(answer.matches("HTTP/1.1 500").count(), 2, "{answer}");
    assert!(answer.contains("InternalFailure"), "{answer}");
    assert!(answer.contains("x-request-id:"), "{answer}");
    assert!(!answer.contains(SECRET), "{answer}");
    assert_observers(&records, &counts, 2);
}

#[tokio::test]
async fn panicking_handler_returns_internal_failure_for_lambda_http_and_direct_invokes() {
    for direct in [false, true] {
        let records = Records::default();
        let counts = Counts::default();
        let service = builder(Phase::Handler, &records, &counts).build().unwrap();
        let answer = host::lambda_event(
            &service,
            if direct {
                json!({"operation": "Ping", "input": {}})
            } else {
                event()
            },
        )
        .await
        .unwrap();
        if direct {
            assert_eq!(answer["error"]["__type"], "InternalFailure");
        } else {
            assert_eq!(answer["statusCode"], 500);
            assert_eq!(answer["headers"]["x-request-id"], "panic-request");
            assert!(answer["body"].as_str().unwrap().contains("InternalFailure"));
        }
        assert!(!answer.to_string().contains(SECRET));
        assert_observers(&records, &counts, 1);
    }
}

#[tokio::test]
async fn panicking_extensions_return_internal_failure_and_complete_remaining_observers() {
    for phase in [
        Phase::Before,
        Phase::AsyncBefore,
        Phase::After,
        Phase::AsyncAfter,
        Phase::Complete,
    ] {
        for family in ["transport", "operation", "interceptor"] {
            if family != "transport" && matches!(phase, Phase::AsyncBefore | Phase::AsyncAfter) {
                continue;
            }
            for lambda in [false, true] {
                let records = Records::default();
                let counts = Counts::default();
                let builder = builder(phase, &records, &counts);
                let service = match family {
                    "transport" => builder.middleware(PanicHook(phase)),
                    "operation" => builder.operation_middleware(PanicAdmission(phase)),
                    _ => builder.interceptor(PanicInterceptor(phase)),
                }
                .build()
                .unwrap();
                if lambda {
                    let answer = host::lambda_event(&service, event()).await.unwrap();
                    assert_eq!(answer["statusCode"], 500, "{family} {phase:?}: {answer}");
                    assert!(!answer.to_string().contains(SECRET));
                } else {
                    let response = service
                        .handle(Request::get("/ping").body(Body::empty()).unwrap())
                        .await;
                    assert_eq!(response.status(), 500, "{family} {phase:?}");
                    assert!(response.headers().contains_key("x-request-id"));
                    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
                }
                assert_observers(&records, &counts, 1);
            }
        }
    }
}

struct PanicWriter;
impl Protocol for PanicWriter {
    fn identify<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<ProtocolMatch>> {
        JsonProtocol.identify(job, operations)
    }
    fn decode<'a>(
        &'a self,
        job: &'a mut Job,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        JsonProtocol.decode(job)
    }
    fn write(&self, _: &Job, _: &Answers<'_>) -> Response {
        panic!("{SECRET}");
    }
}

#[tokio::test]
async fn panicking_error_writer_falls_back_to_internal_failure_and_logs() {
    let records = Records::default();
    let counts = Counts::default();
    let operations = Operations::builder(&SERVICE)
        .add::<weather::Ping, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(()) }))
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .middleware(ServiceLog::errors_only().sink(records.clone()))
        .middleware(Metrics::new(counts.clone()))
        .protocol(PanicWriter)
        .protocol(InvokeProtocol)
        .build()
        .unwrap();
    let answer = host::lambda_event(&service, event()).await.unwrap();
    assert_eq!(answer["statusCode"], 500);
    assert!(answer["body"].as_str().unwrap().contains("InternalFailure"));
    assert!(!answer.to_string().contains(SECRET));
    assert_observers(&records, &counts, 1);
}

#[tokio::test]
async fn panicking_exception_rule_returns_internal_failure_and_logs() {
    let records = Records::default();
    let counts = Counts::default();
    let service = builder(Phase::Before, &records, &counts)
        .operation_middleware(before_fn(|_| Err(Error::internal("trigger translation"))))
        .exception_translator(ExceptionTranslator::new().translate_with(|_| panic!("{SECRET}")))
        .build()
        .unwrap();
    let answer = host::lambda_event(&service, event()).await.unwrap();
    assert_eq!(answer["statusCode"], 500);
    assert!(!answer.to_string().contains(SECRET));
    assert_observers(&records, &counts, 1);
}
