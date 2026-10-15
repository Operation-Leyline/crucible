//! Non-blob payloads, error payloads and base64 media-type headers over
//! loopback HTTP, against the shared `error-payloads` fixture and the
//! `payload-kinds` fixture (union, document and enum payloads).
mod common;

use std::sync::{Arc, Mutex};

use crucible_client_runtime_rust::{Config, Error, Field, Retry};

#[allow(dead_code)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/wire_client.rs"));
}
#[allow(dead_code)]
mod kinds {
    include!(concat!(env!("OUT_DIR"), "/kinds_client.rs"));
}

/// Echoes the body and the media headers, as the Go and Python suites do.
fn echo(
    invalid: Arc<Mutex<Option<String>>>,
    seen: Arc<Mutex<Vec<common::Request>>>,
) -> impl Fn(common::Request) -> common::Response {
    move |request| {
        seen.lock().unwrap().push(request.clone());
        if request.path() == "/fail" {
            return (
                422,
                vec![("x-crucible-error".into(), "TextFault".into())],
                b"invalid text".to_vec(),
            );
        }
        let mut headers: Vec<(String, String)> = request
            .headers
            .iter()
            .filter(|(name, _)| name == "x-meta")
            .cloned()
            .collect();
        if let Some(value) = invalid.lock().unwrap().clone() {
            headers.retain(|(name, _)| name != "x-meta");
            headers.push(("x-meta".into(), value));
        }
        if let Some(content_type) = request.header("content-type") {
            headers.push(("content-type".into(), content_type.into()));
        }
        (200, headers, request.body)
    }
}

#[tokio::test]
async fn text_and_structure_payloads_round_trip_with_base64_media_headers() {
    let invalid = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fail = Arc::new(Mutex::new(false));
    let handler = echo(invalid.clone(), seen.clone());
    let failing = fail.clone();
    let endpoint = common::serve(move |mut request| {
        if *failing.lock().unwrap() {
            request.target = "/fail".into();
        }
        handler(request)
    })
    .await;
    let client = wire::WireClient::new(Config::new(endpoint).retry(Retry::none()));

    let meta = vec![r#"{"key":"héllo"}"#.to_owned(), "a,b".into(), String::new()];
    let input = wire::TextInput::builder()
        .text("héllo 🌍")
        .meta(meta.clone())
        .build();
    let output = client.text(input.clone()).await.unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.body, "héllo 🌍".as_bytes());
    assert_eq!(request.header("content-type"), Some("text/plain"));
    assert_eq!(
        request.header("x-meta"),
        Some(r#"eyJrZXkiOiJow6lsbG8ifQ==, YSxi, """#)
    );
    assert_eq!(output.text(), Some(&"héllo 🌍".to_owned()));
    assert_eq!(output.meta(), Some(&meta));

    let record = wire::Record::builder().name("nested").build();
    let result = client
        .document(wire::DocumentInput::builder().document(record).build())
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.body, br#"{"name":"nested"}"#);
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(
        result.document().unwrap().name(),
        Some(&"nested".to_owned())
    );

    // An absent structure payload sends no body and reads back absent.
    let result = client
        .document(wire::DocumentInput::builder().build())
        .await
        .unwrap();
    assert!(seen.lock().unwrap().last().unwrap().body.is_empty());
    assert!(result.document.is_absent());

    // An explicitly null structure payload is `null` on the wire and reads
    // back as explicit null (#274).
    let result = client
        .document(wire::DocumentInput {
            document: Field::Null,
        })
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().last().unwrap().body, b"null");
    assert!(result.document.is_null());

    // An empty string is a present, empty text payload.
    let mut empty = input.clone();
    empty.text = Field::Value(String::new());
    let output = client.text(empty.clone()).await.unwrap();
    assert_eq!(output.text(), Some(&String::new()));

    for header in ["%%%", "/w=="] {
        *invalid.lock().unwrap() = Some(header.to_owned());
        let error = client.text(empty.clone()).await.unwrap_err();
        assert!(matches!(error, Error::Decode(_)), "{header}: {error:?}");
    }
    *invalid.lock().unwrap() = None;

    *fail.lock().unwrap() = true;
    let error = client.text(empty).await.unwrap_err();
    let Error::Service(error) = error else {
        panic!("modeled text error: {error:?}")
    };
    let wire::TextError::TextFault(error) = error.error;
    assert_eq!(error.details(), Some(&"invalid text".to_owned()));
}

