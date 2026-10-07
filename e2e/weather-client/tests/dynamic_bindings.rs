//! Generated clients and servers agree on dynamic HTTP bindings over TCP.
use crucible_client_runtime_rust::{Config, Error};
use serde_json::{Value, json};
use std::sync::Arc;

#[allow(dead_code)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/dynamic_client.rs"));
}
#[allow(dead_code)]
mod generated_server {
    include!(concat!(env!("OUT_DIR"), "/dynamic_server.rs"));
}
use generated_server::dynamic as server;

struct Dynamic;
impl server::GetOperation for Dynamic {
    async fn get(
        &self,
        input: server::Input,
    ) -> crucible_server_runtime_rust::Result<server::Output> {
        let value = serde_json::to_value(input).unwrap();
        if value["key"] == "error" {
            return Err(serde_json::from_value::<server::Problem>(
                json!({"status":418,"details":{"reason":"no"}}),
            )
            .unwrap()
            .into());
        }
        assert_eq!(value["query"], json!({"free":["a b","c"]}));
        assert_eq!(value["explicit"], "preferred");
        assert_eq!(value["metadata"], json!({"owner":"rust"}));
        assert_eq!(value["wire-value"], "body");
        Ok(serde_json::from_value(
            json!({"status":207,"metadata":{"echo":"yes"},"wire-value":value["key"]}),
        )
        .unwrap())
    }
}
impl server::HeadersOperation for Dynamic {
    async fn headers(
        &self,
        input: server::HeaderInput,
    ) -> crucible_server_runtime_rust::Result<server::HeaderOutput> {
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(value["values"]["x-other"], "other");
        assert_eq!(value["explicit"], "preferred");
        Ok(serde_json::from_value(json!({"wire-status":206,"wire-explicit":"response"})).unwrap())
    }
}
impl server::PagesOperation for Dynamic {
    async fn pages(
        &self,
        input: server::PageInput,
    ) -> crucible_server_runtime_rust::Result<server::PageOutput> {
        let input = serde_json::to_value(input).unwrap();
        Ok(
            serde_json::from_value(if input.get("token").is_some_and(|v| !v.is_null()) {
                json!({})
            } else {
                json!({"wire-next":{"wire-token":"second"}})
            })
            .unwrap(),
        )
    }
}
async fn serve() -> client::DynamicClient {
    let service =
        crucible_server_runtime_rust::Crucible::builder(server::operations(Arc::new(Dynamic)))
            .protocol(crucible_server_runtime_rust::JsonProtocol)
            .on_rejection(|_| {
                Box::new(serde_json::from_value::<server::Problem>(json!({"status":422})).unwrap())
            })
            .build()
            .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    client::DynamicClient::new(Config::new(endpoint))
}
#[tokio::test]
async fn generated_dynamic_members_round_trip_over_http() {
    let client = serve().await;
    for key in ["a/b \u{e9}", "a//b/"] {
        let output = client.get(serde_json::from_value(json!({"key":key,"query":{"free":["a b","c"],"q":["shadow"],"fixed":["shadow"]},"explicit":"preferred","metadata":{"owner":"rust"},"wire-value":"body"})).unwrap()).await.unwrap();
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            json!({"status":207,"metadata":{"echo":"yes"},"wire-value":key})
        );
    }
    let output = client
        .headers(
            serde_json::from_value(
                json!({"values":{"x-explicit":"shadow","x-other":"other"},"explicit":"preferred"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(output).unwrap(),
        json!({"wire-status":206,"wire-explicit":"response"})
    );
}
#[tokio::test]
async fn modeled_error_uses_actual_status_and_prefix_headers() {
    let error = serve()
        .await
        .get(serde_json::from_value(json!({"key":"error"})).unwrap())
        .await
        .unwrap_err();
    assert_eq!(error.metadata().unwrap().status, 418);
    let Error::Service(error) = error else {
        panic!("modeled error")
    };
    let client::GetError::Problem(problem) = error.error;
    assert_eq!(
        serde_json::to_value(problem).unwrap(),
        json!({"status":418,"details":{"reason":"no"}})
    );
}
#[tokio::test]
async fn renamed_nested_tokens_paginate_and_greedy_dot_segments_are_rejected() {
    let client = serve().await;
    let mut pages = client.pages_pages(serde_json::from_value(json!({})).unwrap());
    let first: Value = serde_json::to_value(pages.next_page().await.unwrap().unwrap()).unwrap();
    assert_eq!(first, json!({"wire-next":{"wire-token":"second"}}));
    assert!(pages.next_page().await.is_some());
    assert!(pages.next_page().await.is_none());
    for key in [".", "a/../b", "a/./b"] {
        let error = client
            .get(serde_json::from_value(json!({"key":key})).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)));
    }
}

#[test]
fn hooks_observe_the_same_dynamic_status_as_the_http_writer() {
    use crucible_server_runtime_rust::ModelledError;
    use crucible_server_runtime_rust::Operation;
    let output = serde_json::from_value::<server::Output>(json!({"status":207})).unwrap();
    assert_eq!(server::Get::output_status(&output), Some(207));
    let error = serde_json::from_value::<server::Problem>(json!({"status":418})).unwrap();
    assert_eq!(error.response_status(), 418);
    let invalid = serde_json::from_value::<server::Output>(json!({"status":0})).unwrap();
    assert_eq!(server::Get::output_status(&invalid), Some(500));
}
