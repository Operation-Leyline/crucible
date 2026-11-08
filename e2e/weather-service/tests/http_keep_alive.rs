//! Each request on a persistent connection gets its own pipeline state while
//! retaining the connection's peer, including after a suspended/error request.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Request};
use crucible_server_runtime_rust::{Endpoint, host};
use tokio::net::TcpListener;
use tower::ServiceExt;
use weather_service::MemoryCities;

#[tokio::test]
async fn keep_alive_requests_preserve_peer_and_isolate_request_state() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .tower_layer(tower::layer::layer_fn(|inner: Endpoint| {
            tower::service_fn(move |request: Request<Body>| {
                let inner = inner.clone();
                let peer = *request
                    .extensions()
                    .get::<SocketAddr>()
                    .expect("the HTTP host supplies the peer");
                async move {
                    tokio::task::yield_now().await;
                    let mut response = inner.oneshot(request).await?;
                    response.headers_mut().insert(
                        "x-test-peer",
                        HeaderValue::from_str(&peer.to_string()).unwrap(),
                    );
                    Ok::<_, std::convert::Infallible>(response)
                }
            })
        }))
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(host::http_on_until(service, listener, async {
        stopped.await.unwrap();
    }));
    // Match the other HTTP integration tests: the client runtime installs
    // the TLS provider required even by a raw reqwest client using HTTP.
    drop(crucible_client_runtime_rust::Config::new(format!(
        "http://{address}"
    )));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut peer = None;
    let mut ids = std::collections::HashSet::new();
    for (path, status) in [("/ping", 200), ("/cities/not-an-id", 400), ("/ping", 200)] {
        let response = client
            .get(format!("http://{address}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let actual_peer = response.headers()["x-test-peer"].clone();
        assert_eq!(peer.get_or_insert(actual_peer.clone()), &actual_peer);
        assert!(ids.insert(response.headers()["x-request-id"].clone()));
        // Read the body so the next request reuses this connection.
        response.bytes().await.unwrap();
    }
    drop(client);
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
