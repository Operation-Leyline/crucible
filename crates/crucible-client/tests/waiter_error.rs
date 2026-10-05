use crucible_client::{Error, WaiterError, WaiterFailure};

#[test]
fn large_models_keep_waiter_errors_small_without_losing_response_or_source() {
    type Failure = WaiterError<[u8; 4096], [u8; 4096]>;
    // Generated Result signatures must remain usable with large model types
    // under the same result_large_err limit enforced by consumer Clippy.
    assert!(std::mem::size_of::<Failure>() < 128);
    let failure = Failure {
        waiter: "Ready".into(),
        kind: WaiterFailure::Failure,
        attempts: 2,
        response: Some(Box::new([7; 4096])),
        cause: Some(Box::new(Error::Invalid("retained cause".into()))),
        detail: None,
    };
    assert_eq!(failure.response.as_deref(), Some(&[7; 4096]));
    assert_eq!(
        std::error::Error::source(&failure).unwrap().to_string(),
        "invalid request: retained cause"
    );
    assert!(failure.to_string().contains("after 2 attempts"));
}
