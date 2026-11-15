//! Clearable PATCH members through the generated server, without a generated client.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::Crucible;
use crucible_server_runtime_rust::permissions::Permissions;
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/clearable.rs"));
#[path = "clearable/grants.rs"]
mod grants;
#[path = "clearable/notes_handler.rs"]
mod notes_handler;

async fn patch(body: Value, allowed: bool) -> (u16, Value, usize) {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Crucible::builder(notes::operations(Arc::new(notes_handler::NotesHandler(
        calls.clone(),
    ))))
    .defaults()
    .interceptor(Permissions::new(grants::Grants(allowed)))
    .build()
    .unwrap();
    let response = service
        .handle(
            Request::patch("/clearable/n-1")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap(),
        calls.load(Ordering::SeqCst),
    )
}

#[tokio::test]
async fn absent_null_and_value_reach_the_handler_distinctly() {
    for (body, expected) in [
        (json!({}), "absent"),
        (
            json!({"description": null, "rating": null, "ordinary": null}),
            "null",
        ),
        (json!({"description": "alpha", "rating": 5}), "value:alpha"),
    ] {
        let (status, response, calls) = patch(body, true).await;
        assert_eq!(status, 200, "{response}");
        assert_eq!(calls, 1);
        assert_eq!(response["descriptionState"], expected);
        assert_eq!(
            response["ratingState"],
            if expected == "value:alpha" {
                "value:5"
            } else {
                expected
            }
        );
        assert_eq!(response["ordinaryPresent"], false);
    }
}

#[tokio::test]
async fn constraints_are_not_applied_to_a_clear() {
    let (status, response, calls) = patch(json!({"description": null, "rating": null}), true).await;
    assert_eq!(status, 200, "{response}");
    assert_eq!(calls, 1);
}

#[tokio::test]
async fn constraints_are_applied_to_a_value() {
    for body in [
        json!({"description": ""}),
        json!({"description": "UPPER"}),
        json!({"description": "toolongvalue"}),
        json!({"rating": 0}),
        json!({"rating": 11}),
    ] {
        let (status, response, calls) = patch(body, true).await;
        assert_eq!(status, 400, "{response}");
        assert_eq!(calls, 0);
    }
}

#[tokio::test]
async fn a_guarded_clear_still_requires_permission() {
    let (status, response, _) = patch(json!({}), false).await;
    assert_eq!(status, 200, "{response}");
    let (status, response, calls) = patch(json!({"description": null}), false).await;
    assert_eq!(status, 403, "{response}");
    assert_eq!(calls, 0);
}

#[tokio::test]
async fn a_null_identifier_fails_closed_as_unbound() {
    let (status, response, calls) = patch(json!({"otherId": null}), true).await;
    assert_eq!(status, 500, "{response}");
    assert_eq!(calls, 0);
}

mod payloads {
    include!(concat!(env!("OUT_DIR"), "/clearable_json_payloads.rs"));
}
#[path = "clearable/payload_handler.rs"]
mod payload_handler;

#[tokio::test]
async fn json_payloads_preserve_omission_null_and_value() {
    for (path, value) in [
        ("/document", json!({"any": "value"})),
        ("/structure", json!({"name": "value"})),
        ("/union", json!({"text": "value"})),
    ] {
        for (body, expected) in [
            (String::new(), "absent"),
            ("null".to_owned(), "null"),
            (value.to_string(), "value"),
        ] {
            let states = Arc::new(std::sync::Mutex::new(Vec::new()));
            let service = Crucible::builder(payloads::notes::operations(Arc::new(
                payload_handler::PayloadHandler(states.clone()),
            )))
            .defaults()
            .build()
            .unwrap();
            let response = service
                .handle(
                    Request::put(path)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await;
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
            assert_eq!(*states.lock().unwrap(), [expected]);
        }
    }
}
