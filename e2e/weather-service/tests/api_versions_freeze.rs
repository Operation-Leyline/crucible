//! A hand-written hook survives its release being frozen (decision 82 in
//! docs/idl/decisions.md). `versioned_freeze/before` is release v2 of a
//! shelf, its migration from v1 a reshape by hook; `versioned_freeze/after`
//! is the same model once v2 is frozen and v3 adds an operation. The hook
//! names v2's types as `versions::v2::..` in both, so one source,
//! tests/support/freeze_hooks.rs, compiles against each and serves v1.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crucible_server_runtime_rust::{Crucible, Unversioned};
use serde_json::{Value, json};

async fn get(service: &Crucible, uri: &str, version: &str) -> Value {
    let response = service
        .handle(
            Request::get(uri)
                .header("crucible-version", version)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{uri} at {version}");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

// Generated as a service crate's root module would be: what this test does
// not call is unused here.
#[allow(dead_code)]
mod before {
    include!(concat!(env!("OUT_DIR"), "/versioned_freeze_before.rs"));
    include!("support/freeze_hooks.rs");

    pub struct Shelves;

    impl shop::GetShelfOperation for Shelves {
        async fn get_shelf(&self) -> crucible_server_runtime_rust::Result<shop::GetShelfOutput> {
            Ok(shop::GetShelfOutput {
                shelf: shop::Shelf {
                    name: "top".to_owned(),
                    width: Some(3),
                },
            })
        }
    }

    pub fn service() -> crucible_server_runtime_rust::Crucible {
        crucible_server_runtime_rust::Crucible::builder(shop::operations(std::sync::Arc::new(
            Shelves,
        )))
        .defaults()
        .api_version(super::Unversioned::Required)
        .build()
        .unwrap()
    }
}

// Generated as a service crate's root module would be: what this test does
// not call is unused here.
#[allow(dead_code)]
mod after {
    include!(concat!(env!("OUT_DIR"), "/versioned_freeze_after.rs"));
    include!("support/freeze_hooks.rs");

    pub struct Shelves;

    fn shelf() -> shop::Shelf {
        shop::Shelf {
            name: "top".to_owned(),
            width: Some(3),
        }
    }

    impl shop::GetShelfOperation for Shelves {
        async fn get_shelf(&self) -> crucible_server_runtime_rust::Result<shop::GetShelfOutput> {
            Ok(shop::GetShelfOutput { shelf: shelf() })
        }
    }

    impl shop::ListShelvesOperation for Shelves {
        async fn list_shelves(
            &self,
        ) -> crucible_server_runtime_rust::Result<shop::ListShelvesOutput> {
            Ok(shop::ListShelvesOutput {
                shelves: vec![shelf()],
            })
        }
    }

    pub fn service() -> crucible_server_runtime_rust::Crucible {
        crucible_server_runtime_rust::Crucible::builder(shop::operations(std::sync::Arc::new(
            Shelves,
        )))
        .defaults()
        .api_version(super::Unversioned::Required)
        .build()
        .unwrap()
    }
}

#[tokio::test]
async fn a_hook_compiles_unchanged_once_its_release_is_frozen() {
    for service in [before::service(), after::service()] {
        assert_eq!(
            get(&service, "/shelf", "v1").await,
            json!({ "name": "top", "width": 3 })
        );
        assert_eq!(
            get(&service, "/shelf", "v2").await,
            json!({ "shelf": { "name": "top", "width": 3 } })
        );
    }
    // The operation v3 added reaches v1 too, as v1 sees `Shelf` now.
    assert_eq!(
        get(&after::service(), "/shelves", "v1").await,
        json!({ "shelves": [{ "name": "top", "width": 3 }] })
    );
}
