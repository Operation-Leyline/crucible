//! Smithy RPC v2 (CBOR and JSON) lowering against the official Smithy protocol test models,
//! and the HTTP Digest and SigV4a auth schemes.
use crucible_smithy::ir::{AuthScheme, HttpTestCase, Shape, TimestampFormat};
use crucible_smithy::{Capabilities, Feature, Model, Protocol, ShapeId};
use serde_json::{Value, json};

const CBOR: &str = include_str!("fixtures/rpcv2-cbor.json");
const JSON: &str = include_str!("fixtures/rpcv2-json.json");
const AUTH: &str = include_str!("fixtures/rpc-auth.json");

fn all() -> Capabilities {
    Capabilities::new(Feature::ALL.iter().copied())
}

fn id(text: &str) -> ShapeId {
    text.parse().unwrap()
}

#[test]
fn official_vectors_lower_to_rpc_v2_paths_settings_and_tests() {
    for (fixture, protocol, service_name, tests) in [
        (CBOR, Protocol::SmithyRpcV2Cbor, "RpcV2Protocol", 88),
        (JSON, Protocol::SmithyRpcV2Json, "RpcV2JsonProtocol", 82),
    ] {
        let service = Model::from_json(fixture)
            .unwrap()
            .lower_with_capabilities(None, &all())
            .unwrap();
        assert_eq!(service.protocol, protocol);
        assert!(
            service
                .requirements
                .iter()
                .any(|usage| Some(usage.feature) == protocol.feature())
        );
        for op in &service.operations {
            assert_eq!(op.method, "POST");
            assert_eq!(
                op.uri.to_string(),
                format!("/service/{service_name}/operation/{}", op.name())
            );
            assert!(op.request.headers.is_empty() && op.request.query.is_empty());
            assert!(op.request.timestamp_formats.is_empty());
        }
        let settings = &service.protocol_settings;
        assert!(settings.omit_null_structure_members && settings.ignore_unknown_union_type);
        let json = protocol == Protocol::SmithyRpcV2Json;
        assert_eq!(settings.non_finite_float_strings, json);
        assert_eq!(settings.arbitrary_precision_strings, json);
        assert_eq!(service.protocol_tests.len(), tests);
        assert!(
            service
                .protocol_tests
                .iter()
                .all(|test| test.protocol == protocol.trait_id())
        );
        let simple = service
            .protocol_tests
            .iter()
            .find(|test| {
                test.id.ends_with("SimpleScalarProperties")
                    && matches!(test.case, HttpTestCase::Request { .. })
            })
            .unwrap();
        let HttpTestCase::Request { uri, .. } = &simple.case else {
            unreachable!()
        };
        assert_eq!(
            uri,
            &format!("/service/{service_name}/operation/SimpleScalarProperties")
        );
    }
}

#[test]
fn rpc_v2_timestamps_ignore_timestamp_format() {
    let service = Model::from_json(JSON)
        .unwrap()
        .lower_with_capabilities(None, &all())
        .unwrap();
    let Some(Shape::Simple(date_time)) = service.shape(&id("smithy.protocoltests.shared#DateTime"))
    else {
        panic!("DateTime shape")
    };
    assert_eq!(date_time.timestamp_format, None);
    let Some(Shape::Structure(input)) = service.shape(&id(
        "smithy.protocoltests.rpcv2Json#TimestampFormatIgnoredIO",
    )) else {
        panic!("TimestampFormatIgnored input")
    };
    assert_eq!(input.members.len(), 4);
    for member in &input.members {
        assert_eq!(
            service.timestamp_format(member),
            TimestampFormat::EpochSeconds,
            "{}",
            member.name
        );
    }
}

