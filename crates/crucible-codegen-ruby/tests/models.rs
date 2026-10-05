use crucible_codegen_ruby::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

#[test]
fn weather_models_generate() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let options = Options {
        package_name: "weather_client".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    };
    let files = emit(&service, &options).unwrap();
    let again = emit(&service, &options).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| !f.path.contains("/runtime/"))
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden)
            .expect("golden")
            .replace("\r\n", "\n")
    );
    assert_eq!(
        files
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
    if let Some(output) = std::env::var_os("RUBY_TEST_OUTPUT") {
        for file in files {
            let path = Path::new(&output).join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.contents).unwrap();
        }
    }
}

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    }
}
fn weather() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
fn lower(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn invalid_package_and_root_module_are_diagnosed() {
    let service = lower(&weather());
    for name in ["../escape", "Weather", "weather-client", "", "3client"] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for name in [
        "String", "Array", "Object", "Time", "ENV", "BEGIN", "bad", "",
    ] {
        let mut config = options();
        config.module_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
}

#[test]
fn ambiguous_ruby_identifiers_are_collected() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary":"Registry"});
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "toWire".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "to_wire_value".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    let diagnostics = emit(&lower(&model), &options())
        .err()
        .expect("diagnostics")
        .to_string();
    assert!(diagnostics.contains("model name"));
    assert!(diagnostics.contains("member name"));
}

#[test]
fn case_conversion_cannot_overwrite_model_files() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] = serde_json::json!({"example.weather#CitySummary":"HTTPValue","example.weather#Location":"HttpValue"});
    assert!(emit(&lower(&model), &options()).is_err());
}

#[test]
fn top_level_support_names_are_diagnosed() {
    for name in ["WeatherClient", "Runtime", "Registry"] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        assert!(emit(&lower(&model), &options()).is_err(), "{name}");
    }
}
