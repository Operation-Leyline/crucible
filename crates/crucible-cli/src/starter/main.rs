crucible_server_runtime_rust::include_service!();
mod operations;

use std::sync::Arc;

use crucible_server_runtime_rust::{Crucible, JsonProtocol};
use operations::Service;

#[tokio::main]
async fn main() -> Result<(), crucible_server_runtime_rust::host::BoxError> {
    let service = Crucible::builder(hello_service::operations(Arc::new(Service)))
        .protocol(JsonProtocol)
        .build()?;
    let address = std::env::var("CRUCIBLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    crucible_server_runtime_rust::host::http_on(service, listener).await
}
