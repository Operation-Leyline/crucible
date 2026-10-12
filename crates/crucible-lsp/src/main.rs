//! `crucible-lsp`: a language server for `.cru` files over standard I/O.
//! Editors start it; it reports the compiler's diagnostics as you type and
//! provides completion, semantic highlighting, hover, navigation, rename,
//! folding, formatting and quick fixes. It also checks `crucible.toml` manifests.

mod actions;
mod analysis;
mod completion;
mod folding;
mod hover;
mod insight;
mod lines;
mod manifest;
mod policy;
mod server;

use lsp_server::Connection;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("crucible-lsp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let (connection, io_threads) = Connection::stdio();
    let (id, params) = connection.initialize_start()?;
    let params: lsp_types::InitializeParams = serde_json::from_value(params)?;
    connection.initialize_finish(
        id,
        json!({
            "capabilities": server::capabilities(&params),
            "serverInfo": { "name": "crucible-lsp", "version": env!("CARGO_PKG_VERSION") },
        }),
    )?;
    server::Server::new(connection, &params).run()?;
    io_threads.join()?;
    Ok(())
}
