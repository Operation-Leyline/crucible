//! A client author's view of the generated SDK package: ordinary construction,
//! a call, and a typed modelled error. Run against the Leyline-shaped service
//! by tools/test-consumers.py, which passes its endpoint as argv[1].

use leyline_sdk::{Config, Error, GetGraphError, GetGraphInput, LeylineClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let client = LeylineClient::new(Config::new(endpoint));

    let health = client.health().await?;
    assert_eq!(health.status, "ok");

    // No credential: the service's modelled refusal arrives as a typed error.
    let refused = client
        .get_graph(GetGraphInput::builder().graph_id("g-1").build().unwrap())
        .await;
    match refused {
        Err(Error::Service(error)) => {
            assert!(
                matches!(error.error, GetGraphError::AccessDeniedException(_)),
                "{:?}",
                error.error
            );
            assert_eq!(error.metadata.status, 401);
        }
        other => panic!("expected AccessDeniedException, got {other:?}"),
    }
    println!("leyline sdk consumer ok");
    Ok(())
}
