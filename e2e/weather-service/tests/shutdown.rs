//! Graceful shutdown over a real socket: once the signal fires the server
//! stops accepting, finishes the request in flight, closes idle connections,
//! and returns.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::host;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tower::ServiceExt;
use weather_service::MemoryCities;

/// Holds every request this long, so one is still in flight at the signal.
const HANDLER_DELAY: Duration = Duration::from_millis(400);

#[tokio::test]
async fn shutdown_finishes_the_request_in_flight_then_returns() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .tower_layer(tower::layer::layer_fn(
            |inner: crucible_server_runtime_rust::Endpoint| {
                tower::service_fn(move |request: Request<Body>| {
                    let inner = inner.clone();
                    async move {
                        tokio::time::sleep(HANDLER_DELAY).await;
                        inner.oneshot(request).await
                    }
                })
            },
        ))
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (signal, stop) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(host::http_on_until(service, listener, async {
        let _ = stop.await;
    }));

    let mut idle = TcpStream::connect(address).await.unwrap();
    let mut busy = TcpStream::connect(address).await.unwrap();
    busy.write_all(b"GET /ping HTTP/1.1\r\nhost: test\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(HANDLER_DELAY / 4).await;
    signal.send(()).unwrap();

    // The request in flight is answered, then its connection closes.
    let mut answer = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), busy.read_to_end(&mut answer))
        .await
        .expect("the in-flight request finishes")
        .unwrap();
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");

    // An idle connection is closed rather than kept open.
    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), idle.read_to_end(&mut rest))
        .await
        .expect("the idle connection closes")
        .unwrap();
    assert!(rest.is_empty());

    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the server returns once its connections close")
        .unwrap()
        .unwrap();
    let refused = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address)).await;
    assert!(
        !matches!(refused, Ok(Ok(_))),
        "the listener stops accepting"
    );
}
