//! Hand-written hooks survive an additive change (decision 82 in
//! docs/idl/decisions.md). `versioned_additive` is `versioned/` with an
//! operation added, FindLabels, whose output reaches `Label`, and a member
//! appended to `Label`; the migration to v4 retypes `Label.size` by hook.
//! The hooks tests/api_versions.rs serves are included here unchanged and
//! compile against it: the addition reaches a hooked shape in a direction
//! the migration already converts, so it reuses that method and adds none.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Error, Result, Unversioned};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/versioned_additive.rs"));
use shop::{
    DeleteItemInput, DeleteItemOperation, DeleteItemOutput, FindLabelsInput, FindLabelsOperation,
    FindLabelsOutput, GetItemInput, GetItemOperation, GetItemOutput, GetLabelInput,
    GetLabelOperation, GetLabelOutput, Label, ListItemsInput, ListItemsOperation, ListItemsOutput,
    ListLabelsOperation, ListLabelsOutput, PutLabelInput, PutLabelOperation, PutLabelOutput,
    RecordPaymentInput, RecordPaymentOperation, RecordPaymentOutput,
};

struct Shop;

// The hooks, as tests/api_versions.rs writes them for the model without the
// addition.
include!("support/versioned_hooks.rs");

impl FindLabelsOperation for Shop {
    async fn find_labels(&self, input: FindLabelsInput) -> Result<FindLabelsOutput> {
        Ok(FindLabelsOutput {
            labels: vec![Label {
                title: input.title,
                size: Some("4".to_owned()),
                color: None,
                shelf: Some("top".to_owned()),
            }],
        })
    }
}

// What this test does not call: the handlers are the service's, not the
// hooks', and an addition may change what they build.
fn unused() -> Error {
    Error::internal("not called by this test")
}

impl GetItemOperation for Shop {
    async fn get_item(&self, _: GetItemInput) -> Result<GetItemOutput> {
        Err(unused())
    }
}

impl ListItemsOperation for Shop {
    async fn list_items(&self, _: ListItemsInput) -> Result<ListItemsOutput> {
        Err(unused())
    }
}

impl ListLabelsOperation for Shop {
    async fn list_labels(&self) -> Result<ListLabelsOutput> {
        Err(unused())
    }
}

impl GetLabelOperation for Shop {
    async fn get_label(&self, _: GetLabelInput) -> Result<GetLabelOutput> {
        Err(unused())
    }
}

impl PutLabelOperation for Shop {
    async fn put_label(&self, _: PutLabelInput) -> Result<PutLabelOutput> {
        Err(unused())
    }
}

impl DeleteItemOperation for Shop {
    async fn delete_item(&self, _: DeleteItemInput) -> Result<DeleteItemOutput> {
        Err(unused())
    }
}

impl RecordPaymentOperation for Shop {
    async fn record_payment(&self, _: RecordPaymentInput) -> Result<RecordPaymentOutput> {
        Err(unused())
    }
}

/// The added operation reaches every release, through the hook the
/// migration to v4 already had for `Label.size`; the appended member
/// reaches every release as it is.
#[tokio::test]
async fn an_addition_reaching_a_hooked_shape_reuses_its_hook() {
    let service = Crucible::builder(shop::operations(Arc::new(Shop)))
        .defaults()
        .api_version(Unversioned::Required)
        .build()
        .unwrap();
    for (version, size) in [("v1", json!(4)), ("v3", json!(4)), ("v4", json!("4"))] {
        let response = service
            .handle(
                Request::get("/found-labels?title=new")
                    .header("crucible-version", version)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{version}");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "labels": [{ "title": "new", "size": size, "shelf": "top" }] }),
            "{version}"
        );
    }
}
