//! The weather fixture's generated code, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

#[test]
fn service_exception_uses_rust_type_spelling() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    service.id = crucible_smithy::ShapeId::new("example.weather", "weather_api");
    let source = generate_client(&service).unwrap();
    assert!(source.contains("pub enum WeatherApiException"));
}

#[test]
fn standard_exception_models_and_answer_functions_match_the_golden_file() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/standard-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    // The existing registration tests cover empty services; this snapshot
    // records the shared exception types, wire metadata and response factories.
    let errors = code
        .split("/// The service, as the framework sees it.")
        .next()
        .unwrap();
    matches_golden(&format!("{}\n", errors.trim_end()), "standard_errors.rs");
}

#[test]
fn empty_services_can_generate_their_declared_rejection_errors() {
    let source = include_str!("../../crucible-smithy/tests/fixtures/empty-service-errors.json");
    let service = Model::from_json(source).unwrap().lower(None).unwrap();
    let code = generate(
        &service,
        &Options {
            require_rejection: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(code.contains("pub struct ValidationError"));
    assert!(code.contains("pub struct InternalError"));
    assert!(code.contains("reject: Some(reject)"));
    assert!(code.contains("internal: Some(internal)"));
}

#[test]
fn high_level_services_diagnose_missing_rejection_errors_before_startup() {
    let model = include_str!("../../../tests/conformance/empty-service.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let diagnostics = generate(
        &service,
        &Options {
            require_rejection: true,
            ..Options::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(diagnostics.contains(&service.id.to_string()));
    assert!(diagnostics.contains("errors: [ValidationException]"));
    assert!(diagnostics.contains("custom_rejections()"));
    // The existing JSON-AST path can still supply its own runtime handler.
    assert!(generate(&service, &Options::default()).is_ok());
}

#[test]
fn requiring_a_rejection_error_preserves_a_valid_services_generated_code() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let options = Options {
        require_rejection: true,
        ..Options::default()
    };
    assert_eq!(
        generate(&service, &options).unwrap(),
        generate(&service, &Options::default()).unwrap()
    );
}

#[test]
fn empty_services_do_not_import_unused_redaction_types() {
    let model = include_str!("../../../tests/conformance/empty-service.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    for code in [
        generate_client(&service).unwrap(),
        generate(&service, &Options::default()).unwrap(),
    ] {
        assert!(
            code.lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n")
                .contains("mod __redaction {\n}")
        );
        assert!(!code.contains("operation::Redaction;"));
    }
}

#[test]
fn client_optional_defaults_are_removed_only_from_client_code() {
    let model = include_str!("../../crucible-smithy/tests/fixtures/review-hardening.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let client = generate_client(&service).unwrap();
    let server = generate(&service, &Options::default()).unwrap();
    assert!(!client.contains("defaults::values__optional_default"));
    assert!(server.contains("defaults::values__optional_default"));
    assert!(client.contains("fn classify(status:"));
    assert!(client.contains("const AUTH_BEARER: bool = false;"));
    assert!(client.contains("const AUTH_BEARER: bool = true;"));
}

#[test]
fn service_client_name_cannot_collide_with_a_model() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#CitySummary":"WeatherClient"});
    });
    let diagnostics = generate_client(&service).unwrap_err();
    assert!(
        diagnostics
            .to_string()
            .contains("generated client `WeatherClient`")
    );
}

fn edited(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn float_ranges_use_the_members_precision() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#Coordinates"]["members"];
        members["latitude"]["target"] = json!("smithy.api#Float");
        members["latitude"]["traits"]["smithy.api#range"] = json!({"min": 0.7});
        members["longitude"]["traits"]["smithy.api#range"] = json!({"max": 0.1});
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("validator.range_float32(*value, Some(0.7f64), None);"));
    assert!(code.contains("validator.range_float(*value as f64, None, Some(0.1f64));"));
    let checks = code
        .lines()
        .filter(|line| line.contains("validator.range_float"))
        .map(|line| format!("{}\n", line.trim()))
        .collect::<String>();
    matches_golden(&checks, "float_ranges.rs");
}

#[test]
fn weather_matches_the_golden_file() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    matches_golden(&code, "weather.rs");
}

#[test]
fn weather_client_matches_the_golden_file() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate_client(&service).unwrap();
    matches_golden(&code, "weather_client.rs");
}

#[test]
fn non_throttling_retryability_reaches_client_metadata() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ConflictError"]["traits"]["smithy.api#retryable"] =
            json!({});
    });
    let code = generate_client(&service).unwrap();
    assert!(
        code.contains("retryable: true,\n        throttling: false,"),
        "{code}"
    );
}

