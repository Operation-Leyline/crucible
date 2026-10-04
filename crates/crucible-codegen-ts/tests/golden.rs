//! The weather fixture's generated client, checked in, so every change to the
//! output shows in review. `CRUCIBLE_BLESS=1 cargo test` rewrites it.
//! `src/runtime.ts` is a verbatim copy of `runtime/runtime.ts` and is left out.

use std::fs;
use std::path::Path;

use crucible_codegen_ts::{Middleware, Options, emit};
use crucible_smithy::Model;

#[test]
fn weather_client_matches_golden_files() {
    let model = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
    let service = Model::from_json(model).unwrap().lower(None).unwrap();
    let options = Options {
        package_name: "@example/weather".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: Some("https://weather.example".into()),
        middleware: vec![Middleware::ContentSha256],
        package_json: None,
    };
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/weather");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut stale = Vec::new();
    for file in emit(&service, &options).unwrap() {
        if file.path == "src/runtime.ts" {
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
        "generated output differs from tests/golden/weather in {stale:?}; \
         review the change and rerun with CRUCIBLE_BLESS=1"
    );
}

/// A model may name a shape `Date` (a calendar date, say). Timestamps are
/// the global `Date`, so they must not resolve to that shape.
#[test]
fn a_shape_named_date_does_not_shadow_timestamps() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let shapes = model["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "example.weather#Date".into(),
        serde_json::json!({ "type": "string" }),
    );
    shapes["example.weather#CitySummary"]["members"]["foundedOn"] =
        serde_json::json!({ "target": "example.weather#Date" });
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let options = Options {
        package_name: "@example/weather".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
    };
    let files = emit(&service, &options).unwrap();
    let models = &files
        .iter()
        .find(|file| file.path == "src/models.ts")
        .unwrap()
        .contents;
    assert!(models.contains("export type Date = string;"), "{models}");
    assert!(
        models.contains("export type DateTime = globalThis.Date;"),
        "{models}"
    );
}
