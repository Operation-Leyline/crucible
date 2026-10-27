use std::sync::{Arc, atomic::Ordering};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let api = Arc::new(streaming_consumer::Api {
        mode: std::env::args().nth(1).unwrap_or_else(|| "hold".into()),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let control = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!(
        "http://{} http://{}",
        listener.local_addr()?,
        control.local_addr()?
    );
    let counters = api.clone();
    let routes = axum::Router::new().route(
        "/",
        axum::routing::get(move || {
            let api = counters.clone();
            async move { axum::Json(serde_counts(&api)) }
        }),
    );
    tokio::spawn(async move {
        axum::serve(control, routes).await.unwrap();
    });
    crucible_server_runtime_rust::host::http_on(streaming_consumer::service(api), listener).await
}

fn serde_counts(api: &streaming_consumer::Api) -> [usize; 2] {
    [
        api.polls.load(Ordering::SeqCst),
        api.drops.load(Ordering::SeqCst),
    ]
}
