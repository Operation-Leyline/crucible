//! Sparse lists and maps (`sparse list Row: Cell`), generated from
//! `sparse/rows.cru`: a null element is read from a request, held, and
//! written to a response, while a dense list refuses one in both directions.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::{Crucible, Result};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/sparse.rs"));

struct Grid;

impl grid::RunOperation for Grid {
    async fn run(&self, input: grid::RunInput) -> Result<grid::RunOutput> {
        // A sparse row whose middle cell is null, and the parameters echoed.
        Ok(grid::RunOutput {
            rows: vec![vec![Some(json!(1)), None, Some(json!("a"))]],
            echo: input.params,
            dense: input.dense,
        })
    }
}

async fn run(body: Value) -> (u16, Value) {
    let service = Crucible::builder(grid::operations(Arc::new(Grid)))
        .defaults()
        .build()
        .expect("the service builds");
    let response = service
        .handle(
            Request::post("/run")
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_null_element_round_trips_through_a_sparse_list_and_map() {
    let (status, body) = run(json!({
        "params": { "limit": 10, "after": null },
        "dense": [1, 2]
    }))
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["rows"], json!([[1, null, "a"]]));
    assert_eq!(body["echo"], json!({ "limit": 10, "after": null }));
}

#[tokio::test]
async fn a_dense_list_refuses_a_null_element() {
    let (status, body) = run(json!({ "params": {}, "dense": [1, null] })).await;
    assert_eq!(status, 400, "{body}");
}