#[tokio::test]
async fn union_document_and_enum_payloads_and_error_payloads() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let endpoint = common::serve(move |request: common::Request| {
        log.lock().unwrap().push(request.clone());
        match request.path() {
            "/fail" if request.header("x-mode") == Some("binary") => (
                503,
                vec![("x-crucible-error".into(), "BinaryError".into())],
                vec![0, 159, 146, 150],
            ),
            "/fail" => (
                409,
                vec![
                    ("x-crucible-error".into(), "RecordError".into()),
                    ("x-request-tag".into(), "tag-1".into()),
                    ("content-type".into(), "application/json".into()),
                ],
                br#"{"reason":"conflict","code":7}"#.to_vec(),
            ),
            // An unknown union arm from a newer service.
            "/choice" if request.body == br#"{"count":2}"# => {
                (200, vec![], br#"{"later":true}"#.to_vec())
            }
            _ => (200, vec![], request.body),
        }
    })
    .await;
    let client = kinds::KindsClient::new(Config::new(endpoint.clone()).retry(Retry::none()));

    let choice = kinds::Choice::Text("hi".into());
    let output = client
        .put_choice(
            kinds::PutChoiceInput::builder()
                .choice(choice.clone())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        seen.lock().unwrap().last().unwrap().body,
        br#"{"text":"hi"}"#
    );
    assert_eq!(output.choice(), Some(&choice));
    let output = client
        .put_choice(
            kinds::PutChoiceInput::builder()
                .choice(kinds::Choice::Count(2))
                .build(),
        )
        .await
        .unwrap();
    let Some(kinds::Choice::UnknownValue(unknown)) = output.choice() else {
        panic!("unknown arm: {output:?}")
    };
    assert_eq!(unknown.name, "later");

    let document: serde_json::Value =
        serde_json::from_str(r#"{"nested":[1,"two",null],"exact":12345678901234567890.5}"#)
            .unwrap();
    let output = client
        .put_free(
            kinds::PutFreeInput::builder()
                .document(document.clone())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(output.document(), Some(&document));
    // Explicit null is a document value; omission remains an empty body.
    let output = client
        .put_free(kinds::PutFreeInput {
            document: Field::Null,
        })
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().last().unwrap().body, b"null");
    // A `null` payload reads back as explicit null, as a body member does.
    assert!(output.document.is_null());
    let output = client
        .put_free(kinds::PutFreeInput::builder().build())
        .await
        .unwrap();
    assert!(seen.lock().unwrap().last().unwrap().body.is_empty());
    assert!(output.document.is_absent());

    let output = client
        .put_mood(
            kinds::PutMoodInput::builder()
                .mood(kinds::Mood::Grumpy)
                .build(),
        )
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.body, b"grumpy");
    assert_eq!(request.header("content-type"), Some("text/plain"));
    assert_eq!(output.mood(), &kinds::Mood::Grumpy);

    let Error::Service(error) = client.fail().await.unwrap_err() else {
        panic!("modeled record error")
    };
    let kinds::FailError::RecordError(error) = error.error else {
        panic!("record error")
    };
    assert_eq!(error.request_tag(), Some(&"tag-1".to_owned()));
    let detail = error.detail().unwrap();
    assert_eq!(detail.reason(), Some(&"conflict".to_owned()));
    assert_eq!(detail.code(), Some(&7));

    let binary = kinds::KindsClient::new(
        Config::new(endpoint)
            .retry(Retry::none())
            .header("x-mode".parse().unwrap(), "binary".parse().unwrap()),
    );
    let Error::Service(error) = binary.fail().await.unwrap_err() else {
        panic!("modeled binary error")
    };
    assert_eq!(error.metadata.status, 503);
    let kinds::FailError::BinaryError(error) = error.error else {
        panic!("binary error")
    };
    assert_eq!(error.dump().unwrap().as_bytes(), [0, 159, 146, 150]);
}
