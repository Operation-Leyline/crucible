//! `@httpPayload` blobs, in process: the body reaches the operation exactly
//! as sent, whatever its content type, and goes back the same way; a blob
//! inside a JSON body is base64.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible::{Blob, Crucible};
use serde_json::{Value, json};
use weather_service::MemoryCities;
use weather_service::contract;

fn service() -> Crucible {
    weather_service::service(Arc::new(MemoryCities::default()))
}

async fn city(service: &Crucible) -> String {
    let created = service
        .invoke("CreateCity", json!({ "name": "Bergen" }))
        .await;
    created["output"]["city"]["id"].as_str().unwrap().to_owned()
}

/// Status, content type and body bytes.
async fn send(service: &Crucible, request: Request<Body>) -> (u16, Option<String>, Vec<u8>) {
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned());
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, content_type, body.to_vec())
}

fn put(id: &str, content_type: Option<&str>, body: &[u8]) -> Request<Body> {
    let mut request =
        Request::put(format!("/cities/{id}/photo")).header("x-photo-caption", "harbour");
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    request.body(Body::from(body.to_vec())).unwrap()
}

fn get(id: &str) -> Request<Body> {
    Request::get(format!("/cities/{id}/photo"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn the_body_reaches_the_operation_unparsed_whatever_its_content_type() {
    let service = service();
    let id = city(&service).await;
    let cases: [(Option<&str>, &[u8]); 4] = [
        (
            Some("image/jpeg"),
            b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\xfe",
        ),
        // A processor's webhook: JSON bytes whose signature covers them as sent.
        (
            Some("application/json"),
            b"{ \"id\": \"evt_1\",\n  \"amount\": 1.50 }",
        ),
        (Some("text/plain; charset=utf-8"), b"not json at all"),
        (None, b"\x00\x01\x02"),
    ];
    for (content_type, bytes) in cases {
        let (status, _, body) = send(&service, put(&id, content_type, bytes)).await;
        assert_eq!(
            status,
            200,
            "{content_type:?}: {}",
            String::from_utf8_lossy(&body)
        );
        let summary: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(summary["photo"]["size"], bytes.len());
        assert_eq!(summary["photo"]["caption"], "harbour");
        let thumbnail = Blob::from_base64(summary["photo"]["thumbnail"].as_str().unwrap()).unwrap();
        assert_eq!(
            &thumbnail[..],
            &bytes[..bytes.len().min(8)],
            "base64 in JSON"
        );

        let (status, content_type, body) = send(&service, get(&id)).await;
        assert_eq!(status, 200);
        assert_eq!(
            content_type.as_deref(),
            Some("image/jpeg"),
            "the blob's media type"
        );
        assert_eq!(body, bytes, "the bytes come back exactly");
    }
}

#[tokio::test]
async fn an_output_without_its_payload_has_no_body() {
    let service = service();
    let id = city(&service).await;
    let (status, content_type, body) = send(&service, get(&id)).await;
    assert_eq!(status, 200);
    assert_eq!(content_type, None);
    assert!(body.is_empty());
}

#[tokio::test]
async fn a_payload_is_validated_like_any_member() {
    let service = service();
    let id = city(&service).await;

    let (status, _, body) = send(&service, put(&id, Some("image/jpeg"), b"")).await;
    assert_eq!(status, 400);
    let missing: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(missing["__type"], "ValidationError", "{missing}");
    assert_eq!(missing["reason"], "fieldValidationFailed", "{missing}");
    assert_eq!(missing["fieldList"][0]["path"], "photo", "{missing}");

    let (status, _, body) = send(&service, put(&id, Some("image/jpeg"), &[7; 4097])).await;
    assert_eq!(status, 400);
    let long: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(long["fieldList"][0]["path"], "photo", "{long}");
    assert!(
        long["fieldList"][0]["message"]
            .as_str()
            .unwrap()
            .contains("between 1 and 4096"),
        "{long}"
    );
}

#[tokio::test]
async fn the_body_limit_still_applies_to_a_payload() {
    let service = weather_service::builder(Arc::new(MemoryCities::default()))
        .operation_body_limit::<contract::PutCityPhoto>(16)
        .build()
        .unwrap();
    let id = city(&service).await;
    let (status, _, body) = send(&service, put(&id, None, &[1; 17])).await;
    assert_eq!(status, 400);
    assert!(String::from_utf8_lossy(&body).contains("larger than 16 bytes"));
}

#[tokio::test]
async fn a_direct_invoke_carries_the_payload_as_base64() {
    let service = service();
    let id = city(&service).await;
    let photo = Blob::from(&b"\xff\x00raw"[..]);
    let stored = service
        .invoke(
            "PutCityPhoto",
            json!({ "cityId": id, "photo": photo.to_base64(), "caption": "pier" }),
        )
        .await;
    assert_eq!(stored["output"]["photo"]["size"], 5, "{stored}");
    let read = service
        .invoke("GetCityPhoto", json!({ "cityId": id }))
        .await;
    assert_eq!(read["output"]["photo"], photo.to_base64(), "{read}");
    assert_eq!(read["output"]["caption"], "pier", "{read}");

    let invalid = service
        .invoke(
            "PutCityPhoto",
            json!({ "cityId": id, "photo": "not base64!" }),
        )
        .await;
    assert_eq!(invalid["error"]["__type"], "ValidationError", "{invalid}");
}
