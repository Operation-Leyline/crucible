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
        if [
            "src/runtime.ts",
            "src/query.ts",
            "src/endpoints.ts",
            "src/waiters.ts",
            "src/credentials.ts",
            "src/checksums.ts",
            "src/events.ts",
        ]
        .contains(&file.path.as_str())
        {
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

#[test]
fn schemas_preserve_ir_nullability_and_exact_long_defaults() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CitySummary"]["members"]["population"]["traits"]["smithy.api#default"] =
        serde_json::json!(i64::MAX);
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(
        &service,
        &Options {
            package_name: "weather".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
        },
    )
    .unwrap();
    let schemas = &files
        .iter()
        .find(|f| f.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(schemas.contains("population: parseJson(\"9223372036854775807\")"));
    let city = schemas
        .lines()
        .find(|line| line.starts_with("export const S_CitySummary:"))
        .unwrap();
    assert!(city.ends_with("[\"id\", \"name\", \"population\"]);"));
    let input = schemas
        .lines()
        .find(|line| line.starts_with("export const S_CreateCityInput:"))
        .unwrap();
    assert!(input.ends_with("[]);"));
    let models = &files
        .iter()
        .find(|f| f.path == "src/models.ts")
        .unwrap()
        .contents;
    assert!(models.contains("population: bigint;"));
    assert!(models.contains("expectedRevision?: bigint;"));
}

#[test]
fn prototype_named_members_defaults_and_errors_are_own_metadata() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CitySummary"]["members"]["__proto__"] = serde_json::json!({
        "target": "smithy.api#Long", "traits": { "smithy.api#default": 3 }
    });
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({ "example.weather#ConflictError": "__proto__" });
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(
        &service,
        &Options {
            package_name: "weather".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
        },
    )
    .unwrap();
    let schemas = &files
        .iter()
        .find(|f| f.path == "src/schemas.ts")
        .unwrap()
        .contents;
    assert!(schemas.contains("[\"__proto__\"]: integer(64)"));
    assert!(schemas.contains("[\"__proto__\"]: parseJson(\"3\")"));
    assert!(schemas.contains("ConflictError: { status: 409"));
    assert!(schemas.contains("new e.__proto__(fields, metadata)"));
}

#[test]
fn generated_hardening_fixture_preserves_names_auth_and_client_defaults() {
    let service = Model::from_json(include_str!("fixtures/hardening.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(
        &service,
        &Options {
            package_name: "hardening".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
        },
    )
    .unwrap();
    let source = |path| {
        &files
            .iter()
            .find(|file| file.path == path)
            .unwrap()
            .contents
    };
    let client = source("src/client.ts");
    for method in [
        "constructor_3",
        "constructor_2",
        "get",
        "get_2",
        "paginateListThings",
        "paginateListThings_2",
    ] {
        assert!(client.contains(&format!("  {method}(")), "{client}");
    }
    assert!(client.contains("return paginate(page => this.listThings(page, options)"));
    let schemas = source("src/schemas.ts");
    let imports = schemas
        .lines()
        .find(|line| line.starts_with("import {"))
        .unwrap();
    for helper in ["boolean", "document", "number", "string"] {
        assert!(imports.contains(helper), "{imports}");
    }
    assert!(schemas.contains("name: \"Public\",\n  method: \"GET\",\n  bearer: false"));
    assert!(schemas.contains("name: \"Protected\",\n  method: \"GET\",\n  bearer: true"));
    let defaults = schemas
        .lines()
        .find(|line| line.starts_with("export const S_Defaults:"))
        .unwrap();
    assert!(defaults.contains("ordinary: parseJson(\"7\")"));
    assert!(!defaults.contains("optional: parseJson"));
    let input = schemas
        .lines()
        .find(|line| line.starts_with("export const S_DefaultInput:"))
        .unwrap();
    assert!(!input.contains("parseJson"));

    // Generated method allocation must not depend on an input model's order.
    let mut reordered = service.clone();
    reordered.operations.reverse();
    let reordered_files = emit(
        &reordered,
        &Options {
            package_name: "hardening".into(),
            package_version: "0.0.0".into(),
            client_name: None,
            endpoint: None,
            middleware: vec![],
            package_json: None,
        },
    )
    .unwrap();
    let reversed_client = &reordered_files
        .iter()
        .find(|file| file.path == "src/client.ts")
        .unwrap()
        .contents;
    for method in [
        "constructor_3",
        "constructor_2",
        "get",
        "get_2",
        "paginateListThings",
        "paginateListThings_2",
    ] {
        assert!(reversed_client.contains(&format!("  {method}(")));
    }
}

#[test]
fn packaged_provenance_tracks_the_shared_wire_schema() {
    use sha2::{Digest, Sha256};
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let options = Options {
        package_name: "weather".into(),
        package_version: "1".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
    };
    let files = emit(&service, &options).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "crucible.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let schema =
        serde_json::to_string(&crucible_codegen_common::schema(&service, str::to_owned)).unwrap();
    assert_eq!(
        manifest["schemaSha256"],
        format!("{:x}", Sha256::digest(schema.as_bytes()))
    );
    let package: serde_json::Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "package.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert!(
        package["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "crucible.json")
    );
    let mut changed = service.clone();
    changed.version.push_str("-next");
    let updated = emit(&changed, &options).unwrap();
    assert_ne!(
        files.iter().find(|f| f.path == "crucible.json"),
        updated.iter().find(|f| f.path == "crucible.json")
    );
}

fn options(name: &str) -> Options {
    Options {
        package_name: name.into(),
        package_version: "0.0.0".into(),
        client_name: None,
        endpoint: None,
        middleware: vec![],
        package_json: None,
    }
}

fn file<'a>(files: &'a [crucible_codegen_ts::File], path: &str) -> &'a str {
    &files.iter().find(|f| f.path == path).unwrap().contents
}

