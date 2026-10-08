//! The server conformance tests generated from `smithy.test` vectors: which
//! vectors are selected, how each kind is called, and that whatever cannot
//! run is a diagnostic or a named skip, never a silent omission.
//! `CRUCIBLE_BLESS=1 cargo test` rewrites the golden file.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{ConformanceOptions, Options, Seed, Skip, generate_conformance};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
const WEATHER_VECTORS: &str =
    include_str!("../../crucible-smithy/tests/fixtures/weather-vectors.json");
const EVENT_STREAMS: &str = include_str!("../../crucible-smithy/tests/fixtures/event-streams.json");
const RPCV2_JSON: &str = include_str!("../../crucible-smithy/tests/fixtures/rpcv2-json.json");
const RPCV2_CBOR: &str = include_str!("../../crucible-smithy/tests/fixtures/rpcv2-cbor.json");

fn lower(source: &str, edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut model: Value = serde_json::from_str(source).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
        .unwrap()
}

/// The weather fixture with its vectors, merged the way the e2e model package
/// does: the vectors are an overlay of traits by shape id.
fn weather(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    lower(WEATHER, |model| {
        let overlay: Value = serde_json::from_str(WEATHER_VECTORS).unwrap();
        for (shape, added) in overlay.as_object().unwrap() {
            let traits = model["shapes"][shape]["traits"].as_object_mut().unwrap();
            traits.extend(added.as_object().unwrap().clone());
        }
        edit(model);
    })
}

fn traits<'a>(model: &'a mut Value, shape: &str) -> &'a mut Value {
    &mut model["shapes"][format!("example.weather#{shape}")]["traits"]
}

fn generate(service: &crucible_smithy::ir::Service) -> String {
    generate_conformance(service, &ConformanceOptions::default()).unwrap()
}

fn diagnostics(service: &crucible_smithy::ir::Service, options: &ConformanceOptions) -> String {
    generate_conformance(service, options)
        .expect_err("generation should fail")
        .to_string()
}

fn matches_golden(code: &str, file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, code).unwrap();
    }
    let golden = fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        golden == code,
        "generated code differs from tests/golden/{file}; review it and rerun with CRUCIBLE_BLESS=1"
    );
}

#[test]
fn the_weather_vectors_generate_the_checked_in_tests() {
    matches_golden(&generate(&weather(|_| {})), "weather_conformance.rs");
}

#[test]
fn every_kind_of_vector_calls_its_harness_function() {
    let code = generate(&weather(|_| {}));
    for call in [
        "conformance::request::<crate::weather::GetCity>(",
        "conformance::response::<crate::weather::GetCity>(",
        "conformance::error::<crate::weather::GetCity, crate::weather::NotFoundError>(",
        "conformance::malformed(__conformance_build, ",
    ] {
        assert!(code.contains(call), "no `{call}` in\n{code}");
    }
    // The registration is generic: no operation's behaviour is written here.
    assert!(
        code.contains(
            ".add::<crate::weather::GetCity, _, _>(stub.clone(), ::crucible_server_runtime_rust::conformance::Stub::call::<crate::weather::GetCity>)"
        ),
        "{code}"
    );
    // A `testParameters` case is a test of its own, named for its values.
    assert!(
        code.contains("fn get_city_malformed_get_city_rejects_label_off_pattern_id_not_a_city()"),
        "{code}"
    );
}

#[test]
fn params_describe_where_the_model_writes_them_differently() {
    let code = generate(&weather(|_| {}));
    // createdAt is epoch seconds, updatedAt the protocol's date-time, the
    // photo a blob, and a union keeps its members' wire names.
    for fragment in [
        "ParamMember { name: \"createdAt\", wire: \"createdAt\", shape: &SHAPE_EPOCH_SECONDS }",
        "ParamMember { name: \"updatedAt\", wire: \"updatedAt\", shape: &SHAPE_DATE_TIME }",
        "ParamMember { name: \"photo\", wire: \"photo\", shape: &SHAPE_BLOB }",
        "ParamShape::Union(&[ParamMember { name: \"coordinates\"",
        "ParamShape::Map(&SHAPE_PLAIN)",
    ] {
        assert!(code.contains(fragment), "no `{fragment}` in\n{code}");
    }
}

#[test]
fn only_vectors_for_a_server_of_this_protocol_are_generated() {
    let service = weather(|model| {
        let vectors = traits(model, "GetCity");
        vectors["smithy.test#httpRequestTests"] = json!([
            {"id":"ForClients","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","appliesTo":"client","params":{"cityId":"c-abc12345"}},
            {"id":"ForServers","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","appliesTo":"server","params":{"cityId":"c-abc12345"}},
            {"id":"ForEither","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","params":{"cityId":"c-abc12345"}},
            {"id":"ForAnother","protocol":"aws.protocols#restJson1","method":"GET","uri":"/cities/c-abc12345","params":{"cityId":"c-abc12345"}},
        ]);
    });
    let code = generate(&service);
    assert!(code.contains("id: \"ForServers\""), "{code}");
    assert!(code.contains("id: \"ForEither\""), "{code}");
    assert!(
        !code.contains("ForClients") && !code.contains("ForAnother"),
        "{code}"
    );
    assert!(
        code.contains("not generated: 1 for another protocol, 1 client-only."),
        "{code}"
    );
}

