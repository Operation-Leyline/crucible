use crucible_codegen_rust::{Options, ScaffoldOptions, generate, scaffold};
use crucible_smithy::{Model, ShapeId};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

#[test]
fn server_namespace_exposes_models_and_exceptions_directly() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("pub mod weather {"));
    assert!(code.contains("pub use model::{"));
    assert!(code.contains("pub use errors::{"));
    assert!(!code.contains("pub mod model"));
    assert!(!code.contains("pub mod errors"));
    assert!(code.contains("fn get_city(&self, input: GetCityInput)"));
    assert!(code.contains("Result<GetCityOutput>"));
}

#[test]
fn service_module_escapes_keywords_and_acronyms() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for (name, module) in [
        ("APIService", "api_service"),
        ("type", "r#type"),
        ("Self", "self_"),
    ] {
        service.id = ShapeId::new("example.weather", name);
        let code = generate(&service, &Options::default()).unwrap();
        assert!(code.contains(&format!("pub mod {module} {{")));
        assert!(
            scaffold(&service, &ScaffoldOptions::default())[0]
                .contents
                .contains(&format!("use crate::{module}::"))
        );
    }
}

#[test]
fn flattened_names_diagnose_collisions_before_emitting_code() {
    for name in [
        "GetCity",
        "GetCityOperation",
        "OperationsBuilder",
        "model",
        "errors",
    ] {
        let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
        ast["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#GetCityInput": name});
        let service = Model::from_json(&ast.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = generate(&service, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(
            diagnostics.contains("example.weather#GetCityInput"),
            "{diagnostics}"
        );
        assert!(diagnostics.contains("collides"), "{diagnostics}");
    }
}

#[test]
fn custom_scaffold_paths_and_result_model_names_are_supported() {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    ast["shapes"]["example.weather#Weather"]["rename"] =
        json!({"example.weather#GetCityInput": "Result"});
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let stubs = scaffold(
        &service,
        &ScaffoldOptions {
            contract: Some("shared_contract::weather".into()),
            ..Default::default()
        },
    );
    let stub = stubs
        .iter()
        .find(|stub| stub.file_stem == "get_city")
        .unwrap();
    assert!(
        stub.contents
            .contains("use shared_contract::weather::{GetCityOperation, GetCityOutput, Result};")
    );
    assert!(
        stub.contents
            .contains("use crucible_server_runtime_rust::Result as ServerResult;")
    );
    assert!(
        stub.contents
            .contains("input: Result) -> ServerResult<GetCityOutput>")
    );
}

#[test]
fn scaffold_aliases_all_conflicting_runtime_and_implementation_imports() {
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    ast["shapes"]["example.weather#Weather"]["rename"] = json!({
        "example.weather#GetCityInput": "Result",
        "example.weather#GetCityOutput": "ServerResult",
    });
    let service = Model::from_json(&ast.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let stubs = scaffold(
        &service,
        &ScaffoldOptions {
            service_type: "crate::operations::Result".into(),
            ..Default::default()
        },
    );
    let stub = stubs
        .iter()
        .find(|stub| stub.file_stem == "get_city")
        .unwrap();
    assert!(
        stub.contents
            .contains("use crate::operations::Result as ServiceImplementation;")
    );
    assert!(stub.contents.contains("Result as ServerResult_2;"));
    assert!(
        stub.contents
            .contains("impl GetCityOperation for ServiceImplementation")
    );
    assert!(
        stub.contents
            .contains("input: Result) -> ServerResult_2<ServerResult>")
    );
}

#[test]
fn pagination_says_whether_the_input_token_is_a_map() {
    // Smithy allows a map token (a composite database key); a server
    // extension that seals string tokens refuses one at build.
    let marker = |ast: &Value| {
        let service = Model::from_json(&ast.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let code = generate(&service, &Options::default()).unwrap();
        let start = code.find("Pagination { input_token").unwrap();
        code[start..start + code[start..].find('\n').unwrap()].to_owned()
    };
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    assert!(marker(&ast).contains("input_token_map: false"));

    let shapes = &mut ast["shapes"];
    shapes["example.weather#TokenMap"] = json!({
        "type": "map",
        "key": {"target": "smithy.api#String"},
        "value": {"target": "smithy.api#String"}
    });
    let list = &mut shapes["example.weather#ListCities"]["traits"];
    list["smithy.api#http"] = json!({"method": "POST", "uri": "/cities/search"});
    list.as_object_mut().unwrap().remove("smithy.api#examples");
    shapes["example.weather#ListCitiesInput"]["members"]["nextToken"] =
        json!({"target": "example.weather#TokenMap"});
    shapes["example.weather#ListCitiesOutput"]["members"]["nextToken"] =
        json!({"target": "example.weather#TokenMap"});
    assert!(marker(&ast).contains("input_token_map: true"));
}

#[test]
fn pagination_carries_the_input_tokens_length_limit() {
    // A server extension that issues tokens checks at build that the longest
    // fits the member's `@length`.
    let marker = |ast: &Value| {
        let service = Model::from_json(&ast.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let code = generate(&service, &Options::default()).unwrap();
        let start = code.find("Pagination { input_token").unwrap();
        code[start..start + code[start..].find('\n').unwrap()].to_owned()
    };
    let mut ast: Value = serde_json::from_str(WEATHER).unwrap();
    assert!(marker(&ast).contains("input_token_max_length: ::std::option::Option::None"));
    ast["shapes"]["example.weather#ListCitiesInput"]["members"]["nextToken"]["traits"]["smithy.api#length"] =
        json!({"max": 2048});
    assert!(marker(&ast).contains("input_token_max_length: ::std::option::Option::Some(2048)"));
}
