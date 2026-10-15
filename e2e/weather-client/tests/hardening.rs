use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crucible_client_runtime_rust::{Config, Error, Interceptor, Retry, Token};

#[allow(dead_code)]
mod fixture {
    include!(concat!(env!("OUT_DIR"), "/hardening_client.rs"));
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<Option<String>>>>);
impl Interceptor for Capture {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
        self.0.lock().unwrap().push(
            request
                .headers()
                .get("authorization")
                .map(|v| v.to_str().unwrap().to_owned()),
        );
        Err("captured before network".into())
    }
}

#[tokio::test]
async fn operation_auth_controls_token_provider_and_authorization() {
    let calls = Arc::new(AtomicU32::new(0));
    let provider_calls = calls.clone();
    let capture = Capture(Arc::default());
    let client = fixture::HardeningClient::new(
        Config::new("https://example.invalid")
            .retry(Retry::none())
            .token(Token::provider(move || {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                async { Some("fixture-token".to_owned()) }
            }))
            .interceptor(capture.clone()),
    );
    assert!(matches!(client.public_ping().await, Err(Error::Invalid(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(client.read_values().await, Err(Error::Invalid(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *capture.0.lock().unwrap(),
        vec![None, Some("Bearer fixture-token".to_owned())]
    );
}

#[tokio::test]
async fn dot_only_labels_are_rejected_before_transport() {
    let capture = Capture(Arc::default());
    let client = fixture::HardeningClient::new(
        Config::new("https://example.invalid")
            .retry(Retry::none())
            .interceptor(capture.clone()),
    );
    for key in [".", ".."] {
        let error = client
            .get_item(fixture::GetItemInput::builder().key(key).build().unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Invalid(ref message) if message.contains("dot-only")));
    }
    assert!(capture.0.lock().unwrap().is_empty());
}

#[test]
fn defaults_hold_their_value_and_dense_strings_reject_null() {
    let value: fixture::ReadValuesOutput = serde_json::from_str("{}").unwrap();
    assert_eq!(value.optional_default, 7);
    assert!(serde_json::from_str::<fixture::ReadValuesOutput>(r#"{"strings":[null]}"#).is_err());
    assert!(
        serde_json::from_str::<fixture::ReadValuesOutput>(r#"{"mapping":{"key":null}}"#).is_err()
    );
}
