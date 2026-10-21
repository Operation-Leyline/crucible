use std::sync::Arc;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let address = std::env::var("CRUCIBLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8088".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    crucible_server_runtime_rust::host::http_on(
        streaming_consumer::service(Arc::default()),
        listener,
    )
    .await
}
