use crucible_codegen_php::{Options, emit};
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
        package_name: "example/weather-client".into(),
        namespace: "Example\\Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    };
    let files = emit(&service, &options).unwrap();
    let again = emit(&service, &options).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| !f.path.starts_with("src/Runtime/"))
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(snapshot, fs::read_to_string(golden).expect("golden"));
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
    if let Some(output) = std::env::var_os("PHP_TEST_OUTPUT") {
        for file in files {
            let path = Path::new(&output).join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.contents).unwrap();
        }
    }
}

fn options() -> Options {
    Options {
        package_name: "example/weather-client".into(),
        namespace: "Example\\Weather".into(),
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
fn configuration_and_reserved_names_are_diagnosed() {
    let service = lower(&weather());
    for name in [
        "../escape",
        "Example/client",
        "example/",
        "example/../client",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for namespace in ["", "Example\\class", "Example\\__PROPERTY__", "../escape"] {
        let mut config = options();
        config.namespace = namespace.into();
        assert!(emit(&service, &config).is_err(), "{namespace}");
    }
    for name in ["__PROPERTY__", "String", "Decoder"] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        assert!(emit(&lower(&model), &options()).is_err(), "{name}");
    }
}
#[test]
fn model_field_and_union_collisions_are_collected() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#Coordinates":"citysummary"});
    for member in ["message", "messageValue"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][member] =
            serde_json::json!({"target":"smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["unknownVariant"] =
        serde_json::json!({"target":"smithy.api#String"});
    assert_eq!(emit(&lower(&model), &options()).err().unwrap().len(), 3);
}
#[test]
fn recursive_aliases_are_diagnosed_before_doc_type_expansion() {
    let mut model = weather();
    model["shapes"]["example.weather#CitySummaries"]["member"]["target"] =
        serde_json::json!("example.weather#CitySummaries");
    assert!(
        emit(&lower(&model), &options())
            .err()
            .unwrap()
            .iter()
            .any(|d| d.message.contains("recursive collection"))
    );
}
#[test]
fn reserved_enum_constants_are_escaped_with_collision_checks() {
    let mut model = weather();
    model["shapes"]["example.weather#Sky"]["members"]["DEFAULT"] =
        serde_json::json!({"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"default"}});
    let files = emit(&lower(&model), &options()).unwrap();
    assert!(
        files
            .iter()
            .find(|f| f.path.ends_with("Sky.php"))
            .unwrap()
            .contents
            .contains("const DEFAULT_VALUE = 'default'")
    );
    model["shapes"]["example.weather#Sky"]["members"]["DEFAULT_VALUE"] = serde_json::json!({"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"different"}});
    assert!(emit(&lower(&model), &options()).is_err());
}