#[test]
fn a_sensitive_blob_payload_is_redacted_from_logs() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Photo"]["traits"]["smithy.api#sensitive"] = json!({});
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(
        code.contains(
            "name: \"photo\", location: ::crucible_server_runtime_rust::operation::Location::Payload(\"image/jpeg\"), scalar: ::crucible_server_runtime_rust::operation::Scalar::Json, redaction: &::crucible_server_runtime_rust::operation::Redaction::Sensitive"
        ),
        "{code}"
    );
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
fn a_validation_error_crucible_cannot_fill_leaves_rejection_to_the_service() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ValidationError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("    reject: None,"), "{code}");
    assert!(!code.contains("fn reject("));
}

#[test]
fn a_pattern_the_rust_regex_crate_cannot_run_is_a_diagnostic() {
    let service = edited(|model| {
        model["shapes"]["example.weather#CityId"]["traits"]["smithy.api#pattern"] =
            json!("^(?!x)c-.*$");
    });
    let diagnostics = generate(&service, &Options::default()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("does not compile")),
        "{diagnostics}"
    );
}

#[test]
fn a_configured_error_that_is_not_an_error_fails_generation() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for require_rejection in [false, true] {
        let options = Options {
            require_rejection,
            validation_error: Some("example.weather#CitySummary".parse().unwrap()),
            internal_error: Some("example.weather#Missing".parse().unwrap()),
            ..Options::default()
        };
        let diagnostics = generate(&service, &options).unwrap_err();
        let messages: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages
                .iter()
                .all(|m| m.contains("is not an @error structure"))
        );
    }
}

#[test]
fn a_configured_validation_error_crucible_cannot_fill_fails_generation() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ConflictError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    for require_rejection in [false, true] {
        let options = Options {
            require_rejection,
            validation_error: Some("example.weather#ConflictError".parse().unwrap()),
            ..Options::default()
        };
        let diagnostics = generate(&service, &options).unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{diagnostics}");
        assert_eq!(
            diagnostics.to_string().contains("custom_rejections()"),
            require_rejection
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("on_rejection")),
            "{diagnostics}"
        );
    }
}

#[test]
fn a_paginated_operation_fills_a_fresh_token_per_page_unless_the_caller_set_one() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ListCitiesInput"]["members"]["clientToken"] = json!({ "target": "smithy.api#String", "traits": { "smithy.api#idempotencyToken": {} } });
    });
    let code = generate_client(&service).unwrap();
    assert!(
        code.contains("let fresh = input.client_token.value().is_none();"),
        "{code}"
    );
    assert!(code.contains(
        "pages.fresh_token(|input| input.client_token = ::crucible_client_runtime_rust::Field::Value(::crucible_client_runtime_rust::idempotency_token()))"
    ));
}

#[test]
fn strict_structures_refuse_unknown_members_on_the_server_only() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let strict = "#[serde(deny_unknown_fields)]\n        pub struct ";
    let marked = generate(&service, &Options::default()).unwrap();
    assert!(marked.contains(&format!("{strict}CreateCityInput")));
    assert!(
        !marked.contains(&format!("{strict}GetCityInput")),
        "unmarked"
    );

    let all = Options {
        strict_inputs: true,
        ..Options::default()
    };
    let everything = generate(&service, &all).unwrap();
    for input in ["GetCityInput", "ListCitiesInput", "Coordinates"] {
        assert!(everything.contains(&format!("{strict}{input}")), "{input}");
    }
    assert!(
        !everything.contains(&format!("{strict}CitySummary")),
        "output only"
    );

    let client = generate_client(&service).unwrap();
    assert!(
        !client.contains("deny_unknown_fields"),
        "clients stay lenient"
    );
}

#[test]
fn without_the_standard_reason_an_unknown_operation_keeps_the_protocol_answer() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#ValidationError"]["members"];
        members.as_object_mut().unwrap().remove("reason");
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("    unknown: None,"), "{code}");
    assert!(!code.contains("fn unknown("));
}

#[test]
fn the_standard_reason_matches_whatever_casing_the_model_uses() {
    let service = edited(|model| {
        let members = &mut model["shapes"]["example.weather#ValidationReason"]["members"];
        for (_, member) in members.as_object_mut().unwrap() {
            let value = member["traits"]["smithy.api#enumValue"].as_str().unwrap();
            let snake: String = value
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_uppercase() {
                        vec!['_', c.to_ascii_lowercase()]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            member["traits"]["smithy.api#enumValue"] = json!(snake);
        }
    });
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("model::ValidationReason::CannotParse } else { model::ValidationReason::FieldValidationFailed }"));
    assert!(code.contains("reason: model::ValidationReason::UnknownOperation"));
}

