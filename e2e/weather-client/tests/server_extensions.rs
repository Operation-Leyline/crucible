//! Payload bindings in a native service.
use std::sync::Arc;

use crucible_client_runtime_rust::{Config, Error, Field, Retry};
use crucible_server_runtime_rust::Result;

#[allow(dead_code)]
mod generated_wire {
    include!(concat!(env!("OUT_DIR"), "/wire_server.rs"));
}
use generated_wire::wire;
#[allow(dead_code)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/wire_client.rs"));
}

struct Service;
impl wire::TextOperation for Service {
    async fn text(&self, input: wire::TextInput) -> Result<wire::TextOutput> {
        if input.text.as_deref() == Some("error") {
            return Err(wire::TextFault {
                details: Some("invalid text".into()),
            }
            .into());
        }
        assert_eq!(
            input.meta.as_ref().unwrap(),
            &["{\"a\":1}".to_owned(), "{\"b\":2}".to_owned()]
        );
        Ok(wire::TextOutput {
            text: input.text,
            meta: input.meta,
        })
    }
}
impl wire::DocumentOperation for Service {
    async fn document(&self, input: wire::DocumentInput) -> Result<wire::DocumentOutput> {
        Ok(wire::DocumentOutput {
            document: input.document,
        })
    }
}

async fn serve(service: crucible_server_runtime_rust::Crucible) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    endpoint
}

#[tokio::test]
async fn text_document_error_payloads_and_media_headers_round_trip() {
    let service =
        crucible_server_runtime_rust::Crucible::builder(wire::operations(Arc::new(Service)))
            .defaults()
            .on_rejection(|_| {
                Box::new(wire::TextFault {
                    details: Some("invalid input".into()),
                })
            })
            .build()
            .unwrap();
    let client = client::WireClient::new(Config::new(serve(service).await).retry(Retry::none()));
    let meta = vec!["{\"a\":1}".into(), "{\"b\":2}".into()];
    let output = client
        .text(
            client::TextInput::builder()
                .text("hello")
                .meta(meta.clone())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(output.text().map(String::as_str), Some("hello"));
    assert_eq!(output.meta(), Some(&meta));
    let output = client
        .document(
            client::DocumentInput::builder()
                .document(client::Record::builder().name("record").build())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        output.document().unwrap().name().map(String::as_str),
        Some("record")
    );
    let error = client
        .text(client::TextInput::builder().text("error").build())
        .await
        .unwrap_err();
    let Error::Service(error) = error else {
        panic!("payload discriminator was lost")
    };
    let client::TextError::TextFault(error) = error.error;
    assert_eq!(error.details().map(String::as_str), Some("invalid text"));
}

/// An explicitly null structure payload: Crucible JSON clients (Rust, TS,
/// Python, Go) send it as the body `null`, and the server reads it as the
/// member's null, as it would `{"document": null}` in a body (#274).
#[tokio::test]
async fn an_explicitly_null_structure_payload_reaches_the_server_as_absent() {
    let service =
        crucible_server_runtime_rust::Crucible::builder(wire::operations(Arc::new(Service)))
            .defaults()
            .on_rejection(|rejection| {
                Box::new(wire::TextFault {
                    details: Some(rejection.summary()),
                })
            })
            .build()
            .unwrap();
    let endpoint = serve(service).await;
    let client = client::WireClient::new(Config::new(&endpoint).retry(Retry::none()));
    // The service echoes the payload it read: `None` sends no body back.
    let output = client
        .document(client::DocumentInput {
            document: Field::Null,
        })
        .await
        .unwrap();
    assert!(output.document.is_absent());
    let output = client
        .document(client::DocumentInput::builder().build())
        .await
        .unwrap();
    assert!(output.document.is_absent());

    // The same bodies on the wire, as every Crucible JSON client sends them.
    let http = reqwest::Client::new();
    let post = |body: &'static str| {
        http.post(format!("{endpoint}/document"))
            .header("content-type", "application/json")
            .body(body)
            .send()
    };
    for body in ["null", " \tnull\r\n", ""] {
        let response = post(body).await.unwrap();
        assert_eq!(response.status(), 200, "{body:?}");
        assert!(response.bytes().await.unwrap().is_empty(), "{body:?}");
    }
    let response = post(r#"{"name":"record"}"#).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        &response.bytes().await.unwrap()[..],
        br#"{"name":"record"}"#
    );
    // Anything else that is not the structure is still malformed, an array
    // too: a structure is a JSON object, never positional values (#292).
    for body in ["nul", "null null", "\"record\"", "[]", "[\"record\"]"] {
        let response = post(body).await.unwrap();
        assert_eq!(response.status(), 422, "{body:?}");
    }
}
