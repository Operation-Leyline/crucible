use std::sync::Arc;

mod browser_fixture;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let address = std::env::var("CRUCIBLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8088".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Listening on http://{}", listener.local_addr()?);
    let api = Arc::new(streaming_consumer::Api::default());
    let service = match std::env::var("CRUCIBLE_BROWSER_ORIGIN") {
        Ok(origin) => crucible_server_runtime_rust::Crucible::builder(
            streaming_consumer::streams::operations(api.clone()),
        )
        .protocol(crucible_server_runtime_rust::JsonProtocol)
        .middleware(browser_fixture::BrowserFixture::new(origin, api)?)
        .build()?,
        Err(_) => streaming_consumer::service(api),
    };
    crucible_server_runtime_rust::host::http_on(service, listener).await
}
