//! The weather fixture's generated code, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn edited(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
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
            "name: \"photo\", location: ::crucible::operation::Location::Payload(\"image/jpeg\"), scalar: ::crucible::operation::Scalar::Json, redaction: &::crucible::operation::Redaction::Sensitive"
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
    let options = Options {
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

#[test]
fn a_configured_validation_error_crucible_cannot_fill_fails_generation() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ConflictError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    let options = Options {
        validation_error: Some("example.weather#ConflictError".parse().unwrap()),
        ..Options::default()
    };
    let diagnostics = generate(&service, &options).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("on_rejection")),
        "{diagnostics}"
    );
}

#[test]
fn a_paginated_operation_fills_a_fresh_token_per_page_unless_the_caller_set_one() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ListCitiesInput"]["members"]["clientToken"] = json!({ "target": "smithy.api#String", "traits": { "smithy.api#idempotencyToken": {} } });
    });
    let code = generate_client(&service).unwrap();
    assert!(
        code.contains("let fresh = input.client_token.is_none();"),
        "{code}"
    );
    assert!(code.contains(
        "pages.fresh_token(|input| input.client_token = Some(::crucible_client::idempotency_token()))"
    ));
}

#[test]
fn strict_structures_refuse_unknown_members_on_the_server_only() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let strict = "#[serde(deny_unknown_fields)]\n    pub struct ";
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
            .contains("use crate::contract::{model, GetCityOperation};")
    );
    assert!(
        get_city
            .contents
            .contains("impl GetCityOperation for Service {")
    );
    assert!(get_city.contents.contains(
        "async fn get_city(&self, input: model::GetCityInput) -> crucible::Result<model::GetCityOutput>"
    ));
    let ping = stubs.iter().find(|stub| stub.module == "ping").unwrap();
    assert!(
        ping.contents
            .contains("use crate::contract::PingOperation;"),
        "{}",
        ping.contents
    );
}