#[test]
fn scaffold_plans_a_file_per_operation_in_a_folder_per_resource() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let stubs = crucible_codegen_rust::scaffold(&service, &Default::default());
    let mut paths: Vec<String> = stubs
        .iter()
        .map(|stub| stub.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "city/create_city.rs",
            "city/get_city.rs",
            "city/get_city_photo.rs",
            "city/list_cities.rs",
            "city/put_city_photo.rs",
            "service/ping.rs",
            "service/status.rs",
        ]
    );
    let get_city = stubs.iter().find(|stub| stub.module == "get_city").unwrap();
    assert!(
        get_city
            .contents
            .contains("use crate::weather::{GetCityInput, GetCityOperation, GetCityOutput};")
    );
    assert!(
        get_city
            .contents
            .contains("impl GetCityOperation for Service {")
    );
    assert!(
        get_city
            .contents
            .contains("async fn get_city(&self, input: GetCityInput) -> Result<GetCityOutput>")
    );
    let ping = stubs.iter().find(|stub| stub.module == "ping").unwrap();
    assert!(
        ping.contents
            .contains("use crate::weather::{PingOperation};"),
        "{}",
        ping.contents
    );
}

#[test]
fn scaffold_escapes_keyword_identifiers_in_modules_and_preserves_file_stems() {
    let model = r#"{
        "smithy": "2.0",
        "shapes": {
            "example#Service": {
                "type": "service",
                "version": "2026-01-01",
                "operations": [
                    { "target": "example#Type" },
                    { "target": "example#Self" }
                ],
                "resources": [
                    { "target": "example#Match" },
                    { "target": "example#Super" }
                ],
                "traits": {
                    "crucible.protocols#json": {}
                }
            },
            "example#Match": {
                "type": "resource",
                "operations": [
                    { "target": "example#Return" },
                    { "target": "example#Move" }
                ]
            },
            "example#Super": {
                "type": "resource",
                "operations": [
                    { "target": "example#Crate" }
                ]
            },
            "example#Type": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/type" } }
            },
            "example#Self": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/self" } }
            },
            "example#Return": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/return" } }
            },
            "example#Move": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/move" } }
            },
            "example#Crate": {
                "type": "operation",
                "traits": { "smithy.api#http": { "method": "GET", "uri": "/crate" } }
            }
        }
    }"#;
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let stubs = crucible_codegen_rust::scaffold(&service, &Default::default());

    let mut paths: Vec<String> = stubs
        .iter()
        .map(|stub| stub.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "match/move.rs",
            "match/return.rs",
            "service/self_.rs",
            "service/type.rs",
            "super_/crate_.rs",
        ]
    );

    let type_stub = stubs.iter().find(|stub| stub.file_stem == "type").unwrap();
    assert_eq!(type_stub.module, "r#type");
    assert_eq!(type_stub.file_stem, "type");
    assert_eq!(type_stub.folder, "service");
    assert!(
        type_stub
            .contents
            .contains("impl TypeOperation for Service {")
    );
    assert!(type_stub.contents.contains("async fn r#type(&self)"));

    let self_stub = stubs.iter().find(|stub| stub.file_stem == "self_").unwrap();
    assert_eq!(self_stub.module, "self_");
    assert_eq!(self_stub.file_stem, "self_");
    assert!(
        self_stub
            .contents
            .contains("impl SelfOperation for Service {")
    );
    assert!(self_stub.contents.contains("async fn self_(&self)"));

    let crate_stub = stubs
        .iter()
        .find(|stub| stub.file_stem == "crate_")
        .unwrap();
    assert_eq!(crate_stub.module, "crate_");
    assert_eq!(crate_stub.folder, "super_");
}

#[test]
fn aws_json_protocols_describe_their_wire_in_client_markers() {
    use crucible_smithy::Protocol;
    let model = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/protocol-json.json"
    ))
    .unwrap();
    for (protocol, marker) in [
        (Protocol::AwsRestJson1, "Protocol::AwsRestJson1;"),
        (
            Protocol::AwsJson10,
            "Protocol::AwsJson10 { service: \"JsonService\" };",
        ),
        (
            Protocol::AwsJson11,
            "Protocol::AwsJson11 { service: \"JsonService\" };",
        ),
    ] {
        let service = model
            .lower_with_protocol(
                None,
                &crucible_codegen_rust::client_capabilities(),
                protocol,
            )
            .unwrap();
        let client = generate_client(&service).unwrap();
        assert!(client.contains(marker), "{protocol:?}");
        assert!(client.contains("const IGNORE_TYPE: bool = true;"));
        assert!(client.contains("codec::NullAbsent<"));
        assert!(client.contains("codec::NonFinite"));
        assert!(
            client.contains(
                "skip_serializing_if = \"::crucible_client_runtime_rust::Field::is_unset\""
            )
        );
        // Documents keep explicit nulls.
        assert!(client.contains(
            "#[serde(rename = \"document\", default, skip_serializing_if = \"::crucible_client_runtime_rust::Field::is_absent\")]"
        ));
        let rest = protocol == Protocol::AwsRestJson1;
        assert_eq!(
            client.contains("Scalar::Timestamp { text: ::crucible_client_runtime_rust::operation::TimestampFormat::HttpDate, json: ::crucible_client_runtime_rust::operation::TimestampFormat::EpochSeconds }"),
            rest
        );
    }
}

