#![cfg(feature = "rust")]

use crucible_codegen::{Generator, RustServerOptions, Server};
use crucible_smithy::Model;

#[test]
fn server_artifacts_have_a_distinct_target_and_use_the_native_emitter() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let server = Server::Rust(RustServerOptions::default());
    assert_eq!(server.capabilities(), crucible_codegen_rust::capabilities());
    let generator = Generator::from(server);
    assert_eq!(generator.target(), "rust-server");
    let files = generator.emit(&service).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "crucible_service.rs");
    assert_eq!(
        files[0].contents,
        crucible_codegen_rust::generate(&service, &RustServerOptions::default()).unwrap()
    );
}
