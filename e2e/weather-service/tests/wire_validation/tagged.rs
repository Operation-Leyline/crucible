//! Tagged enum failures keep paths without exposing sensitive keys.
use super::*;

/// A numeric JSON value used to lose its path when read as a tagged enum.
#[tokio::test]
async fn a_tagged_enum_number_in_a_map_names_the_entry() {
    assert_tagged_decode_path(br#"{"tagged":{"p":42}}"#, "tagged/p").await;
}

/// The same typed syntax error must retain the index of a list element.
#[tokio::test]
async fn a_tagged_enum_number_in_a_list_names_the_element() {
    assert_tagged_decode_path(br#"{"choices":[{"text":"ok"},42]}"#, "choices/1").await;
}

async fn assert_tagged_decode_path(bytes: &'static [u8], expected: &str) {
    let service = service("valid", Records::default(), Records::default());
    for path in ["/body", "/bound"] {
        let (status, body) = answer(&service, path, bytes).await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["__type"], "ValidationError");
        assert_eq!(body["reason"], "cannotParse");
        assert_eq!(body["fieldList"][0]["path"], expected, "{path}: {body}");
    }
    let value = serde_json::from_slice(bytes).unwrap();
    let body = service.invoke("BodyOnly", value).await;
    assert_eq!(body["error"]["__type"], "ValidationError");
    assert_eq!(body["error"]["reason"], "cannotParse");
    assert_eq!(body["error"]["fieldList"][0]["path"], expected, "{body}");
    let body: Value =
        serde_json::from_slice(&service.invoke_json("BodyOnly", bytes).await).unwrap();
    assert_eq!(body["error"]["__type"], "ValidationError");
    assert_eq!(body["error"]["reason"], "cannotParse");
    assert_eq!(body["error"]["fieldList"][0]["path"], expected, "{body}");
}

#[tokio::test]
async fn a_tagged_enum_number_in_a_sensitive_map_hides_the_key() {
    assert_tagged_decode_path(br#"{"secret":{"private-key":42}}"#, "secret/[key]").await;
}

#[tokio::test]
async fn a_tagged_enum_in_a_map_accepts_its_tagged_value() {
    assert_tagged_value(
        br#"{"tagged":{"p":{"text":"ok"}}}"#,
        "tagged",
        json!({"p":{"text":"ok"}}),
    )
    .await;
}

#[tokio::test]
async fn a_tagged_enum_in_a_list_accepts_its_tagged_value() {
    assert_tagged_value(
        br#"{"choices":[{"text":"ok"}]}"#,
        "choices",
        json!([{"text":"ok"}]),
    )
    .await;
}

async fn assert_tagged_value(bytes: &'static [u8], member: &str, expected: Value) {
    let service = service("valid", Records::default(), Records::default());
    for path in ["/body", "/bound"] {
        let (status, body) = answer(&service, path, bytes).await;
        assert_eq!(status, 200, "{path}: {body}");
        assert_eq!(body[member], expected, "{path}: {body}");
    }
}
