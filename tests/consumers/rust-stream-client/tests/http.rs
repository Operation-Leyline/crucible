use axum::{Router, body::Body, http::Response};
use futures_util::StreamExt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use streams_sdk::{
    CallOptions, CancellationToken, Config, Error, Progress, PublishInput, Retry, StreamsClient,
    Token,
};

const MIME: &str = "text/event-stream; charset=utf-8; crucible-stream=1";
const COMPLETE: &str = "event: crucible.complete\ndata: {}\n\n";
const ITEM: &str = "data: {\"ready\":\"first\"}\n\n";

async fn raw(
    wire: Vec<u8>,
    status: u16,
    content_type: &'static str,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let route = Router::new().fallback(move || {
        let wire = wire.clone();
        let count = requests.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(status)
                .header("content-type", content_type)
                .header("x-request-id", "profile-request")
                .header("x-reason", "from-opening-header")
                .body(Body::from(wire))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, route).await.unwrap();
    });
    (url, count, task)
}
fn client(endpoint: String) -> StreamsClient {
    StreamsClient::new(Config::new(endpoint).token(Token::fixed("token")))
}
async fn dropped(api: &streaming_consumer::Api) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while api.drops.load(Ordering::SeqCst) != 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("client lifetime released the producer before server cleanup");
}
async fn framework(
    mode: &str,
) -> (
    String,
    Arc<streaming_consumer::Api>,
    tokio::task::JoinHandle<()>,
) {
    let api = Arc::new(streaming_consumer::Api {
        mode: mode.into(),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let service = streaming_consumer::service(api.clone());
    let task = tokio::spawn(async move {
        crucible_server_runtime_rust::host::http_on(service, listener)
            .await
            .unwrap();
    });
    (url, api, task)
}

#[tokio::test]
async fn generated_get_post_unary_first_before_close_and_typed_late_error() {
    let (url, api, server) = framework("hold").await;
    let c = client(url);
    let mut events = c.watch().await.unwrap();
    assert_eq!(events.metadata().status, 200);
    assert!(events.metadata().request_id.is_some());
    assert!(
        matches!(tokio::time::timeout(Duration::from_secs(2),events.next()).await.unwrap().unwrap().unwrap(),Progress::Ready(ref value) if value=="first")
    );
    assert_eq!(api.drops.load(Ordering::SeqCst), 0);
    assert_eq!(c.read().await.unwrap().value, "unary");
    events.cancel();
    dropped(&api).await;
    server.abort();
    let (url, api, server) = framework("ok").await;
    let c = client(url);
    let mut events = c
        .publish(PublishInput::builder().topic("typed").build().unwrap())
        .await
        .unwrap();
    assert!(matches!(
        events.next().await.unwrap().unwrap(),
        Progress::Ready(_)
    ));
    let Error::Service(error) = events.next().await.unwrap().unwrap_err() else {
        panic!("typed late error")
    };
    assert!(
        matches!(error.error,streams_sdk::PublishError::WorkFailed(ref error) if error.message=="step failed")
    );
    assert_eq!(error.metadata.status, 503);
    assert!(error.metadata.request_id.is_some());
    assert!(events.next().await.is_none());
    dropped(&api).await;
    server.abort();
}

#[tokio::test]
async fn shared_profile_vectors_and_unknown_union_values() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../response-stream.json")).unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let wire = if let Some(text) = case["wire"].as_str() {
            text.as_bytes().to_vec()
        } else {
            let text = case["hex"].as_str().unwrap();
            (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                .collect()
        };
        let (url, count, server) = raw(wire, 200, MIME).await;
        let c = StreamsClient::new(
            Config::new(url)
                .token(Token::fixed("token"))
                .max_stream_event_bytes(case["limit"].as_u64().unwrap_or(1024 * 1024) as usize),
        );
        let mut events = c.watch().await.unwrap();
        let mut items = 0;
        let mut failed = false;
        while let Some(item) = events.next().await {
            match item {
                Ok(_) => items += 1,
                Err(error) => {
                    assert!(
                        matches!(error, Error::Decode(_)),
                        "{}: {error:?}",
                        case["name"]
                    );
                    failed = true;
                    break;
                }
            }
        }
        assert_eq!(
            !failed,
            case["valid"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(items, case["items"].as_u64().unwrap(), "{}", case["name"]);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "no replay {}",
            case["name"]
        );
        server.abort();
    }
    let (url, _, server) = raw(
        format!("data: {{\"future\":{{\"x\":\"y\"}}}}\n\n{COMPLETE}").into_bytes(),
        200,
        MIME,
    )
    .await;
    let mut events = client(url).watch().await.unwrap();
    assert!(matches!(
        events.next().await.unwrap().unwrap(),
        Progress::UnknownValue(_)
    ));
    assert!(events.next().await.is_none());
    server.abort();
}

#[tokio::test]
async fn late_errors_use_document_fields_for_body_headers_and_enum_payloads() {
    for (name, status, fields) in [
        ("WorkFailed", 503, r#"{"message":"failed"}"#),
        (
            "HeaderFailure",
            409,
            r#"{"message":"failed","reason":"broken"}"#,
        ),
        ("PayloadFailure", 422, r#"{"reason":"broken"}"#),
        ("FutureFailure", 500, r#"{"message":"future"}"#),
    ] {
        let text = format!(
            "event: crucible.error\ndata: {{\"type\":\"{name}\",\"status\":{status},\"error\":{fields}}}\n\n"
        );
        let (url, count, server) = raw(text.into_bytes(), 200, MIME).await;
        let mut events = client(url).watch().await.unwrap();
        let error = events.next().await.unwrap().unwrap_err();
        assert_eq!(error.metadata().unwrap().status, status);
        assert_eq!(
            error.metadata().unwrap().request_id.as_deref(),
            Some("profile-request")
        );
        let expected_page = match name {
            "FutureFailure" => None,
            "HeaderFailure" | "PayloadFailure" => Some(format!(
                "https://docs.example.test/errors/{name}/reason-broken"
            )),
            _ => Some(format!("https://docs.example.test/errors/{name}")),
        };
        assert_eq!(error.doc_url(), expected_page.as_deref());
        match (name, error) {
            ("WorkFailed", Error::Service(e)) => {
                assert!(matches!(e.error, streams_sdk::WatchError::WorkFailed(_)))
            }
            ("HeaderFailure", Error::Service(e)) => assert!(
                matches!(e.error,streams_sdk::WatchError::HeaderFailure(ref e) if e.reason.as_str()=="broken")
            ),
            ("PayloadFailure", Error::Service(e)) => assert!(
                matches!(e.error,streams_sdk::WatchError::PayloadFailure(ref e) if e.reason.as_str()=="broken")
            ),
            ("FutureFailure", Error::Unknown(e)) => {
                assert_eq!(e.type_name.as_deref(), Some("FutureFailure"))
            }
            (_, error) => panic!("{name}: {error:?}"),
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(events.next().await.is_none());
        server.abort();
    }
}

#[tokio::test]
async fn before_consumption_drop_cancel_token_and_deadline_release_server() {
    for mode in ["drop", "cancel", "token", "deadline"] {
        let (url, api, server) = framework("pending").await;
        let token = CancellationToken::new();
        let c = client(url);
        let options = CallOptions {
            timeout: (mode == "deadline").then_some(Duration::from_millis(300)),
            cancellation: Some(token.clone()),
        };
        let mut events = c.watch_with_options(options).await.unwrap();
        match mode {
            "drop" => drop(events),
            "cancel" => {
                events.cancel();
                dropped(&api).await;
                assert!(events.next().await.is_none());
            }
            "token" => {
                token.cancel();
                dropped(&api).await;
                assert!(matches!(events.next().await, Some(Err(Error::Cancelled))));
            }
            "deadline" => {
                dropped(&api).await;
                assert!(matches!(events.next().await, Some(Err(Error::Timeout))));
            }
            _ => unreachable!(),
        }
        dropped(&api).await;
        server.abort();
    }
}

#[tokio::test]
async fn cancellation_mid_read_and_between_items_releases_server_without_replay() {
    for deadline in [false, true] {
        let (url, api, server) = framework("hold").await;
        let token = CancellationToken::new();
        let c = client(url);
        let mut events = c
            .watch_with_options(CallOptions {
                timeout: deadline.then_some(Duration::from_millis(300)),
                cancellation: Some(token.clone()),
            })
            .await
            .unwrap();
        events.next().await.unwrap().unwrap();
        if deadline {
            assert!(matches!(events.next().await, Some(Err(Error::Timeout))));
        } else {
            let trigger = token.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                trigger.cancel();
            });
            assert!(matches!(events.next().await, Some(Err(Error::Cancelled))));
        }
        dropped(&api).await;
        assert!(events.next().await.is_none());
        server.abort();
    }
    let (url, _, server) = raw(format!("{ITEM}{ITEM}{COMPLETE}").into_bytes(), 200, MIME).await;
    let token = CancellationToken::new();
    let c = client(url);
    let mut events = c
        .watch_with_options(CallOptions {
            timeout: None,
            cancellation: Some(token.clone()),
        })
        .await
        .unwrap();
    events.next().await.unwrap().unwrap();
    token.cancel();
    assert!(matches!(events.next().await, Some(Err(Error::Cancelled))));
    server.abort();
}

#[tokio::test]
async fn wrong_success_media_type_and_interruption_never_replay() {
    for (wire, mime) in [
        (ITEM.as_bytes().to_vec(), MIME),
        (COMPLETE.as_bytes().to_vec(), "application/json"),
    ] {
        let (url, count, server) = raw(wire, 200, mime).await;
        let result = client(url).watch().await;
        match result {
            Ok(mut events) => {
                events.next().await.unwrap().unwrap();
                assert!(events.next().await.unwrap().is_err());
            }
            Err(error) => assert!(matches!(error, Error::Decode(_))),
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

struct Stamp(Arc<AtomicUsize>);
impl streams_sdk::Interceptor for Stamp {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
        let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        request
            .headers_mut()
            .insert("x-native-attempt", n.to_string().parse().unwrap());
        Ok(())
    }
}

#[derive(Debug)]
struct OpeningRequest {
    authorization: Option<String>,
    attempt: Option<String>,
    method: String,
    body: Result<Vec<u8>, String>,
}

#[tokio::test]
async fn opening_retries_reauthenticate_and_apply_middleware_but_post_does_not_retry() {
    for safe in [true, false] {
        let requests = Arc::new(AtomicUsize::new(0));
        let received = requests.clone();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let observed = observations.clone();
        let route = Router::new().fallback(move |request: axum::extract::Request| {
            let received = received.clone();
            let observed = observed.clone();
            async move {
                let n = received.fetch_add(1, Ordering::SeqCst) + 1;
                let authorization = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let attempt = request
                    .headers()
                    .get("x-native-attempt")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let method = request.method().to_string();
                let body = axum::body::to_bytes(request.into_body(), 4096)
                    .await
                    .map(|bytes| bytes.to_vec())
                    .map_err(|error| error.to_string());
                observed.lock().unwrap().push(OpeningRequest {
                    authorization,
                    attempt,
                    method,
                    body,
                });
                if n == 1 {
                    Response::builder()
                        .status(503)
                        .header("x-crucible-error", "WorkFailed")
                        .header("retry-after", "0")
                        .body(Body::from(r#"{"message":"opening failed"}"#))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(200)
                        .header("content-type", MIME)
                        .body(Body::from(COMPLETE))
                        .unwrap()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, route).await.unwrap();
        });
        let identities = Arc::new(AtomicUsize::new(0));
        let tokens = identities.clone();
        let middleware = Arc::new(AtomicUsize::new(0));
        let c = StreamsClient::new(
            Config::new(url)
                .retry(Retry {
                    max_attempts: 2,
                    base_delay: Duration::ZERO,
                    max_delay: Duration::ZERO,
                })
                .token(Token::provider(move || {
                    let n = tokens.fetch_add(1, Ordering::SeqCst) + 1;
                    async move { Some(format!("token{n}")) }
                }))
                .interceptor(Stamp(middleware.clone())),
        );
        if safe {
            let mut events = c.watch().await.unwrap();
            assert!(StreamExt::next(&mut events).await.is_none());
        } else {
            assert!(matches!(
                c.publish(PublishInput::builder().topic("opening").build().unwrap())
                    .await,
                Err(Error::Service(_))
            ));
        }
        let expected = if safe { 2 } else { 1 };
        assert_eq!(requests.load(Ordering::SeqCst), expected);
        assert_eq!(identities.load(Ordering::SeqCst), expected);
        assert_eq!(middleware.load(Ordering::SeqCst), expected);
        // Assert in the test task: a handler panic can otherwise be hidden by retry.
        let observations = observations.lock().unwrap();
        assert_eq!(observations.len(), expected);
        for (index, request) in observations.iter().enumerate() {
            let n = index + 1;
            assert_eq!(
                request.authorization.as_deref(),
                Some(format!("Bearer token{n}").as_str())
            );
            assert_eq!(request.attempt.as_deref(), Some(n.to_string().as_str()));
            assert_eq!(request.method, if safe { "GET" } else { "POST" });
            let body = request.body.as_ref().unwrap();
            if safe {
                assert!(body.is_empty());
            } else {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(body).unwrap()["topic"],
                    "opening"
                );
            }
        }
        server.abort();
    }
}

#[tokio::test]
async fn total_budget_includes_credentials_and_pre_cancelled_calls_do_not_open() {
    let (url, count, server) = raw(COMPLETE.as_bytes().to_vec(), 200, MIME).await;
    let identities = Arc::new(AtomicUsize::new(0));
    let requested = identities.clone();
    let c = StreamsClient::new(Config::new(url).token(Token::provider(move || {
        requested.fetch_add(1, Ordering::SeqCst);
        async { std::future::pending().await }
    })));
    assert!(matches!(
        c.watch_with_options(CallOptions {
            timeout: Some(Duration::from_millis(30)),
            cancellation: None
        })
        .await,
        Err(Error::Timeout)
    ));
    assert_eq!(identities.load(Ordering::SeqCst), 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        c.watch_with_options(CallOptions {
            timeout: None,
            cancellation: Some(token)
        })
        .await,
        Err(Error::Cancelled)
    ));
    assert_eq!(identities.load(Ordering::SeqCst), 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn queued_unread_item_does_not_extend_deadline_or_cancellation_lease() {
    for by_deadline in [false, true] {
        let (url, api, server) = framework("hold").await;
        let token = CancellationToken::new();
        let c = client(url);
        let mut events = c
            .watch_with_options(CallOptions {
                timeout: by_deadline.then_some(Duration::from_millis(300)),
                cancellation: Some(token.clone()),
            })
            .await
            .unwrap();
        let mut read = Box::pin(events.next());
        assert!(futures_util::poll!(read.as_mut()).is_pending());
        drop(read);
        tokio::time::sleep(Duration::from_millis(40)).await;
        if !by_deadline {
            token.cancel();
        }
        // Neither another poll nor finally/server cleanup is allowed to release this producer.
        dropped(&api).await;
        assert!(if by_deadline {
            matches!(events.next().await, Some(Err(Error::Timeout)))
        } else {
            matches!(events.next().await, Some(Err(Error::Cancelled)))
        });
        server.abort();
    }
}

#[tokio::test]
async fn connection_reset_after_success_headers_is_not_an_opening_retry() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let count = requests.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if socket.read(&mut byte).await.unwrap() == 0 {
                        return;
                    }
                    request.push(byte[0]);
                }
                count.fetch_add(1, Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {MIME}\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{ITEM}"
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            });
        }
    });
    let mut events = client(url).watch().await.unwrap();
    let mut transport = false;
    while let Some(item) = events.next().await {
        if let Err(error) = item {
            assert!(matches!(error, Error::Transport(_)), "{error:?}");
            transport = true;
            break;
        }
    }
    assert!(transport);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn verified_terminal_is_not_retroactively_timed_out_while_queued() {
    let (url, _, server) = raw(COMPLETE.as_bytes().to_vec(), 200, MIME).await;
    let c = client(url);
    let token = CancellationToken::new();
    let mut events = c
        .watch_with_options(CallOptions {
            timeout: Some(Duration::from_millis(500)),
            cancellation: Some(token.clone()),
        })
        .await
        .unwrap();
    let mut read = Box::pin(events.next());
    assert!(futures_util::poll!(read.as_mut()).is_pending());
    drop(read);
    tokio::time::sleep(Duration::from_millis(700)).await;
    token.cancel();
    assert!(events.next().await.is_none());
    server.abort();
}

#[tokio::test]
async fn services_without_declared_errors_compile_and_retain_unknown_late_metadata() {
    let wire = format!("{ITEM}{COMPLETE}");
    let (url, _, server) = raw(wire.into_bytes(), 200, MIME).await;
    let c = empty_streams_sdk::StreamsClient::new(
        empty_streams_sdk::Config::new(url).token(empty_streams_sdk::Token::fixed("token")),
    );
    let mut stream = c.watch().await.unwrap();
    assert!(matches!(
        stream.next().await.unwrap().unwrap(),
        empty_streams_sdk::Progress::Ready(_)
    ));
    assert!(stream.next().await.is_none());
    server.abort();
    let (url, _, server) = raw(b"event: crucible.error\ndata: {\"type\":\"Future\",\"status\":503,\"error\":{\"message\":\"failed\"}}\n\n".to_vec(), 200, MIME).await;
    let c = empty_streams_sdk::StreamsClient::new(
        empty_streams_sdk::Config::new(url).token(empty_streams_sdk::Token::fixed("token")),
    );
    let mut stream = c.watch().await.unwrap();
    assert!(
        matches!(stream.next().await,Some(Err(empty_streams_sdk::Error::Unknown(ref error))) if error.type_name.as_deref()==Some("Future") && error.metadata.status==503)
    );
    server.abort();
}

#[tokio::test]
async fn default_one_mib_frame_bound_and_opening_error_bound_are_enforced() {
    let large = format!(
        "data: {{\"ready\":\"{}\"}}\n\n{COMPLETE}",
        "x".repeat(1024 * 1024)
    );
    for limit in [None, Some(2 * 1024 * 1024)] {
        let (url, count, server) = raw(large.as_bytes().to_vec(), 200, MIME).await;
        let mut config = Config::new(url).token(Token::fixed("token"));
        if let Some(limit) = limit {
            config = config.max_stream_event_bytes(limit);
        }
        let mut events = StreamsClient::new(config).watch().await.unwrap();
        let item = events.next().await.unwrap();
        if limit.is_some() {
            assert!(matches!(item,Ok(Progress::Ready(ref text)) if text.len()==1024*1024));
            assert!(events.next().await.is_none());
        } else {
            assert!(matches!(item, Err(Error::Decode(_))));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }
    let (url, count, server) = raw(vec![b'x'; 300], 503, "application/json").await;
    let c = StreamsClient::new(
        Config::new(url)
            .token(Token::fixed("token"))
            .retry(Retry::none())
            .max_stream_event_bytes(256),
    );
    assert!(matches!(c.watch().await, Err(Error::Decode(_))));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn total_budget_includes_retry_after_backoff() {
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let route = Router::new().fallback(move || {
        let count = requests.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(503)
                .header("x-crucible-error", "WorkFailed")
                .header("retry-after", "60")
                .body(Body::from(r#"{"message":"opening failed"}"#))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, route).await.unwrap();
    });
    let c = client(url);
    assert!(matches!(
        c.watch_with_options(CallOptions {
            timeout: Some(Duration::from_millis(300)),
            cancellation: None
        })
        .await,
        Err(Error::Timeout)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn late_error_documentation_preserves_defaults_null_future_and_unknown_identity() {
    for (name, status, fields, page) in [
        (
            "WorkFailed",
            503,
            r#"{"message":"failed","reason":"broken"}"#,
            "WorkFailed/reason-broken",
        ),
        ("WorkFailed", 503, r#"{"message":"failed"}"#, "WorkFailed"),
        (
            "WorkFailed",
            503,
            r#"{"message":"failed","reason":null}"#,
            "WorkFailed",
        ),
        (
            "WorkFailed",
            503,
            r#"{"message":"failed","reason":"future"}"#,
            "WorkFailed",
        ),
        (
            "DefaultReason",
            419,
            r#"{"message":"failed"}"#,
            "DefaultReason/reason-broken",
        ),
        (
            "DefaultReason",
            419,
            r#"{"message":"failed","reason":null}"#,
            "DefaultReason",
        ),
        (
            "DefaultReason",
            419,
            r#"{"message":"failed","reason":"future"}"#,
            "DefaultReason",
        ),
        (
            "DefaultEmptyReason",
            420,
            r#"{"message":"failed"}"#,
            "DefaultEmptyReason/reason-",
        ),
    ] {
        let wire = format!(
            "event: crucible.error\ndata: {{\"type\":\"{name}\",\"status\":{status},\"error\":{fields}}}\n\n"
        );
        let (url, count, server) = raw(wire.into_bytes(), 200, MIME).await;
        let mut events = client(url).watch().await.unwrap();
        let error = events.next().await.unwrap().unwrap_err();
        assert_eq!(
            error.doc_url(),
            Some(format!("https://docs.example.test/errors/{page}").as_str()),
            "{name}: {fields}"
        );
        assert_eq!(error.metadata().unwrap().status, status);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(events.next().await.is_none());
        server.abort();
    }
}

#[derive(Debug)]
struct SlowError;
impl streams_sdk::runtime::OperationErrors for SlowError {
    fn decode(
        _: u16,
        _: Option<&str>,
        _: &streams_sdk::runtime::__generated::http::HeaderMap,
        _: &[u8],
    ) -> Option<Self> {
        None
    }
    fn decode_document(_: u16, _: &str, _: &[u8]) -> Option<Self> {
        // Simulates synchronous modeled decoding that crosses the remaining budget.
        std::thread::sleep(Duration::from_millis(200));
        Some(Self)
    }
    fn info(&self) -> &'static streams_sdk::runtime::operation::ErrorInfo {
        static INFO: streams_sdk::runtime::operation::ErrorInfo =
            streams_sdk::runtime::__generated::error_info(
                "WorkFailed",
                503,
                streams_sdk::runtime::operation::Fault::Server,
                false,
                false,
                &[],
            );
        &INFO
    }
}
#[tokio::test]
async fn terminal_decoding_cannot_commit_an_outcome_after_the_budget_expired() {
    let wire=b"event: crucible.error\ndata: {\"type\":\"WorkFailed\",\"status\":503,\"error\":{\"message\":\"failed\"}}\n\n";
    let (url, count, server) = raw(wire.to_vec(), 200, MIME).await;
    let transport =
        streams_sdk::runtime::Transport::new(Config::new(url).token(Token::fixed("token")));
    let mut events = transport
        .send_stream_with_options::<streams_sdk::Watch, SlowError>(
            &streams_sdk::runtime::operation::Unit {},
            &CallOptions {
                timeout: Some(Duration::from_millis(100)),
                cancellation: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(events.next().await, Some(Err(Error::Timeout))));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}