#[test]
fn a_skip_keeps_the_vector_as_an_ignored_test_with_its_reason() {
    let service = weather(|_| {});
    let options = ConformanceOptions {
        seeds: Vec::new(),
        skip: vec![
            Skip {
                vector: "GetCityOutput".into(),
                reason: "known gap".into(),
            },
            // A template id skips every case its `testParameters` expand to.
            Skip {
                vector: "ListCitiesRejectsMaxResultsOutsideRange".into(),
                reason: "all three".into(),
            },
        ],
    };
    let code = generate_conformance(&service, &options).unwrap();
    assert!(
        code.contains("#[ignore = \"known gap\"]\nfn get_city_response_get_city_output()"),
        "{code}"
    );
    assert_eq!(
        code.matches("#[ignore = \"all three\"]").count(),
        3,
        "{code}"
    );
    assert_eq!(code.matches("#[ignore").count(), 4, "{code}");
}

#[test]
fn a_skip_naming_no_vector_is_a_diagnostic() {
    let options = ConformanceOptions {
        seeds: Vec::new(),
        skip: vec![Skip {
            vector: "NoSuchVector".into(),
            reason: "stale".into(),
        }],
    };
    let error = diagnostics(&weather(|_| {}), &options);
    assert!(
        error.contains("skip `NoSuchVector` names no vector"),
        "{error}"
    );
}

#[test]
fn a_model_without_an_applicable_vector_is_a_diagnostic_not_an_empty_module() {
    let service = weather(|model| {
        for shape in model["shapes"].as_object_mut().unwrap().values_mut() {
            if let Some(traits) = shape.get_mut("traits").and_then(Value::as_object_mut) {
                traits.retain(|name, _| !name.starts_with("smithy.test#"));
            }
        }
    });
    let error = diagnostics(&service, &ConformanceOptions::default());
    assert!(error.contains("no smithy.test vector applies"), "{error}");
}

#[test]
fn vectors_that_cannot_run_are_diagnosed_unless_skipped() {
    let service = weather(|model| {
        traits(model, "GetCity")["smithy.test#httpRequestTests"] = json!([
            {"id":"Vendor","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","vendorParams":{"a":1},"params":{"cityId":"c-abc12345"}},
            {"id":"Signed","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","authScheme":"smithy.api#httpBearerAuth","params":{"cityId":"c-abc12345"}},
        ]);
        // A stream cannot be compared with params.
        traits(model, "Photo")["smithy.api#streaming"] = json!({});
        traits(model, "Photo")
            .as_object_mut()
            .unwrap()
            .remove("smithy.api#length");
        model["shapes"]["example.weather#GetCityPhotoOutput"]["members"]["photo"]["traits"]["smithy.api#required"] =
            json!({});
    });
    let error = diagnostics(&service, &ConformanceOptions::default());
    for expected in [
        "protocol test Vendor: vendorParams",
        "protocol test Signed: authScheme",
        "protocol test PutCityPhotoPayload: reaches streaming blob",
        "protocol test GetCityPhotoPayload: reaches streaming blob",
    ] {
        assert!(error.contains(expected), "no `{expected}` in\n{error}");
    }
    let skipped = ConformanceOptions {
        seeds: Vec::new(),
        skip: [
            "Vendor",
            "Signed",
            "PutCityPhotoPayload",
            "GetCityPhotoPayload",
        ]
        .map(|vector| Skip {
            vector: vector.into(),
            reason: "not applicable here".into(),
        })
        .to_vec(),
    };
    let code = generate_conformance(&service, &skipped).unwrap();
    assert!(
        code.contains("#[ignore = \"not applicable here\"]\nfn get_city_request_vendor() {}"),
        "{code}"
    );
}

#[test]
fn a_response_vector_needs_a_request_to_be_answered_through() {
    let service = weather(|model| {
        traits(model, "GetCityPhoto")
            .as_object_mut()
            .unwrap()
            .remove("smithy.test#httpRequestTests");
    });
    let error = diagnostics(&service, &ConformanceOptions::default());
    assert!(
        error.contains("GetCityPhoto needs a request to answer through"),
        "{error}"
    );
}

#[test]
fn params_the_model_does_not_have_fail_when_the_test_runs_not_when_generated() {
    // The harness reports the path in params; generation does not guess types.
    let service = weather(|model| {
        traits(model, "GetCity")["smithy.test#httpRequestTests"] = json!([
            {"id":"Typo","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/c-abc12345","params":{"cityID":"c-abc12345"}},
        ]);
    });
    assert!(generate(&service).contains("\"{\\\"cityID\\\":\\\"c-abc12345\\\"}\""));
}