#[test]
fn rpc_v2_requires_an_explicit_target_capability() {
    for (fixture, feature) in [
        (CBOR, Feature::SmithyRpcV2Cbor),
        (JSON, Feature::SmithyRpcV2Json),
    ] {
        let without: Vec<_> = Feature::ALL
            .iter()
            .copied()
            .filter(|f| *f != feature)
            .collect();
        let error = Model::from_json(fixture)
            .unwrap()
            .lower_with_capabilities(None, &Capabilities::new(without))
            .unwrap_err()
            .to_string();
        assert!(error.contains(feature.name()), "{error}");
    }
}

fn dynamic() -> Value {
    serde_json::from_str(include_str!("fixtures/dynamic-bindings.json")).unwrap()
}

#[test]
fn rpc_v2_ignores_http_bindings_and_json_names() {
    let mut model = dynamic();
    model["shapes"]["example.dynamic#Dynamic"]["traits"] = json!({"smithy.protocols#rpcv2Cbor":{}});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new([Feature::SmithyRpcV2Cbor]))
        .unwrap();
    assert_eq!(service.requirements.len(), 1);
    let op = &service.operations[0];
    assert_eq!(op.method, "POST");
    assert_eq!(op.uri.to_string(), "/service/Dynamic/operation/Get");
    assert!(op.request.headers.is_empty() && op.request.prefix_headers.is_empty());
    assert_eq!(op.request.body, ["key", "query", "metadata", "value"]);
    let Some(Shape::Structure(input)) = service.shape(op.input.as_ref().unwrap()) else {
        panic!()
    };
    assert_eq!(input.member("value").unwrap().wire_name, "value");
}

#[test]
fn several_rpc_v2_protocols_require_selection() {
    let mut model = dynamic();
    model["shapes"]["example.dynamic#Dynamic"]["traits"] = json!({
        "smithy.protocols#rpcv2Cbor":{},
        "smithy.protocols#rpcv2Json":{"http":["h2","http/1.1"]}
    });
    let model = Model::from_json(&model.to_string()).unwrap();
    let error = model
        .lower_with_capabilities(None, &all())
        .unwrap_err()
        .to_string();
    assert!(error.contains("explicitly select"), "{error}");
    let service = model
        .lower_with_protocol(None, &all(), Protocol::SmithyRpcV2Json)
        .unwrap();
    assert_eq!(service.protocol, Protocol::SmithyRpcV2Json);
    assert_eq!(service.protocol_settings.http, ["h2", "http/1.1"]);
}

fn lowered(protocol: &str, edit: &dyn Fn(&mut Value)) -> Result<(), String> {
    let mut model = dynamic();
    model["shapes"]["example.dynamic#Dynamic"]["traits"] = json!({ protocol: {} });
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &all())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn unsupported_rpc_v2_constructs_are_diagnosed() {
    const CBOR_TRAIT: &str = "smithy.protocols#rpcv2Cbor";
    const JSON_TRAIT: &str = "smithy.protocols#rpcv2Json";
    let document = |model: &mut Value| {
        model["shapes"]["example.dynamic#Input"]["members"]["doc"] =
            json!({"target":"smithy.api#Document"});
    };
    let error = lowered(CBOR_TRAIT, &document).unwrap_err();
    assert!(
        error.contains("not supported by smithy.protocols#rpcv2Cbor"),
        "{error}"
    );
    // Documents are any JSON value in RPC v2 JSON.
    assert!(lowered(JSON_TRAIT, &document).is_ok());
    let streaming = |model: &mut Value| {
        model["shapes"]["example.dynamic#Stream"] =
            json!({"type":"blob","traits":{"smithy.api#streaming":{}}});
        model["shapes"]["example.dynamic#Input"]["members"]["stream"] =
            json!({"target":"example.dynamic#Stream"});
    };
    for protocol in [CBOR_TRAIT, JSON_TRAIT] {
        let error = lowered(protocol, &streaming).unwrap_err();
        assert!(
            error.contains(&format!("@streaming is not supported with @{protocol}")),
            "{error}"
        );
    }
    let rename = |model: &mut Value| {
        model["shapes"]["example.dynamic#Dynamic"]["rename"] =
            json!({"example.dynamic#Problem":"Other"});
    };
    let error = lowered(JSON_TRAIT, &rename).unwrap_err();
    assert!(
        error.contains("cannot rename error shape example.dynamic#Problem"),
        "{error}"
    );
    let compression = |model: &mut Value| {
        model["shapes"]["example.dynamic#Get"]["traits"]["smithy.api#requestCompression"] =
            json!({"encodings":["gzip"]});
    };
    let error = lowered(CBOR_TRAIT, &compression).unwrap_err();
    assert!(
        error.contains("@requestCompression is not supported"),
        "{error}"
    );
}

