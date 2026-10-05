use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen_cli::{Options, emit};
use crucible_smithy::Model;
use crucible_smithy::ir::Service;
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn options() -> Options {
    Options {
        bin_name: "weather".into(),
        package_version: "0.1.0".into(),
        endpoint: Some("http://localhost:8080".into()),
        runtime_path: PathBuf::from("../crucible"),
    }
}

fn edited(edit: impl FnOnce(&mut Value)) -> Service {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    edit(&mut model);
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn weather_matches_golden_package() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for file in emit(&service, &options()).unwrap() {
        // SDK output has its own authoritative golden in codegen-rust.
        if file.path == "src/client.rs" {
            assert_eq!(
                file.contents,
                crucible_codegen_rust::generate_client(&service).unwrap()
            );
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/weather")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        let golden = fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(
            golden, file.contents,
            "{}; review output before CRUCIBLE_BLESS=1",
            file.path
        );
    }
}

#[test]
fn collects_command_flag_and_sdk_diagnostics() {
    let service = edited(|model| {
        let shapes = &mut model["shapes"];
        shapes["example.weather#GetCityInput"]["members"]["inputJSON"] =
            json!({"target":"smithy.api#String"});
        shapes["example.weather#GetCityInput"]["members"]["cityID"] =
            json!({"target":"smithy.api#String"});
        shapes["example.weather#CityId"]["traits"]["smithy.api#pattern"] = json!("^(?!x)c-.*$");
    });
    let message = emit(&service, &options()).unwrap_err().to_string();
    assert!(
        message.contains("flag `--input-json` is reserved"),
        "{message}"
    );
    assert!(message.contains("flag `--city-id` collides"), "{message}");
    assert!(message.contains("does not compile"), "{message}");
}

#[test]
fn command_collisions_are_reported_against_operation() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let mut duplicate = service.operations[0].clone();
    duplicate.id = "example.weather#Create_City".parse().unwrap();
    service.operations.push(duplicate);
    let errors = emit(&service, &options()).unwrap_err().to_string();
    assert!(
        errors.contains("CLI command `create-city` collides"),
        "{errors}"
    );
}

#[test]
fn rejects_invalid_package_metadata_without_emitting_files() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let options = Options {
        bin_name: "../weather\n[dependencies]".into(),
        package_version: "1.0\n".into(),
        runtime_path: PathBuf::new(),
        ..options()
    };
    let errors = emit(&service, &options).unwrap_err();
    assert_eq!(errors.len(), 3);
}

#[test]
fn descriptor_uses_client_presence_and_timestamp_semantics() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_array()
        .unwrap();
    for member in members {
        assert_eq!(member["required"], false, "@input uses client optionality");
    }
    let coordinate_members = schema["shapes"]["example.weather#Coordinates"]["members"]
        .as_array()
        .unwrap();
    assert!(
        coordinate_members
            .iter()
            .all(|member| member["required"] == true)
    );
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(code.contains("client.list_cities_pages(input)"));
    assert!(code.contains("Output::Jsonl"));
    assert!(!code.contains("from_value(invocation.input).map_err(Error::input)"));
}

#[test]
fn emission_does_not_depend_on_operation_order() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let expected = emit(&service, &options()).unwrap();
    service.operations.reverse();
    let actual = emit(&service, &options()).unwrap();
    for (expected, actual) in expected.iter().zip(actual.iter()) {
        // The SDK assumes lowering's sorted input; the CLI owns only its files.
        if expected.path != "src/client.rs" {
            assert_eq!(expected, actual);
        }
    }
}

#[test]
fn timestamp_arguments_use_the_sdk_member_format() {
    let service = edited(|model| {
        model["shapes"]["example.weather#CreateCityInput"]["members"]["observedAt"] = json!({
            "target": "smithy.api#Timestamp",
            "traits": {"smithy.api#timestampFormat": "epoch-seconds"}
        });
        model["shapes"]["example.weather#CreateCityInput"]["members"]["sentAt"] = json!({
            "target": "smithy.api#Timestamp"
        });
    });
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_array()
        .unwrap();
    assert_eq!(
        members
            .iter()
            .find(|member| member["name"] == "observedAt")
            .unwrap()["kind"],
        "number"
    );
    assert_eq!(
        members
            .iter()
            .find(|member| member["name"] == "sentAt")
            .unwrap()["kind"],
        "string"
    );
}

#[test]
fn rejects_windows_device_names_for_portable_packages() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for name in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
        let options = Options {
            bin_name: name.into(),
            ..options()
        };
        let errors = emit(&service, &options).unwrap_err().to_string();
        assert!(
            errors.contains("Windows device names are reserved"),
            "{name}: {errors}"
        );
    }
    for name in ["console", "com10", "lpt0"] {
        let options = Options {
            bin_name: name.into(),
            ..options()
        };
        assert!(emit(&service, &options).is_ok(), "{name}");
    }
}

#[test]
fn empty_services_do_not_emit_unused_dispatch_bindings() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    service.operations.clear();
    let files = emit(&service, &options()).unwrap();
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(!code.contains("write_json"));
    assert!(!code.contains("let client ="));
    assert!(!code.contains("match invocation"));
    assert!(code.contains("service has no operations"));
}

#[test]
fn descriptor_preserves_root_shape_sensitivity() {
    let service = edited(|model| {
        model["shapes"]["example.weather#CreateCityInput"]["traits"]["smithy.api#sensitive"] =
            json!({});
    });
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        schema["shapes"]["example.weather#CreateCityInput"]["sensitive"],
        true
    );
    assert_eq!(
        schema["shapes"]["example.weather#GetCityInput"]["sensitive"],
        false
    );
}

#[test]
fn input_version_is_a_member_flag_separate_from_root_version() {
    let service = edited(|model| {
        model["shapes"]["example.weather#GetCityInput"]["members"]["version"] = json!({
            "target": "smithy.api#String"
        });
    });
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#GetCityInput"]["members"]
        .as_array()
        .unwrap();
    let version = members
        .iter()
        .find(|member| member["name"] == "version")
        .unwrap();
    assert_eq!(version["flag"], "version");
    assert_eq!(schema["version"], "0.1.0");
}

#[test]
fn unit_responses_do_not_bind_unit_values() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let files = emit(&service, &options()).unwrap();
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(code.contains("client.r#ping().await.map_err(Error::request)?;"));
    assert!(!code.contains("let output = client.r#ping()"));
    assert!(code.contains("write_json(&())"));
}