#[test]
fn checksums_payload_kinds_and_media_headers_reach_client_markers() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/checksums.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
    .unwrap();
    let client = generate_client(&service).unwrap();
    assert!(client.contains("Some(::crucible_client_runtime_rust::operation::Checksum::Md5)"));
    assert!(client.contains("request_algorithm_member: Some(\"algorithm\"), validation_mode_member: Some(\"validation\")"));
    assert!(client.contains("ChecksumAlgorithm::Crc64Nvme"));
    let mut ast: Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/error-payloads.json"
    ))
    .unwrap();
    // Flat SDK types need distinct names for the exception and operation enum.
    ast["shapes"]["example.wire#Wire"]["rename"] = json!({"example.wire#TextError": "TextFault"});
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
        .unwrap();
    let client = generate_client(&service).unwrap();
    assert!(client.contains("Location::TextPayload(\"text/plain\")"));
    assert!(client.contains("Location::DocumentPayload(\"application/json\")"));
    assert!(
        client.contains("Scalar::List(&::crucible_client_runtime_rust::operation::Scalar::Base64)")
    );
    assert!(client.contains("Location::PrefixHeaders(\"x-extra-\"), scalar: ::crucible_client_runtime_rust::operation::Scalar::Base64"));
    // The error's payload is described too, so its body decodes as text.
    assert!(client.contains(
        "name: \"details\", location: ::crucible_client_runtime_rust::operation::Location::TextPayload"
    ));
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/error-payloads.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
    .unwrap();
    assert!(
        generate(&service, &Options::default())
            .unwrap()
            .contains("Scalar::Base64")
    );
}

#[test]
fn rpc_v2_official_models_generate_both_rust_targets() {
    for fixture in [
        include_str!("../../crucible-smithy/tests/fixtures/rpcv2-cbor.json"),
        include_str!("../../crucible-smithy/tests/fixtures/rpcv2-json.json"),
    ] {
        let service = Model::from_json(fixture)
            .unwrap()
            .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
            .unwrap();
        let server = generate(&service, &Options::default()).unwrap();
        let client = generate_client(&service).unwrap();
        assert!(server.contains("SmithyRpcV2"));
        assert!(client.contains("SmithyRpcV2"));
    }
}

#[test]
fn keyword_operation_derivatives_and_outputless_waiters_use_valid_names_and_types() {
    let model = include_str!("fixtures/keyword-waiters.json");
    let service = Model::from_json(model)
        .unwrap()
        .lower_with_capabilities(
            None,
            &crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied()),
        )
        .unwrap();
    let code = generate_client(&service).unwrap();
    assert!(code.contains("pub async fn r#type("));
    assert!(code.contains("pub async fn type_with_options_2("));
    assert!(code.contains("pub async fn wait_until_match_2("));
    assert!(
        code.contains("WaiterResult<::crucible_client_runtime_rust::operation::Unit, TypeError>")
    );
    assert!(!code.contains("wait_until_r#"));
}

#[test]
fn paginated_endpoint_metadata_matches_the_golden_file() {
    let source = include_str!("../../../e2e/weather-client/tests/fixtures/endpoints.json");
    let service = Model::from_json(source)
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
        .unwrap();
    let code = generate_client(&service).unwrap();
    assert!(code.contains(".metadata(&__crucible_metadata_Read)"));
    let pages = code
        .split("    /// Every page of Read, one request each.\n")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    matches_golden(&format!("{pages}\n    }}\n"), "paginated_endpoints.rs");
}

#[test]
fn finite_only_float_members_use_checked_codec() {
    let service = edited(|model| {
        model["shapes"]["example.weather#Coordinates"]["members"]["latitude"]["target"] =
            json!("smithy.api#Float");
    });
    let server = generate(&service, &Options::default()).unwrap();
    let client = generate_client(&service).unwrap();
    assert!(server.contains("Adapter::<::crucible_server_runtime_rust::codec::FiniteFloat>"));
    assert!(client.contains("Adapter::<::crucible_client_runtime_rust::codec::FiniteFloat>"));
    let adapters = [&server, &client]
        .into_iter()
        .flat_map(|code| code.lines())
        .filter(|line| line.contains("codec::FiniteFloat"))
        .map(|line| format!("{}\n", line.trim()))
        .collect::<String>();
    matches_golden(&adapters, "finite_float.rs");
}
