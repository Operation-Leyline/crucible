use crucible_smithy::ir::{
    Checksum, ChecksumAlgorithm, EventBinding, HeaderEncoding, PayloadKind, Shape,
};
use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::{Value, json};
const MODEL: &str = include_str!("fixtures/wire-extensions.json");

#[test]
fn raw_error_payloads_require_an_explicit_target_capability() {
    let model = Model::from_json(include_str!("fixtures/error-payloads.json")).unwrap();
    let caps = Capabilities::new([Feature::NonBlobPayload, Feature::MediaTypeHeaders]);
    assert!(
        model
            .lower_with_capabilities(None, &caps)
            .unwrap_err()
            .to_string()
            .contains("error-payload")
    );
    let service = model
        .lower_with_capabilities(
            None,
            &Capabilities::new([
                Feature::NonBlobPayload,
                Feature::MediaTypeHeaders,
                Feature::ErrorPayload,
                Feature::PrefixHeaders,
            ]),
        )
        .unwrap();
    let Some(Shape::Structure(error)) = service.shape(&"example.wire#TextError".parse().unwrap())
    else {
        panic!()
    };
    let payload = error.error.as_ref().unwrap().payload.as_ref().unwrap();
    assert_eq!(payload.member, "details");
    assert_eq!(payload.kind, PayloadKind::String);
    assert_eq!(payload.media_type, "text/plain");
    assert!(
        service
            .protocol_tests
            .iter()
            .any(|case| case.id == "CrucibleStringErrorPayload" && case.error)
    );
}
fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::AwsRestJson1,
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::HttpChecksum,
        Feature::Streaming,
        Feature::EventStreams,
    ])
}

#[test]
fn flexible_checksum_overrides_the_legacy_trait() {
    let mut model: Value = serde_json::from_str(MODEL).unwrap();
    model["shapes"]["example.wire#Upload"]["traits"]["smithy.api#httpChecksumRequired"] = json!({});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    let upload = service
        .operations
        .iter()
        .find(|op| op.name() == "Upload")
        .unwrap();
    assert!(matches!(
        upload.checksum,
        Some(Checksum::AwsFlexible { .. })
    ));
}

#[test]
fn payload_header_checksum_and_stream_semantics_are_typed_and_gated() {
    let model = Model::from_json(MODEL).unwrap();
    let diagnostics = model.lower(None).unwrap_err().to_string();
    for feature in capabilities().iter() {
        assert!(diagnostics.contains(feature.name()), "{diagnostics}");
    }
    let service = model
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    let op = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.name() == name)
            .unwrap()
    };
    assert_eq!(
        op("Text").request.payload.as_ref().unwrap().kind,
        PayloadKind::String
    );
    assert_eq!(
        op("Text").request.payload.as_ref().unwrap().media_type,
        "text/plain"
    );
    assert_eq!(
        op("Text").request.header_encodings["meta"],
        HeaderEncoding::Base64
    );
    assert_eq!(op("Text").checksum, Some(Checksum::Md5));
    assert_eq!(
        op("Document").request.payload.as_ref().unwrap().kind,
        PayloadKind::Document
    );
    let upload = op("Upload");
    let payload = upload.request.payload.as_ref().unwrap();
    assert!(payload.streaming && payload.requires_length);
    let Some(Checksum::AwsFlexible {
        request_algorithms,
        response_algorithms,
        ..
    }) = &upload.checksum
    else {
        panic!()
    };
    assert_eq!(
        request_algorithms,
        &[ChecksumAlgorithm::Crc32, ChecksumAlgorithm::Sha256]
    );
    assert_eq!(response_algorithms, request_algorithms);
    assert_eq!(
        op("Events").request.payload.as_ref().unwrap().kind,
        PayloadKind::EventStream
    );
    let Some(Shape::Structure(event)) = service.shape(&"example.wire#Event".parse().unwrap())
    else {
        panic!()
    };
    assert_eq!(
        event.member("name").unwrap().event_binding,
        Some(EventBinding::Header)
    );
    assert_eq!(
        event.member("data").unwrap().event_binding,
        Some(EventBinding::Payload)
    );
}

#[test]
fn malformed_streams_and_checksums_remain_diagnostics_with_all_capabilities() {
    let mut model: Value = serde_json::from_str(MODEL).unwrap();
    model["shapes"]["example.wire#Event"]["members"]["extra"] =
        json!({"target":"smithy.api#String"});
    model["shapes"]["example.wire#UploadInput"]["members"]["body"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("smithy.api#required");
    model["shapes"]["example.wire#Upload"]["traits"]["aws.protocols#httpChecksum"]["requestAlgorithmMember"] =
        json!("missing");
    let diagnostics = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap_err()
        .to_string();
    for text in [
        "every other member",
        "must be required",
        "must name an input enum member",
    ] {
        assert!(diagnostics.contains(text), "{diagnostics}");
    }
}
