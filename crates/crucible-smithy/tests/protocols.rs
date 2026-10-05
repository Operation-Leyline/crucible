use crucible_smithy::ir::{Shape, TimestampFormat};
use crucible_smithy::{Capabilities, Feature, Model, Protocol};
use serde_json::{Value, json};

fn model() -> Value {
    serde_json::from_str(include_str!("fixtures/dynamic-bindings.json")).unwrap()
}

#[test]
fn json_protocol_fixture_exposes_one_consistent_wire_policy() {
    let model = Model::from_json(include_str!("fixtures/protocol-json.json")).unwrap();
    let capabilities = Capabilities::new(Feature::ALL.iter().copied());
    for protocol in [
        Protocol::AwsRestJson1,
        Protocol::AwsJson10,
        Protocol::AwsJson11,
    ] {
        let service = model
            .lower_with_protocol(None, &capabilities, protocol)
            .unwrap();
        assert!(service.protocol_settings.omit_null_structure_members);
        assert!(service.protocol_settings.ignore_unknown_union_type);
        assert!(service.protocol_settings.non_finite_float_strings);
        let echo = service
            .operations
            .iter()
            .find(|op| op.name() == "Echo")
            .unwrap();
        if protocol == Protocol::AwsRestJson1 {
            assert_eq!(
                echo.request.timestamp_formats["date"],
                TimestampFormat::HttpDate
            );
            assert_eq!(
                echo.request.timestamp_formats["dates"],
                TimestampFormat::HttpDate
            );
            assert_eq!(
                echo.request.timestamp_formats["query"],
                TimestampFormat::DateTime
            );
            assert_eq!(
                echo.request.timestamp_formats["queries"],
                TimestampFormat::DateTime
            );
        } else {
            assert!(echo.request.timestamp_formats.is_empty());
            assert!(echo.request.headers.is_empty());
        }
    }
}

#[test]
fn rpc_protocols_ignore_rest_bindings_and_json_names() {
    let mut model = model();
    model["shapes"]["example.dynamic#Dynamic"]["traits"] = json!({"aws.protocols#awsJson1_0":{}});
    model["shapes"]["example.dynamic#Get"]["traits"]["smithy.api#httpChecksumRequired"] = json!({});
    // The pre-existing greedy URI, query-map, prefixes, status and jsonName
    // are ignored, so none of their target capabilities should be required.
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(
            None,
            &Capabilities::new([Feature::AwsJson10, Feature::HttpChecksum]),
        )
        .unwrap();
    assert_eq!(service.protocol, Protocol::AwsJson10);
    assert_eq!(service.requirements.len(), 2);
    assert_eq!(
        service.operations[0].checksum,
        Some(crucible_smithy::ir::Checksum::Md5)
    );
    let op = &service.operations[0];
    assert_eq!(op.method, "POST");
    assert_eq!(op.uri.to_string(), "/");
    assert!(op.request.headers.is_empty());
    assert_eq!(op.request.body, ["key", "query", "metadata", "value"]);
    let Some(Shape::Structure(input)) = service.shape(op.input.as_ref().unwrap()) else {
        panic!()
    };
    assert_eq!(input.member("value").unwrap().wire_name, "value");
}

#[test]
fn selection_prefers_crucible_and_requires_explicit_ambiguous_alternatives() {
    let mut model = model();
    let traits = &mut model["shapes"]["example.dynamic#Dynamic"]["traits"];
    traits["aws.protocols#awsJson1_0"] = json!({});
    traits["aws.protocols#awsJson1_1"] = json!({});
    let capabilities = Capabilities::new(Feature::ALL.iter().copied());
    assert_eq!(
        Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &capabilities)
            .unwrap()
            .protocol,
        Protocol::CrucibleJson
    );
    model["shapes"]["example.dynamic#Dynamic"]["traits"]
        .as_object_mut()
        .unwrap()
        .remove("crucible.protocols#json");
    let model = Model::from_json(&model.to_string()).unwrap();
    assert!(
        model
            .lower_with_capabilities(None, &capabilities)
            .unwrap_err()
            .to_string()
            .contains("explicitly select")
    );
    assert_eq!(
        model
            .lower_with_protocol(None, &capabilities, Protocol::AwsJson11)
            .unwrap()
            .protocol,
        Protocol::AwsJson11
    );
    assert!(
        model
            .lower_with_protocol(None, &capabilities, Protocol::AwsQuery)
            .unwrap_err()
            .to_string()
            .contains("not declared")
    );
}

#[test]
fn rest_timestamp_defaults_distinguish_headers_and_json_body() {
    let mut model = model();
    model["shapes"]["example.dynamic#Dynamic"]["traits"] = json!({"aws.protocols#restJson1":{}});
    let members = &mut model["shapes"]["example.dynamic#Output"]["members"];
    members["created"] = json!({"target":"smithy.api#Timestamp","traits":{"smithy.api#default":0}});
    members["modified"] = json!({"target":"smithy.api#Timestamp","traits":{"smithy.api#httpHeader":"last-modified","smithy.api#default":0}});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap();
    let Some(Shape::Structure(output)) =
        service.shape(service.operations[0].output.as_ref().unwrap())
    else {
        panic!()
    };
    let body = output.member("created").unwrap();
    let header = output.member("modified").unwrap();
    assert_eq!(
        service.timestamp_format(body),
        TimestampFormat::EpochSeconds
    );
    assert_eq!(body.client_wire_default(), Some(&json!(0)));
    assert_eq!(service.timestamp_format(header), TimestampFormat::HttpDate);
    assert_eq!(
        header.client_wire_default(),
        Some(&json!("Thu, 01 Jan 1970 00:00:00 GMT"))
    );
}
