use crucible_codegen_python::{Options, emit};
use crucible_smithy::Model;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn options() -> Options {
    Options {
        package_name: "weather-client".into(),
        module_name: "weather_client".into(),
        package_version: "0.0.0.dev0".into(),
        endpoint: Some("https://weather.example".into()),
        content_sha256: true,
    }
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    for file in emit(&service, &options()).unwrap() {
        if [
            "runtime.py",
            "auth.py",
            "query.py",
            "waiters.py",
            "endpoints.py",
            "checksums.py",
            "streams.py",
            "policy.py",
            "discovery.py",
            "telemetry.py",
        ]
        .iter()
        .any(|name| file.path.ends_with(name))
        {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}

#[test]
fn runtime_nullability_is_taken_from_ir() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let files = emit(&service, &options()).unwrap();
    let models = &files
        .iter()
        .find(|file| file.path.ends_with("/models.py"))
        .unwrap()
        .contents;
    let literal = models
        .lines()
        .find_map(|line| line.strip_prefix("_SCHEMA = json.loads("))
        .unwrap()
        .strip_suffix(')')
        .unwrap();
    let text: String = serde_json::from_str(literal).unwrap();
    let schema: Value = serde_json::from_str(&text).unwrap();
    let city = &schema["shapes"]["example.weather#CitySummary"]["members"];
    assert_eq!(city["id"]["optional"], false);
    assert_eq!(city["population"]["optional"], false);
    assert_eq!(city["updatedAt"]["optional"], true);
    assert_eq!(
        schema["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["optional"],
        true
    );
}

#[test]
fn naming_collisions_are_collected_before_emission() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let members = &mut model["shapes"]["example.weather#CreateCityInput"]["members"];
    members["fooBar"] = json!({"target":"smithy.api#String"});
    members["foo_bar"] = json!({"target":"smithy.api#String"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut config = options();
    config.module_name = "../escape".into();
    let errors = emit(&service, &config).err().expect("invalid names fail");
    assert_eq!(errors.len(), 2, "{errors}");
}

#[test]
fn reversed_shape_order_does_not_change_output() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let shapes = model["shapes"].as_object_mut().unwrap();
    let reversed = shapes
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    *shapes = reversed;
    let a = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let b = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = |service| {
        emit(service, &options())
            .unwrap()
            .into_iter()
            .map(|f| (f.path, f.contents))
            .collect::<Vec<_>>()
    };
    assert_eq!(files(&a), files(&b));
}

#[test]
fn unit_only_service_has_valid_empty_exports() {
    let model = json!({"smithy":"2.0", "shapes": {
        "test#Api": {"type":"service", "version":"1", "operations":[{"target":"test#Ping"}],
            "traits":{"crucible.protocols#json":{}}},
        "test#Ping": {"type":"operation", "traits":{"smithy.api#http":{"method":"GET","uri":"/","code":200}}}
    }});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let init = files
        .iter()
        .find(|f| f.path.ends_with("__init__.py"))
        .unwrap();
    assert!(!init.contents.contains("from .models import"));
    assert!(
        init.contents
            .contains("from .client import ApiClient, AsyncApiClient")
    );
}

#[test]
fn service_client_export_collisions_are_diagnosed() {
    for name in ["WeatherClient", "AsyncWeatherClient"] {
        let mut model: Value = serde_json::from_str(WEATHER).unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#CitySummary":name});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        assert!(emit(&service, &options()).is_err(), "{name}");
    }
}

#[test]
fn documentation_and_formatting_use_the_lowered_contract() {
    let service = Model::from_json(include_str!("../../../tests/fixtures/client-docs.json"))
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_python::capabilities())
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let models = &files
        .iter()
        .find(|f| f.path.ends_with("models.py"))
        .unwrap()
        .contents;
    let client = &files
        .iter()
        .find(|f| f.path.ends_with("client.py"))
        .unwrap()
        .contents;
    let runtime = &files
        .iter()
        .find(|f| f.path.ends_with("streams.py"))
        .unwrap()
        .contents;
    assert!(runtime.contains("the default negative size reads to completion"));
    assert!(!runtime.contains("Read the asynchronous response stream to completion"));
    assert!(models.contains("https://example.com"));
    assert!(models.contains("line two"));
    assert!(models.contains("Label marker."));
    assert!(client.contains("Modeled errors: Problem"));
    assert!(client.contains("Construct") || client.contains("Construct with"));
    assert!(models.contains("def __repr__"));
}

#[test]
fn formatting_helper_names_are_reserved() {
    let service = Model::from_json(include_str!(
        "../../../tests/fixtures/client-docs-python-collisions.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let diagnostics = emit(&service, &options()).err().unwrap();
    assert!(diagnostics.to_string().contains("model_repr"));
}

/// The typed permissions fixture (`tests/fixtures/permissions`) as a client
/// sees it (docs/permissions.md §6.3), hostile documentation included: every
/// generated file that names a permission, checked in under
/// `tests/golden/permissions/`. Nothing of the internal tier appears.
#[test]
fn permissions_match_goldens() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    let service = crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap();
    let mut checked = 0;
    for file in emit(&service, &options()).unwrap() {
        if !file.contents.contains("graph:read") {
            continue;
        }
        checked += 1;
        assert!(
            !file.contents.contains("organization:read"),
            "{}",
            file.path
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/permissions")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}; rerun with CRUCIBLE_BLESS=1 and read the diff",
            file.path
        );
    }
    assert!(checked > 0, "no generated file names a permission");
}
