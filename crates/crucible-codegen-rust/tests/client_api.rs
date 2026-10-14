use crucible_codegen_rust::{ClientOptions, emit_client_package, generate_client};
use crucible_smithy::Model;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

#[test]
fn sdk_exposes_models_and_exceptions_directly() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate_client(&service).unwrap();
    assert!(code.contains("pub use model::{"));
    assert!(code.contains("pub use errors::{"));
    assert!(!code.contains("pub mod model"));
    assert!(!code.contains("pub mod errors"));
    assert!(code.contains("GetCityInput, GetCityOutput"));
    assert!(code.contains("ConflictError, NotFoundError"));
    assert!(code.contains("pub async fn get_city(&self, input: GetCityInput)"));
    assert!(code.contains(
        "Result<GetCityOutput, ::crucible_client_runtime_rust::__generated::Error<GetCityError>>"
    ));
}

#[test]
fn flattened_names_diagnose_collisions_before_emitting_code() {
    for name in [
        "GetCity",
        "GetCityError",
        "WeatherClient",
        "WeatherException",
        "model",
        "errors",
        "runtime",
        "GET_CITY_OPERATION",
        "__redaction",
    ] {
        let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
        ast["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#GetCityInput": name});
        let service = Model::from_json(&ast.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = generate_client(&service).unwrap_err().to_string();
        assert!(
            diagnostics.contains("example.weather#GetCityInput"),
            "{diagnostics}"
        );
        assert!(diagnostics.contains("collides"), "{diagnostics}");
        assert!(emit_client_package(&service, &ClientOptions::default()).is_err());
    }
}

#[test]
fn modeled_errors_colliding_with_operation_errors_are_diagnosed() {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    ast["shapes"]["example.weather#Weather"]["rename"] =
        json!({"example.weather#NotFoundError": "GetCityError"});
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let diagnostics = generate_client(&service).unwrap_err().to_string();
    assert!(
        diagnostics.contains("example.weather#NotFoundError"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("collides"), "{diagnostics}");
}

#[test]
fn flat_exports_are_deterministic() {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let expected = generate_client(&service).unwrap();
    let shapes = ast["shapes"].as_object_mut().unwrap();
    let reversed: Vec<_> = shapes.clone().into_iter().rev().collect();
    shapes.clear();
    shapes.extend(reversed);
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(expected, generate_client(&service).unwrap());
}

#[test]
fn sensitive_enum_debug_does_not_rewrite_documentation() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    for (name, kind, value) in [
        ("SecretEnum", "enum", json!("known")),
        ("SecretIntEnum", "intEnum", json!(1)),
    ] {
        let id = format!("example.weather#{name}");
        model["shapes"][&id] = json!({
            "type": kind,
            "traits": {"smithy.api#sensitive": {}, "smithy.api#documentation": "Debug, for example #[derive(Debug, Clone)]"},
            "members": {"Known": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": value, "smithy.api#documentation": "Debug, member documentation"}}}
        });
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] = json!({"target": id});
    }
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::client_capabilities())
        .unwrap();
    let code = generate_client(&service).unwrap();
    assert_eq!(
        // Model docs are escaped for rustdoc (a bare `[x]` is no link), and
        // otherwise left as written.
        code.matches(r"/// Debug, for example #\[derive(Debug, Clone)]")
            .count(),
        2
    );
    assert_eq!(code.matches("/// Debug, member documentation").count(), 2);
    for name in ["SecretEnum", "SecretIntEnum"] {
        let declaration = format!("pub enum {name} {{");
        let lines: Vec<_> = code.lines().map(str::trim).collect();
        let index = lines.iter().position(|line| *line == declaration).unwrap();
        assert_eq!(lines[index - 1], "#[non_exhaustive]");
        assert!(lines[index - 2].starts_with("#[derive(Clone,"));
        assert!(code.contains(&format!("impl ::std::fmt::Debug for {name}")));
    }
}