/// A model may name its shapes after the globals generated code uses
/// (`Record`, `Partial`, `Uint8Array`, `Object`): the code reaches every
/// global through `globalThis` instead. `tools/test-ts.sh` compiles the result.
#[test]
fn shapes_named_after_globals_do_not_shadow_them() {
    let service = Model::from_json(include_str!("fixtures/globals.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options("globals")).unwrap();
    let models = file(&files, "src/models.ts");
    assert!(models.contains("export interface Record {"), "{models}");
    assert!(models.contains("export type Map = globalThis.Record<string, Record>;"));
    assert!(
        models.contains(
            "export type ColorMap = globalThis.Partial<globalThis.Record<Color, Partial>>;"
        )
    );
    assert!(models.contains("bytes?: globalThis.Uint8Array;"));
    assert!(models.contains("buffer?: Uint8Array;"));
    let errors = file(&files, "src/errors.ts");
    assert!(errors.contains("export class Object extends GlobalsException"));
    assert!(errors.contains("fields: globalThis.Record<string, unknown>"));
    assert!(errors.contains(
        "if (globalThis.Object.hasOwn(fields, \"data\")) globalThis.Object.defineProperty("
    ));
    assert!(!errors.contains(" Object.") && !errors.contains("(Object."));
}

/// `globalThis` itself, the generated namespaces and reserved words cannot
/// be declared; each is a diagnostic rather than a client that fails to compile.
#[test]
fn reserved_names_are_diagnostics() {
    for name in ["globalThis", "m", "class"] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({ "example.weather#CitySummary": name });
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(&service, &options("weather")).unwrap_err().to_string();
        assert!(
            diagnostics.contains(&format!("`{name}` is reserved")),
            "{diagnostics}"
        );
    }
}

#[test]
fn checksum_operations_carry_their_lowered_description() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/checksums.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_ts::capabilities())
    .unwrap();
    let files = emit(&service, &options("checksums")).unwrap();
    let schemas = file(&files, "src/schemas.ts");
    assert!(
        schemas.contains("  checksum: {\"kind\":\"md5\"},\n"),
        "{schemas}"
    );
    assert!(schemas.contains("  checksum: {\"kind\":\"awsFlexible\",\"requestRequired\":true,\"requestAlgorithms\":[],\"responseAlgorithms\":[]},\n"));
    assert!(schemas.contains("\"requestAlgorithmMember\":\"algorithm\""));
    assert!(schemas.contains("\"validationModeMember\":\"validation\""));
    assert!(files.iter().any(|f| f.path == "src/checksums.ts"));
    let index = file(&files, "src/index.ts");
    assert!(index.contains("export { ChecksumError } from \"./checksums.js\";"));
    assert!(index.contains("export type { ChecksumPolicy } from \"./checksums.js\";"));
}

#[test]
fn configured_client_cannot_shadow_the_service_exception() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let mut options = options("weather");
    options.client_name = Some("WeatherException".into());
    assert!(
        emit(&service, &options)
            .unwrap_err()
            .to_string()
            .contains("client name collides")
    );
}
