//! The weather fixture's generated server, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.
//! `src/runtime.rs` is a verbatim copy of `runtime/server.rs` and is left out.

use std::fs;
use std::path::Path;

use crucible_core::Model;
use crucible_emit_rust::{Options, emit_server};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-core/tests/fixtures/weather.json");

fn options() -> Options {
    Options {
        crate_name: "weather-api".into(),
        crate_version: "0.0.0".into(),
        validation_error: None,
    }
}

fn edited(edit: impl FnOnce(&mut Value)) -> crucible_core::ir::Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn weather_server_matches_golden_files() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather-server");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut stale = Vec::new();
    for file in emit_server(&service, &options()).unwrap() {
        if file.path == "src/runtime.rs" {
            continue;
        }
        let path = golden.join(&file.path);
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        } else if fs::read_to_string(&path)
            .map(|s| s.replace("\r\n", "\n"))
            .ok()
            != Some(file.contents)
        {
            stale.push(file.path);
        }
    }
    assert!(
        stale.is_empty(),
        "generated output differs from tests/golden/weather-server in {stale:?}; \
         review the change and rerun with CRUCIBLE_BLESS=1"
    );
}

#[test]
fn a_required_reason_leaves_reject_to_the_service() {
    let service = edited(|model| {
        model["shapes"]["example.weather#ValidationError"]["members"]["reason"] =
            json!({ "target": "smithy.api#String", "traits": { "smithy.api#required": {} } });
    });
    let files = emit_server(&service, &options()).unwrap();
    let api = &files
        .iter()
        .find(|f| f.path == "src/api.rs")
        .unwrap()
        .contents;
    assert!(
        api.contains("fn reject(&self, rejection: Rejection) -> crate::errors::ValidationError;")
    );
}

#[test]
fn a_pattern_the_rust_regex_crate_cannot_run_is_a_diagnostic() {
    let service = edited(|model| {
        model["shapes"]["example.weather#CityId"]["traits"]["smithy.api#pattern"] =
            json!("^(?!x)c-.*$");
    });
    let diagnostics = emit_server(&service, &options()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("does not compile")),
        "{diagnostics}"
    );
}

#[test]
fn paths_differing_only_in_label_names_are_a_diagnostic() {
    // axum panics at startup on `/cities/{cityId}` beside `/cities/{id}`.
    let service = edited(|model| {
        let shapes = &mut model["shapes"];
        shapes["example.weather#Weather"]["operations"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "target": "example.weather#DeleteCity" }));
        shapes["example.weather#DeleteCity"] = json!({
            "type": "operation",
            "input": { "target": "example.weather#DeleteCityInput" },
            "traits": { "smithy.api#http": { "method": "DELETE", "uri": "/cities/{id}" } }
        });
        shapes["example.weather#DeleteCityInput"] = json!({
            "type": "structure",
            "members": { "id": { "target": "smithy.api#String", "traits": {
                "smithy.api#required": {}, "smithy.api#httpLabel": {} } } },
            "traits": { "smithy.api#input": {} }
        });
    });
    let diagnostics = emit_server(&service, &options()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("differ only in label names")),
        "{diagnostics}"
    );
}
