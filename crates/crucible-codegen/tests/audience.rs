#![cfg(feature = "rust")]

use crucible_codegen::{Audience, Client, Generator, Server};
mod common;

#[test]
fn shared_selection_filters_clients_and_never_filters_server_outputs() {
    let full = common::load("internal");
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
    // The generated constructor retains readonly, idempotent, internal in
    // that order; inspect the Secret operation rather than struct field text.
    let secret = code
        .split_once("static SECRET_OPERATION:")
        .unwrap()
        .1
        .split_once("\n    );")
        .unwrap()
        .0;
    assert!(secret.contains("\n        false,\n        false,\n        true,\n"));
    assert_eq!(
        server.emit_for_audience(&full, Audience::External).unwrap(),
        server.emit_for_audience(&full, Audience::Internal).unwrap()
    );
}
