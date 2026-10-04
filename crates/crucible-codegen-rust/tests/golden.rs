//! The weather fixture's generated code, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate};
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather.rs");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &code).unwrap();
    }
    let golden = fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        golden == code,
        "generated code differs from tests/golden/weather.rs; review it and rerun with CRUCIBLE_BLESS=1"
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
        internal_error: None,
    };
    let diagnostics = generate(&service, &options).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("on_rejection")),
        "{diagnostics}"
    );
}
