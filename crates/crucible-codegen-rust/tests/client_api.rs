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
    assert!(
        code.contains("Result<GetCityOutput, ::crucible_client_runtime_rust::Error<GetCityError>>")
    );
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
