//! Serves the weather service over HTTP on the address in argv[1] (default
//! `127.0.0.1:0`), printing `listening on <address>` once it accepts
//! connections. tools/test-e2e.sh reads that line.

#[tokio::main]
async fn main() {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("bind");
    println!("listening on {}", listener.local_addr().expect("address"));
    crucible::host::http_on(weather_service::service(), listener)
        .await
        .expect("serve");
}