#[test]
fn rpc_v2_json_vectors_are_generated_with_the_requests_they_carry() {
    let service = lower(RPCV2_JSON, |_| {});
    let applicable = service
        .protocol_tests
        .iter()
        .filter(|test| test.applies_to != Some(crucible_smithy::ir::TestAppliesTo::Client))
        .count();
    // Its error vectors have no request vector to be answered through, which
    // the generator names.
    let missing = diagnostics(&service, &ConformanceOptions::default());
    for vector in [
        "RpcV2JsonResponseComplexError",
        "RpcV2JsonResponseEmptyComplexError",
        "RpcV2JsonResponseInvalidGreetingError",
    ] {
        assert!(missing.contains(vector), "{missing}");
    }
    // The model's owner names a request the operation accepts.
    let seed = |operation: &str| Seed {
        operation: operation.into(),
        method: "POST".into(),
        uri: "/service/RpcV2JsonProtocol/operation/GreetingWithErrors".into(),
        headers: vec![("smithy-protocol".into(), "rpc-v2-json".into())],
        body: None,
    };
    let options = ConformanceOptions {
        seeds: vec![seed("GreetingWithErrors")],
        ..Default::default()
    };
    let code = generate_conformance(&service, &options).unwrap();
    assert_eq!(code.matches("\n#[test]\n").count(), applicable, "{code}");
    assert!(!code.contains("#[ignore"), "{code}");
    assert!(
        code.contains("seed: ::crucible_server_runtime_rust::conformance::HttpRequest { method: \"POST\", uri: \"/service/RpcV2JsonProtocol/operation/GreetingWithErrors\", host: None, query: &[], headers: &[(\"smithy-protocol\", \"rpc-v2-json\")]"),
        "{code}"
    );
    // A seed for an operation the service does not have is a mistake.
    let options = ConformanceOptions {
        seeds: vec![seed("GreetingWithErrors"), seed("Nope")],
        ..Default::default()
    };
    assert!(diagnostics(&service, &options).contains("seed names no operation `Nope`"),);
}

#[test]
fn cbor_services_are_refused_once_not_per_vector() {
    let service = lower(RPCV2_CBOR, |_| {});
    let error = diagnostics(&service, &ConformanceOptions::default());
    assert_eq!(error.lines().count(), 1, "{error}");
    assert!(error.contains("rpcv2Cbor"), "{error}");
}

#[test]
fn vectors_stay_out_of_every_operations_metadata() {
    // They are the IR's protocol tests, not traits the service's code carries.
    let service = weather(|_| {});
    assert!(!service.protocol_tests.is_empty());
    let code = crucible_codegen_rust::generate(&service, &Options::default()).unwrap();
    assert!(!code.contains("smithy.test#"), "{code}");
    assert!(
        code.contains("example.weather#tier"),
        "custom traits remain"
    );
}

#[test]
fn an_event_stream_cannot_be_compared_with_params() {
    let service = lower(EVENT_STREAMS, |model| {
        // The server has no checksum support to lower this operation with.
        model["shapes"]["example.events#Events"]["operations"]
            .as_array_mut()
            .unwrap()
            .retain(|operation| operation["target"] != "example.events#SubscribeChecksums");
        model["shapes"]["example.events#Chat"]["traits"]["smithy.test#httpRequestTests"] = json!([
            {"id":"Streams","protocol":"aws.protocols#restJson1","method":"POST","uri":"/chat","params":{}},
        ]);
    });
    let error = diagnostics(&service, &ConformanceOptions::default());
    assert!(
        error.contains("protocol test Streams: reaches event stream"),
        "{error}"
    );
}

#[test]
fn every_vector_reaching_a_cached_stream_shape_is_diagnosed() {
    let service = weather(|model| {
        traits(model, "Photo")["smithy.api#streaming"] = json!({});
        traits(model, "Photo")
            .as_object_mut()
            .unwrap()
            .remove("smithy.api#length");
        model["shapes"]["example.weather#GetCityPhotoOutput"]["members"]["photo"]["traits"]["smithy.api#required"] =
            json!({});
        let requests = traits(model, "PutCityPhoto")["smithy.test#httpRequestTests"]
            .as_array_mut()
            .unwrap();
        let mut again = requests[0].clone();
        again["id"] = json!("PutCityPhotoAgain");
        requests.push(again);
    });
    let error = diagnostics(&service, &ConformanceOptions::default());
    for expected in ["PutCityPhotoPayload", "PutCityPhotoAgain"] {
        assert!(
            error.contains(&format!("protocol test {expected}: reaches streaming blob")),
            "no diagnostic for {expected} in\n{error}"
        );
    }
}
