//! Serves the Leyline-shaped service over HTTP on the address in argv[1]
//! (default `127.0.0.1:0`), printing `listening on <address>` once it accepts
//! connections. tools/test-consumers.py reads that line and drives the
//! generated SDK against it.

use std::sync::Arc;

use crucible_auth::{IssuerConfig, JwtAuthenticator};
use leyline_service::Audit;

#[tokio::main]
async fn main() {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("bind");
    println!("listening on {}", listener.local_addr().expect("address"));
    // Keys are fetched lazily, so starting never needs the identity provider.
    let config = IssuerConfig::cognito(
        "us-east-1",
        "us-east-1_EXAMPLE",
        vec!["leyline-app-client".into()],
    );
    let jwt = Arc::new(JwtAuthenticator::new(config).expect("a valid issuer"));
    crucible_server_runtime_rust::host::http_on(
        leyline_service::service(vec![jwt], Audit::default()),
        listener,
    )
    .await
    .expect("serve");
}
