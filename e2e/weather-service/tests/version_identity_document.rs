//! An empty migration must retain the frozen optional-document response.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use crucible_server_runtime_rust::{Crucible, Unversioned};
use std::sync::Arc;

#[allow(dead_code)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/version_identity_document.rs"));
}
use generated::probe;

struct Reader;
impl probe::ReadOperation for Reader {
    async fn read(&self) -> crucible_server_runtime_rust::Result<probe::ReadOutput> {
        Ok(probe::ReadOutput {
            value: Some(serde_json::Value::Null),
        })
    }
}

#[tokio::test]
async fn frozen_document_null_response_survives_generated_registration() {
    // Use the complete generated registry, so generator eligibility controls
    // the behavior under test, rather than selecting an entry by hand.
    let service = Crucible::builder(probe::operations(Arc::new(Reader)))
        .defaults()
        .api_version(Unversioned::Required)
        .build()
        .unwrap();
    for (version, expected) in [("v1", "{}"), ("v2", r#"{"value":null}"#)] {
        let response = service
            .handle(
                Request::get("/read")
                    .header("crucible-version", version)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["crucible-version"], version);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), expected.as_bytes(), "release {version}");
    }
}
