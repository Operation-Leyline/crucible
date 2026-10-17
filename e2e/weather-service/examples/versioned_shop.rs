//! Serves the versioned shop (`versioned/`: release v4, with v1, v2 and v3
//! frozen and served as the current model sees them) over HTTP on the
//! address in argv[1] (default `127.0.0.1:0`), printing `listening on
//! <address>` once it accepts connections. tools/test-e2e.sh reads that line
//! and calls it with the TypeScript client of release v1, an SDK line
//! (docs/api-versions-clients.md, 5c). A request that names no release is
//! refused, so a client that did not send its own would fail.

include!(concat!(env!("OUT_DIR"), "/versioned.rs"));
include!("../tests/support/versioned_shop.rs");

#[tokio::main]
async fn main() {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("bind");
    println!("listening on {}", listener.local_addr().expect("address"));
    crucible_server_runtime_rust::host::http_on(
        versioned_shop::service(crucible_server_runtime_rust::Unversioned::Required),
        listener,
    )
    .await
    .expect("serve");
}
