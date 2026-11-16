use notes_sdk::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The client sends Crucible-Version: 1 on every request.
    let client = NotesClient::new(runtime::Config::new(std::env::var("NOTES_ENDPOINT")?));

    // Values from the model's example "Clear description with a collision".
    let input = CollisionInput::builder()
        .clear_description2()
        .clear_description("keep")
        .build();

    client.collision(input).await?;
    Ok(())
}
