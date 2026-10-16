use crucible_codegen_rust::{Options, ScaffoldOptions, generate, scaffold};
use crucible_ir::ShapeId;

mod common;

#[test]
fn server_namespace_exposes_models_and_exceptions_directly() {
    let service = common::load("weather");
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
    let mut service = common::load("weather");
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
        let mut service = common::load("weather");
        common::rename(&mut service, &[("example.weather#GetCityInput", name)]);
        let diagnostics = generate(&service, &Options::default())
            .unwrap_err()
            .to_string();
        assert!(
            diagnostics.contains(&format!("example.weather#{name}")),
            "{diagnostics}"
        );
        assert!(diagnostics.contains("collides"), "{diagnostics}");
    }
}

#[test]
fn custom_scaffold_paths_and_result_model_names_are_supported() {
    let mut service = common::load("weather");
    common::rename(&mut service, &[("example.weather#GetCityInput", "Result")]);
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
    let mut service = common::load("weather");
    common::rename(
        &mut service,
        &[
            ("example.weather#GetCityInput", "Result"),
            ("example.weather#GetCityOutput", "ServerResult"),
        ],
    );
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

/// The weather service with `ListCities` as a POST whose pagination token is
/// `token`, a map or a string with a length limit.
fn paginated(token: &str) -> String {
    let service = common::load_with("weather", |package| {
        let list = package
            .files
            .iter_mut()
            .find(|file| file.text.starts_with("operation ListCities"))
            .unwrap();
        list.text = format!(
            "operation ListCities lists City {{\n    http POST \"/cities/search\"\n    paginated(items: cities, pageSize: maxResults)\n    input {{\n        nextToken?: {token}\n        maxResults?: integer {{ range 1..100 }}\n    }}\n    output {{\n        cities: CitySummaries\n        nextToken?: {token}\n    }}\n}}\n"
        );
        package.files.push(crucible_lang::SourceFile {
            path: "token.cru".into(),
            text: "map TokenMap: string -> string\n".into(),
        });
    });
    let code = generate(&service, &Options::default()).unwrap();
    let start = code.find("Pagination { input_token").unwrap();
    code[start..start + code[start..].find('\n').unwrap()].to_owned()
}

#[test]
fn pagination_says_whether_the_input_token_is_a_map() {
    // A server extension that seals string tokens refuses a map token (a
    // composite database key) at build.
    assert!(paginated("string").contains("input_token_map: false"));
    assert!(paginated("TokenMap").contains("input_token_map: true"));
}

#[test]
fn pagination_carries_the_input_tokens_length_limit() {
    // A server extension that issues tokens checks at build that the longest
    // fits the member's `length`.
    assert!(paginated("string").contains("input_token_max_length: ::std::option::Option::None"));
    assert!(
        paginated("string { length ..2048 }")
            .contains("input_token_max_length: ::std::option::Option::Some(2048)")
    );
}
