use crucible_client::{Error, ResponseMetadata, ServiceError, UnknownError};

#[test]
fn unifying_service_errors_preserves_metadata_and_unknown_failures() {
    let metadata = ResponseMetadata {
        status: 429,
        request_id: Some("request-id".into()),
        headers: Default::default(),
    };
    let error = Error::Service(ServiceError {
        error: 7_u8,
        metadata: Box::new(metadata.clone()),
    });
    let mapped = error.map_service(u64::from);
    assert_eq!(mapped.metadata(), Some(&metadata));
    let Error::Service(error) = mapped else {
        panic!("lost modeled failure")
    };
    assert_eq!(error.error, 7_u64);

    let unknown = UnknownError {
        metadata: Box::new(metadata),
        type_name: Some("FutureException".into()),
        message: Some("try later".into()),
        retryable: true,
        throttling: true,
    };
    let mapped = Error::<u8>::Unknown(unknown.clone())
        .map_service(|_| -> u64 { panic!("unknown errors must bypass modeled conversion") });
    let Error::Unknown(error) = mapped else {
        panic!("lost unknown failure")
    };
    assert_eq!(error, unknown);
    assert!(matches!(
        Error::<u8>::Cancelled.map_service(u64::from),
        Error::Cancelled
    ));
    assert!(
        matches!(Error::<u8>::Decode("bad body".into()).map_service(u64::from), Error::Decode(message) if message == "bad body")
    );
}
