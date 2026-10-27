use axum::{Router, body::Body, http::Response};
use futures_util::{StreamExt, stream};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use streams_sdk::{CallOptions, Config, Error, Progress, Token};
const MIME: &str = "text/event-stream; charset=utf-8; crucible-stream=1";

#[tokio::test]
async fn bare_cr_item_delivers_without_unrelated_next_byte() {
    for ending in ["\n\n", "\r\n\r\n", "\r\r"] {
        let release = Arc::new(tokio::sync::Notify::new());
        let signal = release.clone();
        let frame = format!("data: {{\"ready\":\"first\"}}{ending}");
        let route = Router::new().fallback(move || {
            let signal = signal.clone();
            let frame = frame.clone();
            async move {
                let body = stream::once(async move { Ok::<_, std::io::Error>(frame) })
                    .chain(stream::once(async move {
                        signal.notified().await;
                        Ok(": ping\n\n".to_owned())
                    }))
                    .chain(stream::pending());
                Response::builder()
                    .header("content-type", MIME)
                    .body(Body::from_stream(body))
                    .unwrap()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let c = streams_sdk::StreamsClient::new(Config::new(url).token(Token::fixed("token")));
        let mut events = c.watch().await.unwrap();
        let first = tokio::time::timeout(Duration::from_millis(200), events.next()).await;
        assert!(
            matches!(first.unwrap().unwrap().unwrap(), Progress::Ready(_)),
            "{ending:?} must deliver while the connection is held"
        );
        events.cancel();
        server.abort();
    }
}

#[tokio::test]
async fn buffered_entrypoint_rejects_declared_stream_before_production() {
    let api = Arc::new(streaming_consumer::Api {
        mode: "hold".into(),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let service = streaming_consumer::service(api.clone());
    let server = tokio::spawn(async move {
        crucible_server_runtime_rust::host::http_on(service, listener)
            .await
            .unwrap()
    });
    let t = streams_sdk::runtime::Transport::new(Config::new(url).token(Token::fixed("token")));
    let result = t
        .send_with_options::<streams_sdk::Watch, streams_sdk::WatchError>(
            &streams_sdk::runtime::operation::Unit {},
            &CallOptions {
                timeout: Some(Duration::from_millis(200)),
                cancellation: None,
            },
        )
        .await;
    assert!(matches!(result, Err(Error::Invalid(_))), "{result:?}");
    assert_eq!(api.polls.load(Ordering::SeqCst), 0);
    assert_eq!(api.drops.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn streaming_entrypoint_rejects_declared_unary_operation() {
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let route = Router::new().fallback(move || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .header("content-type", MIME)
                .body(Body::from(
                    "data: {\"value\":\"unexpected\"}\n\nevent: crucible.complete\ndata: {}\n\n",
                ))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
    let t = streams_sdk::runtime::Transport::new(Config::new(url).token(Token::fixed("token")));
    let result = t
        .send_stream::<streams_sdk::Read, streams_sdk::ReadError>(
            &streams_sdk::runtime::operation::Unit {},
        )
        .await;
    assert!(matches!(result, Err(Error::Invalid(_))));
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    server.abort();
}
#[tokio::test]
async fn buffered_stream_marker_is_rejected_without_opening_or_replay() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let count = count.clone();
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
                    "HTTP/1.1 200 OK\r\nContent-Type: {MIME}\r\nContent-Length: 4096\r\nConnection: close\r\n\r\ndata: {{\"ready\":\"first\"}}\n\n"
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            });
        }
    });
    let t =
        streams_sdk::runtime::Transport::new(Config::new(url).token(Token::fixed("token")).retry(
            streams_sdk::Retry {
                max_attempts: 3,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            },
        ));
    let result = t
        .send::<streams_sdk::Watch, streams_sdk::WatchError>(
            &streams_sdk::runtime::operation::Unit {},
        )
        .await;
    assert!(matches!(result, Err(Error::Invalid(_))));
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn every_public_transport_entrypoint_rejects_wrong_mode_before_authentication() {
    let credentials = Arc::new(AtomicUsize::new(0));
    let calls = credentials.clone();
    let transport = streams_sdk::runtime::Transport::new(Config::new("http://127.0.0.1:1").token(
        Token::provider(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Some("token".to_owned()) }
        }),
    ));
    let input = streams_sdk::runtime::operation::Unit {};
    let options = CallOptions::default();
    assert!(matches!(
        transport
            .send::<streams_sdk::Watch, streams_sdk::WatchError>(&input)
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        transport
            .send_with_options::<streams_sdk::Watch, streams_sdk::WatchError>(&input, &options)
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        transport
            .send_stream::<streams_sdk::Read, streams_sdk::ReadError>(&input)
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        transport
            .send_stream_with_options::<streams_sdk::Read, streams_sdk::ReadError>(&input, &options)
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(credentials.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bounded_opening_errors_keep_eligible_retries_and_attempt_limits() {
    for (status, post, recover, expected) in [
        (503, false, true, 2),
        (503, false, false, 3),
        (400, false, false, 1),
        (503, true, true, 1),
    ] {
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let route = Router::new().fallback(move || {
            let count = count.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst) + 1;
                if recover && attempt > 1 {
                    Response::builder()
                        .header("content-type", MIME)
                        .body(Body::from("event: crucible.complete\ndata: {}\n\n"))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(status)
                        .header("retry-after", "0")
                        .body(Body::from(vec![b'x'; 300]))
                        .unwrap()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let client = streams_sdk::StreamsClient::new(
            Config::new(url)
                .token(Token::fixed("token"))
                .max_stream_event_bytes(256)
                .retry(streams_sdk::Retry {
                    max_attempts: 3,
                    base_delay: Duration::ZERO,
                    max_delay: Duration::ZERO,
                }),
        );
        if post {
            let result = client
                .publish(
                    streams_sdk::PublishInput::builder()
                        .topic("opening")
                        .build()
                        .unwrap(),
                )
                .await;
            assert!(matches!(result, Err(Error::Decode(_))));
        } else {
            let result = client.watch().await;
            if recover {
                assert!(result.unwrap().next().await.is_none());
            } else {
                assert!(matches!(result, Err(Error::Decode(_))));
            }
        }
        assert_eq!(
            requests.load(Ordering::SeqCst),
            expected,
            "status {status}, post {post}, recover {recover}"
        );
        server.abort();
    }
}

#[tokio::test]
async fn terminal_crlf_may_split_across_reads_but_extra_newlines_are_rejected() {
    for tail in ["\n", "\n\n"] {
        let release = Arc::new(tokio::sync::Notify::new());
        let signal = release.clone();
        let route = Router::new().fallback(move || {
            let signal = signal.clone();
            async move {
                let body = stream::once(async {
                    Ok::<_, std::io::Error>("event: crucible.complete\r\ndata: {}\r\n\r")
                })
                .chain(stream::once(async move {
                    signal.notified().await;
                    Ok(tail)
                }));
                Response::builder()
                    .header("content-type", MIME)
                    .body(Body::from_stream(body))
                    .unwrap()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let client = streams_sdk::StreamsClient::new(Config::new(url).token(Token::fixed("token")));
        let mut events = client.watch().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), events.next())
                .await
                .is_err(),
            "terminal must still validate EOF"
        );
        release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap();
        if tail == "\n" {
            assert!(result.is_none());
        } else {
            assert!(matches!(result, Some(Err(Error::Decode(_)))));
        }
        server.abort();
    }
}
