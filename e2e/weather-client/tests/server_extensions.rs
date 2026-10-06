//! Payload bindings and effective authentication metadata in a native service.
use std::sync::{Arc, Mutex};

use crucible::{Hook, OperationAwareMiddleware, OperationJob, Prepare, Result};
use crucible_client::{Config, Error, Retry};

#[allow(dead_code)]
mod auth {
    include!(concat!(env!("OUT_DIR"), "/auth_server.rs"));
}
#[allow(dead_code)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/wire_server.rs"));
}
#[allow(dead_code)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/wire_client.rs"));
}

struct Service;
macro_rules! unit {
    ($trait:ident, $method:ident) => {
        impl auth::$trait for Service {
            async fn $method(&self) -> Result<()> {
                Ok(())
            }
        }
    };
}
unit!(MixedOperation, mixed);
unit!(BasicOperation, basic);
unit!(ApiOperation, api);
unit!(PublicOperation, public);
unit!(OptionalOperation, optional);

impl wire::TextOperation for Service {
    async fn text(&self, input: wire::model::TextInput) -> Result<wire::model::TextOutput> {
        if input.text.as_deref() == Some("error") {
            return Err(wire::errors::TextError {
                details: Some("invalid text".into()),
            }
            .into());
        }
        assert_eq!(
            input.meta.as_ref().unwrap(),
            &["{\"a\":1}".to_owned(), "{\"b\":2}".to_owned()]
        );
        assert_eq!(input.extras.as_ref().unwrap()["one"], "{\"c\":3}");
        Ok(wire::model::TextOutput {
            text: input.text,
            meta: input.meta,
            extras: input.extras,
        })
    }
}
impl wire::DocumentOperation for Service {
    async fn document(
        &self,
        input: wire::model::DocumentInput,
    ) -> Result<wire::model::DocumentOutput> {
        Ok(wire::model::DocumentOutput {
            document: input.document,
        })
    }
}

struct Policy(Arc<Mutex<Vec<String>>>);
impl OperationAwareMiddleware for Policy {
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let mixed = prepare.operations().find(|op| op.name == "Mixed").unwrap();
        let metadata = prepare.metadata(mixed).unwrap();
        assert_eq!(metadata.auth_schemes.unwrap().len(), 4);
        assert!(metadata.unsigned_payload);
        let public = prepare.operations().find(|op| op.name == "Public").unwrap();
        assert_eq!(
            prepare
                .metadata(public)
                .unwrap()
                .auth_schemes
                .unwrap()
                .len(),
            0
        );
        Ok(())
    }
    fn before<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, Result<()>> {
        let metadata = job.metadata();
        assert!(!metadata.has_input && !metadata.has_output);
        assert_eq!(metadata.optional_auth, job.operation().name == "Optional");
        self.0.lock().unwrap().push(job.operation().name.into());
        Hook::Ready(Ok(()))
    }
}

async fn serve(service: crucible::Crucible) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(crucible::host::http_on(service, listener));
    endpoint
}

#[tokio::test]
async fn resolved_auth_contract_is_available_before_decoding() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let service = crucible::Crucible::builder(auth::operations(Arc::new(Service)))
        .defaults()
        .operation_middleware(Policy(seen.clone()))
        .on_rejection(|_| {
            Box::new(wire::errors::TextError {
                details: Some("invalid input".into()),
            })
        })
        .build()
        .unwrap();
    let endpoint = serve(service).await;
    for operation in ["mixed", "basic", "api", "public", "optional"] {
        let response = reqwest::get(format!("{endpoint}/{operation}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
    }
    assert_eq!(
        *seen.lock().unwrap(),
        ["Mixed", "Basic", "Api", "Public", "Optional"]
    );
}

#[tokio::test]
async fn text_document_error_payloads_and_media_headers_round_trip() {
    let service = crucible::Crucible::builder(wire::operations(Arc::new(Service)))
        .defaults()
        .on_rejection(|_| {
            Box::new(wire::errors::TextError {
                details: Some("invalid input".into()),
            })
        })
        .build()
        .unwrap();
    let client = client::WireClient::new(Config::new(serve(service).await).retry(Retry::none()));
    let meta = vec!["{\"a\":1}".into(), "{\"b\":2}".into()];
    let extras = std::collections::BTreeMap::from([("one".into(), "{\"c\":3}".into())]);
    let output = client
        .text(
            client::model::TextInput::builder()
                .text("hello")
                .meta(meta.clone())
                .extras(extras.clone())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(output.text().map(String::as_str), Some("hello"));
    assert_eq!(output.meta(), Some(&meta));
    assert_eq!(output.extras(), Some(&extras));
    let output = client
        .document(
            client::model::DocumentInput::builder()
                .document(client::model::Record::builder().name("record").build())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        output.document().unwrap().name().map(String::as_str),
        Some("record")
    );
    let error = client
        .text(client::model::TextInput::builder().text("error").build())
        .await
        .unwrap_err();
    let Error::Service(error) = error else {
        panic!("payload discriminator was lost")
    };
    let client::TextError::TextError(error) = error.error;
    assert_eq!(error.details().map(String::as_str), Some("invalid text"));
}
