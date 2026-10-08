//! Serves the weather service over HTTP on the address in argv[1] (default
//! `127.0.0.1:0`), printing `listening on <address>` once it accepts
//! connections. tools/test-e2e.sh reads that line. A deployment would call
//! `crucible_server_runtime_rust::host::run` with a database-backed `CityStore` instead.

use std::sync::Arc;

use weather_service::MemoryCities;

#[tokio::main]
async fn main() {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("bind");
    println!("listening on {}", listener.local_addr().expect("address"));
    let mut builder = weather_service::builder(Arc::new(MemoryCities::default()));
    if cfg!(debug_assertions) {
        builder = builder.explorer(crucible_server_runtime_rust::explorer::Explorer::new(weather_service::weather::DESCRIPTION));
    }
    crucible_server_runtime_rust::host::http_on(
        builder.build().expect("weather service"),
        listener,
    )
    .await
    .expect("serve");
}
