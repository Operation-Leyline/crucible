//! Generate a standalone service CLI backed by the ordinary Rust SDK.

mod file;
mod names;
mod options;
mod package;
mod schema;

pub use file::File;
pub use options::Options;

use crucible_smithy::Diagnostics;
use crucible_smithy::ir::Service;

/// Generate package sources without reading the model AST or touching disk.
pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    package::validate(service, options, &mut diagnostics);
    let schema = schema::emit(service, options, &mut diagnostics);
    let client = match crucible_codegen_rust::generate_client(service) {
        Ok(client) => client,
        Err(errors) => {
            for error in errors.iter() {
                diagnostics.push(&error.shape, &error.message);
            }
            String::new()
        }
    };
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let file = |path: &str, contents: String| File {
        path: path.into(),
        contents,
    };
    Ok(vec![
        file("Cargo.toml", package::manifest(options)),
        file("schema.json", schema),
        file("src/client.rs", client),
        file("src/lib.rs", package::library(service)),
        file("src/main.rs", package::MAIN.into()),
    ])
}
