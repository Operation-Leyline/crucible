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

/// A sender-signed operation (decision 81) is a third party's to call: the
/// client and its surface leave it out, in every audience; the server
/// implements it.
#[test]
fn clients_leave_sender_signed_operations_out_and_servers_keep_them() {
    let full = common::load("sender-signed");
    let client = Client::Rust(Default::default());
    for audience in [Audience::External, Audience::Internal] {
        let files = client.emit_for_audience(&full, audience).unwrap();
        let code: String = files.iter().map(|file| file.contents.as_str()).collect();
        assert!(code.contains("get_account"), "{audience:?}");
        for gone in [
            "record_payment_event",
            "RecordPaymentEventInput",
            "report_leaked_keys",
            "LeakedKey",
        ] {
            assert!(!code.contains(gone), "{audience:?}: {gone}");
        }
        let surface = client.surface(&full, audience).unwrap();
        let operations: Vec<&str> = surface.operations.keys().map(String::as_str).collect();
        assert_eq!(operations, ["example.hooks#GetAccount"], "{audience:?}");
    }
    let options = crucible_codegen::RustServerOptions {
        mcp: true,
        ..Default::default()
    };
    let server = Generator::Server(Server::Rust(options));
    let code = &server.emit(&full).unwrap()[0].contents;
    assert!(code.contains("RECORD_PAYMENT_EVENT_OPERATION"));
    // Nor is it an MCP tool: an agent has no signature to send.
    let tools = code.split_once("MCP_TOOLS").unwrap().1;
    assert!(!tools.contains("\"RecordPaymentEvent\""), "{tools}");
    assert!(tools.contains("\"GetAccount\""), "{tools}");
}
