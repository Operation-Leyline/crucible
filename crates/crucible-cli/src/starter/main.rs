mod contract;
mod operations;

use std::sync::Arc;

use crucible::{Crucible, JsonProtocol};
use operations::Service;

#[tokio::main]
async fn main() -> Result<(), crucible::host::BoxError> {
    let service = Crucible::builder(contract::operations(Arc::new(Service)))
        .protocol(JsonProtocol)
        .build()?;
    let address = std::env::var("CRUCIBLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    crucible::host::http_on(service, listener).await
}
