#![cfg(feature = "rust")]

use crucible_codegen::{Audience, Client, Generator, Server};
use crucible_smithy::Model;

#[test]
fn shared_selection_filters_clients_and_never_filters_server_outputs() {
    let full = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/internal-cors.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let client = Client::Rust(Default::default());
    let files = client.emit(&full).unwrap();
    assert!(
        files
            .iter()
            .all(|file| !file.contents.contains("SecretInput"))
    );
    let files = client.emit_for_audience(&full, Audience::Internal).unwrap();
    assert!(
        files
            .iter()
            .any(|file| file.contents.contains("SecretInput"))
    );
    let server = Generator::Server(Server::Rust(Default::default()));
    let files = server.emit(&full).unwrap();
    let code = &files[0].contents;
    assert!(code.contains("internal: true"));
    assert!(code.contains("CorsInfo"));
    for header in [
        "x-private",
        "x-secret",
        "x-private-error",
        "x-private-output",
    ] {
        assert!(code.contains(header), "missing server CORS header {header}");
    }
    assert_eq!(
        server.emit_for_audience(&full, Audience::External).unwrap(),
        server.emit_for_audience(&full, Audience::Internal).unwrap()
    );
}