#[test]
fn digest_and_sigv4a_auth_schemes_are_target_capabilities() {
    let features = [
        Feature::SmithyRpcV2Cbor,
        Feature::DigestAuth,
        Feature::SigV4a,
        Feature::BigInteger,
        Feature::BigDecimal,
    ];
    let model = Model::from_json(AUTH).unwrap();
    let service = model
        .lower_with_capabilities(None, &Capabilities::new(features))
        .unwrap();
    assert_eq!(
        service.auth_schemes[&id("smithy.api#httpDigestAuth")],
        AuthScheme::Digest
    );
    assert_eq!(
        service.auth_schemes[&id("aws.auth#sigv4a")],
        AuthScheme::SigV4a {
            signing_name: "vault".into()
        }
    );
    let auth = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.name() == name)
            .unwrap()
            .auth
            .clone()
    };
    assert_eq!(auth("Put"), [id("smithy.api#httpDigestAuth")]);
    assert_eq!(auth("Sign"), [id("aws.auth#sigv4a")]);
    assert!(auth("Ping").is_empty());
    assert_eq!(
        serde_json::to_value(&service.auth_schemes[&id("aws.auth#sigv4a")]).unwrap(),
        json!({"kind":"sigV4a","signingName":"vault"})
    );
    for missing in [Feature::DigestAuth, Feature::SigV4a] {
        let error = model
            .lower_with_capabilities(
                None,
                &Capabilities::new(features.into_iter().filter(|f| *f != missing)),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains(missing.name()), "{error}");
    }
    let mut unnamed: Value = serde_json::from_str(AUTH).unwrap();
    unnamed["shapes"]["example.vault#Vault"]["traits"]["aws.auth#sigv4a"] = json!({});
    let error = Model::from_json(&unnamed.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(features))
        .unwrap_err()
        .to_string();
    assert!(error.contains("name must be a string"), "{error}");
}

#[test]
fn rpc_v2_accepts_framed_unions_and_retains_error_status() {
    for protocol in ["rpcv2Json", "rpcv2Cbor"] {
        let mut model: Value =
            serde_json::from_str(include_str!("fixtures/event-streams.json")).unwrap();
        model["shapes"]["example.events#Events"]["traits"] =
            json!({format!("smithy.protocols#{protocol}"): {}});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &all())
            .unwrap();
        let publish = service
            .operations
            .iter()
            .find(|op| op.name() == "Publish")
            .unwrap();
        assert!(
            publish
                .request
                .event_stream
                .as_ref()
                .unwrap()
                .initial_message
        );
        assert_eq!(publish.request.body, ["channel"]);
        let Some(Shape::Structure(error)) = service.shape(&id("example.events#UnavailableError"))
        else {
            panic!("error")
        };
        assert_eq!(error.error.as_ref().unwrap().status, 503);
    }
}

#[test]
fn invalid_blob_defaults_are_diagnosed_before_generation() {
    let mut model = dynamic();
    model["shapes"]["example.dynamic#Input"]["members"]["blob"] =
        json!({"target":"smithy.api#Blob", "traits":{"smithy.api#default":"invalid!"}});
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &all())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("example.dynamic#Input$blob: blob default must be valid base64"),
        "{error}"
    );
}
