use notes_sdk::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The client sends Crucible-Version: 1 on every request.
    let client = NotesClient::new(runtime::Config::new(std::env::var("NOTES_ENDPOINT")?));

    // Values from the model's example "Clear description".
    let input = ClearInput::builder()
        .clear_description()
        .other("keep")
        .build();

    client.clear(input).await?;
    Ok(())
}
