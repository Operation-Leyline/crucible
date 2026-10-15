use crucible_client_runtime_rust::Config;
use hello_client::{HelloServiceClient, SayHelloInput};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let client = HelloServiceClient::new(Config::new(endpoint));
    let output = client
        .say_hello(SayHelloInput::builder().name("World").build()?)
        .await?;
    println!("{}", output.message);
    Ok(())
}
